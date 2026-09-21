//! Real TCP observations validated against the served OpenAPI.
use super::*;
use uuid::Uuid;
const ROUTE: &str = "/api/v1/memory/records/batch";
fn body(ids: Value, visibility: &str) -> Value {
    json!({"contract_version":1,"scope":memory_scope(visibility),"record_ids":ids})
}
struct Server {
    base: String,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Server {
    async fn start(router: Router) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        Self {
            base,
            task: tokio::spawn(async move { axum::serve(listener, router).await.unwrap() }),
        }
    }
    async fn check(&self, token: &str, request: Value, status: u16) -> Value {
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap();
        let api: Value = client
            .get(format!("{}/openapi.json", self.base))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let response = client
            .post(format!("{}{ROUTE}", self.base))
            .bearer_auth(token)
            .json(&request)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), status);
        assert_eq!(response.headers()["cache-control"], "no-store");
        let result: Value = response.json().await.unwrap();
        let schema = json!({"allOf":[api["paths"][ROUTE]["post"]["responses"][status.to_string()]["content"]["application/json"]["schema"]],"components":api["components"]});
        let validator = jsonschema::draft202012::options().build(&schema).unwrap();
        let errors: Vec<_> = validator
            .iter_errors(&result)
            .map(|e| e.to_string())
            .collect();
        assert!(errors.is_empty(), "{errors:?}");
        if status == 200 {
            assert_eq!(
                result["batch"]["entries"].as_array().unwrap().len(),
                request["record_ids"].as_array().unwrap().len()
            );
            for (entry, id) in result["batch"]["entries"]
                .as_array()
                .unwrap()
                .iter()
                .zip(request["record_ids"].as_array().unwrap())
            {
                assert_eq!(&entry["record_id"], id);
                if !entry["record"].is_null() {
                    assert_eq!(&entry["record"]["record_id"], id);
                }
            }
            let mut bad = result.clone();
            bad["batch"]["entries"][0]["record"] = "unavailable".into();
            assert!(!validator.is_valid(&bad));
            let mut bad = result.clone();
            bad["batch"]["record_bytes"] = 8_388_609.into();
            assert!(!validator.is_valid(&bad));
        }
        result
    }
}

#[tokio::test]
async fn batch_read_http_matches_current_records_and_rejects_malformed_batches() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("tenant", "operator").unwrap();
    let token = memory_key(&identity, &admin.secret, "owner", true);
    let server = Server::start(router.clone()).await;
    let mut ids = vec![];
    for key in ["first", "second"] {
        let created = request(
            &router,
            "POST",
            "/api/v1/memory/commands",
            &token,
            memory_create(key, key, "shared"),
        )
        .await;
        assert_eq!(created.0, StatusCode::OK);
        ids.push(created.1["receipt"]["record_id"].clone());
    }
    let query = body(json!([ids[1], Uuid::new_v4(), ids[0]]), "shared");
    let batch = server.check(&token, query.clone(), 200).await;
    assert!(batch["batch"]["entries"][1]["record"].is_null());
    for entry in [&batch["batch"]["entries"][0], &batch["batch"]["entries"][2]] {
        let single = request(
            &router,
            "GET",
            &record_url(entry["record_id"].as_str().unwrap(), "shared"),
            &token,
            Value::Null,
        )
        .await;
        assert_eq!(single.0, StatusCode::OK);
        assert_eq!(single.1["record"], entry["record"]);
    }
    let deleted=request(&router,"POST","/api/v1/memory/commands",&token,json!({"contract_version":1,"scope":memory_scope("shared"),"idempotency_key":"delete","operation":{"type":"delete","record_id":ids[0],"expected_revision":1}})).await;
    assert_eq!(deleted.0, StatusCode::OK);
    assert!(server.check(&token, query, 200).await["batch"]["entries"][2]["record"].is_null());
    for invalid in [
        json!([]),
        json!([ids[0], ids[0]]),
        json!(["invalid"]),
        json!(null),
        json!((0..101).map(|_| Uuid::new_v4()).collect::<Vec<_>>()),
    ] {
        server.check(&token, body(invalid, "shared"), 400).await;
    }
    for field in ["subject_id", "tenant_id"] {
        let mut forged = body(json!([ids[1]]), "shared");
        forged[field] = "another".into();
        server.check(&token, forged, 400).await;
    }
    let mut invalid = body(json!([ids[1]]), "shared");
    invalid["contract_version"] = 2.into();
    server.check(&token, invalid, 400).await;
    let valid = body(
        json!((0..100).map(|_| Uuid::new_v4()).collect::<Vec<_>>()),
        "shared",
    );
    assert!(
        server.check(&token, valid, 200).await["batch"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["record"].is_null())
    );
    let oversized = format!("{}{}", " ".repeat(65_536), body(json!([ids[1]]), "shared"));
    let response = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap()
        .post(format!("{}{ROUTE}", server.base))
        .bearer_auth(&token)
        .header("Content-Type", "application/json")
        .body(oversized)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let error: Value = response.json().await.unwrap();
    assert_eq!(error["error"]["code"], "invalid_request");
}

#[tokio::test]
async fn batch_read_http_isolates_tenants_private_subjects_and_every_scope_axis() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("tenant", "operator").unwrap();
    let foreign = identity.bootstrap_tenant("foreign", "operator").unwrap();
    let owner = memory_key(&identity, &admin.secret, "owner", true);
    let other = memory_key(&identity, &admin.secret, "other", true);
    let outside = memory_key(&identity, &foreign.secret, "owner", true);
    let mut write_spec = spec();
    write_spec.capabilities = [Capability::MemoryWrite].into();
    let write_only = identity.issue(&admin.secret, write_spec).unwrap().secret;
    let server = Server::start(router.clone()).await;
    let mut private_ids = vec![];
    for visibility in ["shared", "private"] {
        for (index, token) in [&owner, &other, &outside].iter().enumerate() {
            let created = request(
                &router,
                "POST",
                "/api/v1/memory/commands",
                token,
                memory_create(visibility, "identical content", visibility),
            )
            .await;
            assert_eq!(created.0, StatusCode::OK);
            let id = created.1["receipt"]["record_id"].clone();
            if visibility == "private" {
                private_ids.push(id.clone());
            }
            for (reader_index, reader) in [&owner, &other, &outside].iter().enumerate() {
                let page = server
                    .check(reader, body(json!([id]), visibility), 200)
                    .await;
                let allowed = if visibility == "shared" {
                    (index == 2) == (reader_index == 2)
                } else {
                    index == reader_index
                };
                assert_eq!(!page["batch"]["entries"][0]["record"].is_null(), allowed);
                if !allowed {
                    assert_eq!(page["batch"]["record_bytes"], 0);
                    assert_eq!(page["batch"]["dependency_work"]["records_examined"], 0);
                }
            }
        }
    }
    let mixed = server
        .check(&owner, body(json!(private_ids), "private"), 200)
        .await;
    assert!(!mixed["batch"]["entries"][0]["record"].is_null());
    assert!(mixed["batch"]["entries"][1]["record"].is_null());
    assert!(mixed["batch"]["entries"][2]["record"].is_null());
    let query = body(json!([private_ids[0]]), "private");
    for field in ["project_id", "agent_id", "mission_id"] {
        let mut changed = query.clone();
        changed["scope"][field] = "outside".into();
        server.check(&owner, changed.clone(), 403).await;
        let mut granted = spec();
        granted.subject_id = "owner".into();
        granted.grants = vec![serde_json::from_value(changed["scope"].clone()).unwrap()];
        let allowed = identity.issue(&admin.secret, granted).unwrap().secret;
        assert!(
            server.check(&allowed, changed, 200).await["batch"]["entries"][0]["record"].is_null()
        );
    }
    server
        .check(&write_only, body(json!([private_ids[0]]), "shared"), 403)
        .await;
    server.check(&admin.secret, query.clone(), 403).await;
    server.check("", query.clone(), 401).await;
    let current = identity.authenticate(&owner).unwrap();
    let rotated = identity
        .rotate(&admin.secret, current.id, current.revision)
        .unwrap();
    server.check(&owner, query.clone(), 401).await;
    assert!(
        !server.check(&rotated.secret, query.clone(), 200).await["batch"]["entries"][0]["record"]
            .is_null()
    );
    identity
        .revoke(
            &admin.secret,
            rotated.credential.id,
            rotated.credential.revision,
        )
        .unwrap();
    server.check(&rotated.secret, query, 401).await;
}
