//! Real HTTP observations against the served schema and current authorization.
use super::*;
const ROUTE: &str = "/api/v2/memory/consumers/diagnose";
fn body(visibility: &str) -> Value {
    json!({"contract_version":2,"scope":memory_scope(visibility),"consumer_id":"cache","witness":null})
}
fn key(identity: &IdentityStore, admin: &str, subject: &str, caps: Vec<Capability>) -> String {
    let mut grant = spec();
    grant.subject_id = subject.into();
    grant.capabilities = caps.into_iter().collect();
    grant.grants = ["shared", "private"]
        .map(|visibility| serde_json::from_value(memory_scope(visibility)).unwrap())
        .into();
    identity.issue(admin, grant).unwrap().secret
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
            let mut bad = result.clone();
            bad["diagnostics"]["pending_positions"] = (-1).into();
            assert!(!validator.is_valid(&bad));
            let mut bad = result.clone();
            bad["diagnostics"]["checkpoint_status"] = "unknown".into();
            assert!(!validator.is_valid(&bad));
            if result["diagnostics"]["checkpoint_status"] != "compatible" {
                let mut bad = result.clone();
                bad["diagnostics"]["pending_positions"] = 0.into();
                assert!(
                    !validator.is_valid(&bad),
                    "Unknown distance must not masquerade as zero"
                );
            }
        }
        result
    }
}

#[tokio::test]
async fn consumer_diagnostics_http_observes_pending_progress_without_acknowledging() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("tenant", "operator").unwrap();
    let token = key(
        &identity,
        &admin.secret,
        "consumer",
        vec![
            Capability::MemoryRead,
            Capability::MemoryCheckpoint,
            Capability::MemoryWrite,
        ],
    );
    let server = Server::start(router.clone()).await;
    let missing = server.check(&token, body("shared"), 200).await;
    assert_eq!(missing["diagnostics"]["checkpoint_status"], "missing");
    assert_eq!(missing["diagnostics"]["pending_positions"], Value::Null);
    let activation = request(
        &router,
        "POST",
        "/api/v2/memory/changes/activate",
        &token,
        json!({"contract_version":2,"scope":memory_scope("shared")}),
    )
    .await;
    assert_eq!(activation.0, StatusCode::OK);
    let baseline = activation.1["baseline"].clone();
    let commit = json!({"scope":memory_scope("shared"),"command":{"contract_version":2,"consumer_id":"cache","idempotency_key":"initial","expected_revision":0,"expected_checkpoint_digest":null,"cursor":baseline}});
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v2/memory/checkpoints",
            &token,
            commit
        )
        .await
        .0,
        StatusCode::OK
    );
    for n in 0..2 {
        assert_eq!(
            request(
                &router,
                "POST",
                "/api/v1/memory/commands",
                &token,
                memory_create(&format!("source-{n}"), "sensitive body", "shared")
            )
            .await
            .0,
            StatusCode::OK
        );
    }
    let mut diagnostic = body("shared");
    diagnostic["witness"] = baseline.clone();
    let observed = server.check(&token, diagnostic.clone(), 200).await;
    assert_eq!(observed["diagnostics"]["pending_positions"], 2);
    assert_eq!(
        observed["diagnostics"]["checkpoint_relative_to_witness"],
        "equal"
    );
    assert_eq!(observed["diagnostics"]["checkpoint"]["revision"], 1);
    assert!(!observed.to_string().contains("sensitive body"));
    assert_eq!(
        observed,
        server.check(&token, diagnostic.clone(), 200).await
    );
    diagnostic["witness"] = observed["diagnostics"]["high_watermark"].clone();
    assert_eq!(
        server.check(&token, diagnostic.clone(), 200).await["diagnostics"]["checkpoint_relative_to_witness"],
        "before"
    );
    diagnostic["witness"]["prefix_digest"] = "f".repeat(64).into();
    let incompatible = server.check(&token, diagnostic.clone(), 200).await;
    assert_eq!(
        incompatible["diagnostics"]["witness_status"],
        "history_incompatible"
    );
    assert_eq!(incompatible["diagnostics"]["pending_positions"], 2);
    assert_eq!(
        incompatible["diagnostics"]["checkpoint_relative_to_witness"],
        Value::Null
    );
    diagnostic["witness"]["prefix_digest"] = "bad".into();
    server.check(&token, diagnostic, 400).await;
    let mut invalid = body("shared");
    invalid["subject_id"] = "other".into();
    server.check(&token, invalid, 400).await;
    let mut invalid = body("shared");
    invalid["contract_version"] = 1.into();
    server.check(&token, invalid, 400).await;
    for consumer in ["", "\n", &"x".repeat(129)] {
        let mut invalid = body("shared");
        invalid["consumer_id"] = consumer.into();
        server.check(&token, invalid, 400).await;
    }
}

#[tokio::test]
async fn consumer_diagnostics_http_enforces_scope_subject_capabilities_rotation_and_revocation() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("tenant", "operator").unwrap();
    let foreign = identity.bootstrap_tenant("foreign", "operator").unwrap();
    let caps = vec![
        Capability::MemoryRead,
        Capability::MemoryCheckpoint,
        Capability::MemoryWrite,
    ];
    let owner = key(&identity, &admin.secret, "owner", caps.clone());
    let other = key(&identity, &admin.secret, "other", caps.clone());
    let outside = key(&identity, &foreign.secret, "owner", caps.clone());
    let no_read = key(
        &identity,
        &admin.secret,
        "owner",
        vec![Capability::MemoryCheckpoint],
    );
    let no_checkpoint = key(
        &identity,
        &admin.secret,
        "owner",
        vec![Capability::MemoryRead],
    );
    let server = Server::start(router.clone()).await;
    for visibility in ["shared", "private"] {
        let activation = request(
            &router,
            "POST",
            "/api/v2/memory/changes/activate",
            &owner,
            json!({"contract_version":2,"scope":memory_scope(visibility)}),
        )
        .await;
        assert_eq!(activation.0, StatusCode::OK);
        let commit = json!({"scope":memory_scope(visibility),"command":{"contract_version":2,"consumer_id":"cache","idempotency_key":"initial","expected_revision":0,"cursor":activation.1["baseline"]}});
        assert_eq!(
            request(
                &router,
                "POST",
                "/api/v2/memory/checkpoints",
                &owner,
                commit
            )
            .await
            .0,
            StatusCode::OK
        );
        let own = server.check(&owner, body(visibility), 200).await;
        assert_eq!(own["diagnostics"]["checkpoint_status"], "compatible");
        let rotated = key(&identity, &admin.secret, "owner", caps.clone());
        assert_eq!(server.check(&rotated, body(visibility), 200).await, own);
        for (token, active) in [(&other, visibility == "shared"), (&outside, false)] {
            let diagnosis = server.check(token, body(visibility), 200).await;
            assert_eq!(diagnosis["diagnostics"]["active"], active);
            assert_eq!(diagnosis["diagnostics"]["checkpoint_status"], "missing");
            assert_eq!(diagnosis["diagnostics"]["checkpoint"], Value::Null);
            assert_eq!(diagnosis["diagnostics"]["pending_positions"], Value::Null);
            let mut with_witness = body(visibility);
            with_witness["witness"] = own["diagnostics"]["high_watermark"].clone();
            let diagnosis = server.check(token, with_witness, 200).await;
            assert_eq!(
                diagnosis["diagnostics"]["witness_status"],
                if active {
                    "compatible"
                } else {
                    "history_incompatible"
                }
            );
        }
        for token in [&admin.secret, &no_read, &no_checkpoint] {
            server.check(token, body(visibility), 403).await;
        }
    }
    for field in ["project_id", "mission_id", "agent_id"] {
        let mut wrong = body("shared");
        wrong["scope"][field] = "outside".into();
        server.check(&owner, wrong, 403).await;
    }
    let credential = identity.authenticate(&owner).unwrap();
    identity
        .revoke(&admin.secret, credential.id, credential.revision)
        .unwrap();
    server.check(&owner, body("shared"), 401).await;
}
