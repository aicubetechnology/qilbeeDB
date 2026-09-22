//! Deterministic capacity failures through the production router and a TCP listener.
use super::*;
use crate::security::identity::ResourceScope;

struct Fixture {
    client: reqwest::Client,
    base: String,
    api: Value,
    token: String,
    identity: Arc<IdentityStore>,
    admin: String,
    limits: RetrievalLimits,
    server: tokio::task::JoinHandle<()>,
    _dir: tempfile::TempDir,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}

#[tokio::test]
async fn company_learning_capacity_errors_follow_current_administrative_authorization() {
    let mut f = Fixture::start().await;
    let requests = [
        (
            "/api/v1/company/learning/query",
            json!({"contract_version":1,"query":{"kind":"experience"}}),
        ),
        (
            "/api/v1/company/learning/read",
            json!({"contract_version":1,"resource":{"kind":"policy","id":"missing"}}),
        ),
        (
            "/api/v1/company/learning/evidence/query",
            json!({"contract_version":1,"query":{"resource":{"kind":"procedure","id":"missing","scope":{"project_id":"project","agent_id":"agent","mission_id":null,"visibility":"shared"},"private_subject_id":null},"kind":"evaluation_submission"}}),
        ),
        (
            "/api/v1/company/learning/evidence/read",
            json!({"contract_version":1,"evidence":{"resource":{"kind":"procedure","id":"missing","scope":{"project_id":"project","agent_id":"agent","mission_id":null,"visibility":"shared"},"private_subject_id":null},"kind":"evaluation_submission","id":"case"}}),
        ),
        (
            "/api/v1/company/learning/knowledge/inspect",
            json!({"contract_version":2,"resource":{"kind":"knowledge","id":"missing","scope":{"project_id":"project","agent_id":"agent","mission_id":null,"visibility":"shared"},"private_subject_id":null}}),
        ),
    ];
    let administrator = f
        .identity
        .issue(
            &f.admin,
            CredentialSpec {
                subject_id: "catalog-reader".into(),
                capabilities: [Capability::CredentialAdmin].into(),
                grants: vec![],
                scope_policy: None,
                expires_at_millis: None,
            },
        )
        .unwrap();
    let permit = f.limits.acquire().ok().unwrap();
    for (route, body) in &requests {
        f.check(route, body.clone(), 403, Some("forbidden")).await;
    }
    f.token = administrator.secret;
    for (route, body) in &requests {
        f.check(route, body.clone(), 503, Some("retrieval_busy"))
            .await;
    }
    drop(permit);
    f.check(requests[0].0, requests[0].1.clone(), 200, None)
        .await;
    f.check(
        requests[1].0,
        requests[1].1.clone(),
        404,
        Some("record_not_found"),
    )
    .await;
    for (route, body) in &requests[2..] {
        f.check(route, body.clone(), 404, Some("record_not_found"))
            .await;
    }
    f.identity
        .revoke(
            &f.admin,
            administrator.credential.id,
            administrator.credential.revision,
        )
        .unwrap();
    let _permit = f.limits.acquire().ok().unwrap();
    for (route, body) in requests {
        f.check(route, body, 401, Some("unauthorized")).await;
    }
}
#[tokio::test]
async fn agent_profile_read_admission_follows_current_company_authorization() {
    let mut f = Fixture::start().await;
    let requests = [
        (
            "/api/v1/company/agents/query",
            json!({"contract_version":1,"query":{}}),
        ),
        (
            "/api/v1/company/agents/read",
            json!({"contract_version":1,"agent_id":"missing"}),
        ),
        (
            "/api/v1/company/agents/history",
            json!({"contract_version":1,"agent_id":"missing"}),
        ),
    ];
    let key = f
        .identity
        .issue(
            &f.admin,
            CredentialSpec {
                subject_id: "profile-reader".into(),
                capabilities: [Capability::CredentialAdmin].into(),
                grants: vec![],
                scope_policy: None,
                expires_at_millis: None,
            },
        )
        .unwrap();
    let permit = f.limits.acquire().ok().unwrap();
    for (path, body) in &requests {
        f.check(path, body.clone(), 403, Some("forbidden")).await;
    }
    f.token = key.secret;
    for (path, body) in &requests {
        f.check(path, body.clone(), 503, Some("retrieval_busy"))
            .await;
    }
    f.check("/api/v1/company/agents/commands",json!({"contract_version":1,"command":{"agent_id":"missing","expected_revision":0,"display_name":"Name","idempotency_key":"attempt"}}),404,Some("record_not_found")).await;
    drop(permit);
    f.check(requests[0].0, requests[0].1.clone(), 200, None)
        .await;
    for (path, body) in &requests[1..] {
        f.check(path, body.clone(), 404, Some("record_not_found"))
            .await;
    }
    f.identity
        .revoke(&f.admin, key.credential.id, key.credential.revision)
        .unwrap();
    let _permit = f.limits.acquire().ok().unwrap();
    for (path, body) in requests {
        f.check(path, body, 401, Some("unauthorized")).await;
    }
}
fn scope() -> Value {
    json!({"project_id":"project","mission_id":null,"agent_id":"agent","visibility":"shared"})
}
fn query(mode: &str) -> (String, Value) {
    let space = json!({"provider":"fixture","model":"capacity","revision":"v1","dimensions":3});
    let mut body = json!({"contract_version":1,"scope":scope(),"query":{"space":space,"vector":[1,0,0],"limit":10,"scan_limit":10}});
    let route = match mode {
        "batch" => {
            body = json!({"contract_version":1,"scope":scope(),"record_ids":[Uuid::new_v4()]});
            "/api/v1/memory/records/batch"
        }
        "graph" => {
            body = json!({"contract_version":1,"scope":scope(),"query":{"root_record_ids":[Uuid::new_v4()]}});
            "/api/v1/memory/graph"
        }
        "graph_search" => {
            body = json!({"contract_version":1,"scope":scope(),"query":{"ranking_version":"typed_path_balanced_v1","seed":{"mode":"lexical","text":"content"},"limit":10}});
            "/api/v1/memory/search/graph"
        }
        "typed_graph" => {
            body = json!({"contract_version":1,"scope":scope(),"query":{"root_record_ids":[Uuid::new_v4()]}});
            "/api/v1/memory/graph/typed"
        }
        "semantic" => "/api/v1/memory/search",
        "hybrid" => {
            body["mode"] = "hybrid".into();
            body["query"]["text"] = "content".into();
            body["query"]["ranking_version"] = "weighted_rrf_v2".into();
            "/api/v1/memory/search/hybrid"
        }
        _ => {
            body["mode"] = "lexical".into();
            body["query"] = json!({"text":"content","limit":10,"scan_limit":10});
            "/api/v1/memory/search/lexical"
        }
    };
    (route.into(), body)
}
impl Fixture {
    async fn start() -> Self {
        let dir = tempfile::TempDir::new().unwrap();
        let database = Arc::new(Database::open_for_testing(dir.path()).unwrap());
        let identity = Arc::new(IdentityStore::new(Arc::new(database.storage().clone())));
        let admin = identity
            .bootstrap_tenant("tenant", "operator")
            .unwrap()
            .secret;
        let grant = CredentialSpec {
            scope_policy: None,
            subject_id: "consumer".into(),
            capabilities: [
                Capability::MemoryRead,
                Capability::MemoryWrite,
                Capability::MemoryReview,
                Capability::MemoryCheckpoint,
            ]
            .into(),
            grants: vec![serde_json::from_value::<ResourceScope>(scope()).unwrap()],
            expires_at_millis: None,
        };
        let token = identity.issue(&admin, grant).unwrap().secret;
        let limits = RetrievalLimits::from_lookup(|key| match key {
            "QILBEE_MAX_EMBEDDING_DIMENSIONS" => Some("3".into()),
            "QILBEE_MAX_CONCURRENT_RETRIEVALS" => Some("1".into()),
            _ => None,
        })
        .unwrap();
        let router = super::super::create_router_with_limits(database, limits.clone()).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap();
        let api = client
            .get(format!("{base}/openapi.json"))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        Self {
            client,
            base,
            api,
            token,
            identity,
            admin,
            limits,
            server,
            _dir: dir,
        }
    }
    async fn check(&self, route: &str, body: Value, status: u16, code: Option<&str>) {
        let response = self
            .client
            .post(format!("{}{route}", self.base))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), status, "{route}");
        assert_eq!(response.headers()["cache-control"], "no-store");
        let result: Value = response.json().await.unwrap();
        if let Some(code) = code {
            assert_eq!(result["error"]["code"], code);
        }
        let schema = &self.api["paths"][route]["post"]["responses"][status.to_string()]["content"]
            ["application/json"]["schema"];
        assert!(!schema.is_null(), "Missing {route} {status}");
        let document = json!({"allOf":[schema],"components":self.api["components"]});
        let validator = jsonschema::draft202012::options().build(&document).unwrap();
        let errors: Vec<_> = validator
            .iter_errors(&result)
            .map(|e| e.to_string())
            .collect();
        assert!(errors.is_empty(), "{route}: {errors:?}");
        if status == 503 || status == 404 {
            let mut wrong = result.clone();
            wrong["error"]["code"] = "invalid_request".into();
            assert!(
                !validator.is_valid(&wrong),
                "Status-specific error codes must be enforced"
            );
        }
    }
}

#[tokio::test]
async fn published_request_limits_match_real_http_boundaries() {
    let f = Fixture::start().await;
    assert!(!f.api.to_string().contains("Unreleased 0.13.0"));
    for route in [
        "/api/v1/memory/embeddings",
        "/api/v1/memory/search",
        "/api/v1/memory/search/hybrid",
        "/api/v1/memory/search/graph",
    ] {
        let operation = &f.api["paths"][route]["post"];
        assert_eq!(
            operation["x-qilbee-max-request-body-bytes"],
            VECTOR_BODY_BYTES
        );
        assert!(
            operation["responses"]["413"]["description"]
                .as_str()
                .unwrap()
                .contains("2,097,152 bytes")
        );
    }
    for (mode, limit) in [
        ("graph_search", VECTOR_BODY_BYTES),
        ("typed_graph", 64 * 1024),
    ] {
        let (route, body) = query(mode);
        let encoded = serde_json::to_string(&body).unwrap();
        // Valid JSON padding tests transport bytes without violating field bounds.
        for (bytes, status) in [(limit, 200), (limit + 1, 413)] {
            let mut padded = encoded.clone();
            padded.extend(std::iter::repeat_n(' ', bytes - padded.len()));
            let response = f
                .client
                .post(format!("{}{route}", f.base))
                .bearer_auth(&f.token)
                .header("content-type", "application/json")
                .body(padded)
                .send()
                .await
                .unwrap();
            assert_eq!(response.status().as_u16(), status, "{route}: {bytes} bytes");
            assert_eq!(response.headers()["cache-control"], "no-store");
            let value: Value = response.json().await.unwrap();
            let schema = &f.api["paths"][&route]["post"]["responses"][status.to_string()]["content"]
                ["application/json"]["schema"];
            let document = json!({"allOf":[schema],"components":f.api["components"]});
            assert!(
                jsonschema::draft202012::options()
                    .build(&document)
                    .unwrap()
                    .is_valid(&value)
            );
        }
    }
}

#[tokio::test]
async fn dimension_and_scan_limits_match_served_schemas() {
    let f = Fixture::start().await;
    for mode in ["semantic", "hybrid"] {
        let (route, body) = query(mode);
        for dimensions in [0, 4] {
            let mut bad = body.clone();
            bad["query"]["space"]["dimensions"] = dimensions.into();
            f.check(&route, bad, 400, Some("embedding_dimension_limit"))
                .await;
        }
    }
    for dimensions in [0, 4] {
        let (route, mut body) = query("graph_search");
        body["query"]["seed"] = json!({"mode":"semantic","space":{"provider":"fixture","model":"capacity","revision":"v1","dimensions":dimensions},"vector":[1,0,0]});
        f.check(&route, body, 400, Some("embedding_dimension_limit"))
            .await;
    }
    for dimensions in [0, 4] {
        f.check("/api/v1/memory/embeddings",json!({"contract_version":1,"scope":scope(),"idempotency_key":"limit","record_id":Uuid::new_v4(),"record_revision":1,"space":{"provider":"fixture","model":"capacity","revision":"v1","dimensions":dimensions},"vector":[1,0,0]}),400,Some("embedding_dimension_limit")).await;
    }
    for mode in ["lexical", "hybrid", "graph_search"] {
        let (route, body) = query(mode);
        for bytes in [0, 67_108_865] {
            let mut bad = body.clone();
            bad["query"]["scan_bytes_limit"] = bytes.into();
            f.check(&route, bad, 400, Some("retrieval_scan_limit"))
                .await;
        }
    }
}

#[tokio::test]
async fn busy_slots_preserve_authorization_precedence_and_release() {
    let f = Fixture::start().await;
    let permit = f.limits.acquire().ok().unwrap();
    for mode in [
        "semantic",
        "lexical",
        "hybrid",
        "batch",
        "graph",
        "typed_graph",
        "graph_search",
    ] {
        let (route, body) = query(mode);
        f.check(&route, body.clone(), 503, Some("retrieval_busy"))
            .await;
        let mut wrong = body;
        wrong["scope"]["project_id"] = "outside".into();
        f.check(&route, wrong, 403, Some("forbidden")).await;
    }
    drop(permit);
    for mode in [
        "semantic",
        "lexical",
        "hybrid",
        "batch",
        "graph",
        "typed_graph",
        "graph_search",
    ] {
        let (route, body) = query(mode);
        f.check(&route, body, 200, None).await;
    }
    let principal = f.identity.authenticate(&f.token).unwrap();
    f.identity
        .revoke(&f.admin, principal.id, principal.revision)
        .unwrap();
    let _permit = f.limits.acquire().ok().unwrap();
    let (route, body) = query("semantic");
    f.check(&route, body, 401, Some("unauthorized")).await;
}

#[tokio::test]
async fn typed_relation_routes_share_admission_after_authorization() {
    let f = Fixture::start().await;
    let id = Uuid::new_v4();
    let requests = [
        (
            "/api/v1/memory/relations/commands",
            json!({"contract_version":1,"scope":scope(),"idempotency_key":"retire-missing","operation":{"type":"retire","relation_id":id,"expected_revision":1,"evidence_ref":"trace://capacity"}}),
        ),
        (
            "/api/v1/memory/relations/read",
            json!({"contract_version":1,"scope":scope(),"relation_id":id}),
        ),
        (
            "/api/v1/memory/relations/inspect",
            json!({"contract_version":1,"scope":scope(),"relation_id":id}),
        ),
        (
            "/api/v1/memory/relations/revision",
            json!({"contract_version":1,"scope":scope(),"relation_id":id,"revision":1}),
        ),
    ];
    let permit = f.limits.acquire().ok().unwrap();
    for (route, body) in &requests {
        f.check(route, body.clone(), 503, Some("retrieval_busy"))
            .await;
        let mut unauthorized = body.clone();
        unauthorized["scope"]["agent_id"] = "outside".into();
        f.check(route, unauthorized, 403, Some("forbidden")).await;
    }
    drop(permit);
    for (route, body) in &requests {
        f.check(route, body.clone(), 404, Some("record_not_found"))
            .await;
    }
    let principal = f.identity.authenticate(&f.token).unwrap();
    f.identity
        .revoke(&f.admin, principal.id, principal.revision)
        .unwrap();
    let _permit = f.limits.acquire().ok().unwrap();
    for (route, body) in requests {
        f.check(route, body, 401, Some("unauthorized")).await;
    }
}

#[tokio::test]
async fn absent_reviews_match_served_schemas_without_cross_scope_probes() {
    let f = Fixture::start().await;
    for route in [
        "/api/v1/memory/reviews/state",
        "/api/v1/memory/reviews/read",
    ] {
        let mut body = json!({"contract_version":1,"scope":scope(),"record_id":Uuid::new_v4()});
        if route.ends_with("/read") {
            body["revision"] = 1.into();
        }
        f.check(route, body.clone(), 404, Some("review_not_found"))
            .await;
        body["scope"]["agent_id"] = "outside".into();
        f.check(route, body, 403, Some("forbidden")).await;
    }
}

#[tokio::test]
async fn existing_unreviewed_record_has_state_but_no_immutable_review_receipt() {
    let f = Fixture::start().await;
    let create = json!({"contract_version":1,"scope":scope(),"idempotency_key":"unreviewed",
        "operation":{"type":"create","record":{"episode_type":"Observation","content":{"primary":"current"},"event_time_millis":1700000000000i64}}});
    let response = f
        .client
        .post(format!("{}/api/v1/memory/commands", f.base))
        .bearer_auth(&f.token)
        .json(&create)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 200);
    let receipt: Value = response.json().await.unwrap();
    let body =
        json!({"contract_version":1,"scope":scope(),"record_id":receipt["receipt"]["record_id"]});
    f.check("/api/v1/memory/reviews/state", body.clone(), 200, None)
        .await;
    let mut revision = body;
    revision["revision"] = 1.into();
    f.check(
        "/api/v1/memory/reviews/read",
        revision,
        404,
        Some("review_not_found"),
    )
    .await;
}

#[tokio::test]
async fn company_inventory_uses_shared_admission_after_company_authorization() {
    let mut f = Fixture::start().await;
    let reader = f.token.clone();
    let permit = f.limits.acquire().ok().unwrap();
    let queries = [
        (
            "/api/v1/company/memory/graph/typed",
            json!({"contract_version":1,"workspace_id":"0".repeat(64),"query":{"root_record_ids":[Uuid::new_v4()]}}),
        ),
        (
            "/api/v1/company/memory/relations/inspect",
            json!({"contract_version":1,"workspace_id":"0".repeat(64),"relation_id":Uuid::new_v4()}),
        ),
        (
            "/api/v1/company/memory/relations/revision",
            json!({"contract_version":1,"workspace_id":"0".repeat(64),"relation_id":Uuid::new_v4(),"revision":1}),
        ),
        (
            "/api/v1/company/memory/graph",
            json!({"contract_version":1,"workspace_id":"0".repeat(64),"query":{"root_record_ids":[Uuid::new_v4()]}}),
        ),
        (
            "/api/v1/company/memory/query",
            json!({"contract_version":1,"workspace_id":"0".repeat(64),"filter":{}}),
        ),
        (
            "/api/v1/company/memory/read",
            json!({"contract_version":1,"workspace_id":"0".repeat(64),"record_id":Uuid::new_v4()}),
        ),
    ];
    for (route, body) in &queries {
        f.token = reader.clone();
        f.check(route, body.clone(), 403, Some("forbidden")).await;
        f.token = f.admin.clone();
        f.check(route, body.clone(), 503, Some("retrieval_busy"))
            .await;
    }
    let route = "/api/v1/company/memory/workspaces";
    for (token, status) in [(&reader, 403), (&f.admin, 503)] {
        let response = f
            .client
            .get(format!("{}{route}?contract_version=1", f.base))
            .bearer_auth(token)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), status);
        let body: Value = response.json().await.unwrap();
        let schema = json!({"allOf":[f.api["paths"][route]["get"]["responses"][status.to_string()]["content"]["application/json"]["schema"]],"components":f.api["components"]});
        jsonschema::draft202012::options()
            .build(&schema)
            .unwrap()
            .validate(&body)
            .unwrap();
    }
    drop(permit);
    for (route, body) in queries {
        f.check(route, body, 404, Some("record_not_found")).await;
    }
}

#[tokio::test]
async fn relation_feed_and_checkpoint_routes_share_admission_after_authorization() {
    let f = Fixture::start().await;
    let prefix = "/api/v1/memory/relations";
    let activate = format!("{prefix}/changes/activate");
    let common = json!({"contract_version":1,"scope":scope()});
    let baseline: Value = f
        .client
        .post(format!("{}{activate}", f.base))
        .bearer_auth(&f.token)
        .json(&common)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let requests = vec![
        (
            format!("{prefix}/changes"),
            json!({"contract_version":1,"scope":scope(),"query":{"limit":1}}),
        ),
        (activate, common),
        (
            format!("{prefix}/checkpoints"),
            json!({"scope":scope(),"command":{"contract_version":1,"idempotency_key":"initial","consumer_id":"cache","expected_revision":0,"operation":{"type":"advance","cursor":baseline["baseline"]}}}),
        ),
        (
            format!("{prefix}/checkpoints/read"),
            json!({"contract_version":1,"scope":scope(),"consumer_id":"cache"}),
        ),
        (
            format!("{prefix}/checkpoints/revision"),
            json!({"contract_version":1,"scope":scope(),"consumer_id":"cache","revision":1}),
        ),
        (
            format!("{prefix}/consumers/diagnose"),
            json!({"contract_version":1,"scope":scope(),"consumer_id":"cache"}),
        ),
    ];
    let permit = f.limits.acquire().ok().unwrap();
    for (route, body) in &requests {
        f.check(route, body.clone(), 503, Some("retrieval_busy"))
            .await;
        let mut unauthorized = body.clone();
        unauthorized["scope"]["project_id"] = "outside".into();
        f.check(route, unauthorized, 403, Some("forbidden")).await;
    }
    drop(permit);
    for (route, body) in &requests {
        f.check(route, body.clone(), 200, None).await;
    }
    let principal = f.identity.authenticate(&f.token).unwrap();
    f.identity
        .revoke(&f.admin, principal.id, principal.revision)
        .unwrap();
    let _permit = f.limits.acquire().ok().unwrap();
    for (route, body) in requests {
        f.check(&route, body, 401, Some("unauthorized")).await;
    }
}

#[tokio::test]
async fn company_consumer_capacity_errors_follow_current_administrative_authorization() {
    let mut f = Fixture::start().await;
    let requests = [
        (
            "/api/v1/company/memory/consumers/query",
            json!({"contract_version":1,"query":{"kind":"memory_v2"}}),
        ),
        (
            "/api/v1/company/memory/consumers/read",
            json!({"contract_version":1,"consumer":{"kind":"memory_v2","scope":{"project_id":"project","agent_id":"agent","mission_id":null,"visibility":"shared"},"private_subject_id":null,"subject_id":"owner","consumer_id":"cache"}}),
        ),
    ];
    let administrator = f
        .identity
        .issue(
            &f.admin,
            CredentialSpec {
                subject_id: "catalog-reader".into(),
                capabilities: [Capability::CredentialAdmin].into(),
                grants: vec![],
                scope_policy: None,
                expires_at_millis: None,
            },
        )
        .unwrap();
    let permit = f.limits.acquire().ok().unwrap();
    for (route, body) in &requests {
        f.check(route, body.clone(), 403, Some("forbidden")).await;
    }
    f.token = administrator.secret;
    for (route, body) in &requests {
        f.check(route, body.clone(), 503, Some("retrieval_busy"))
            .await;
    }
    drop(permit);
    f.check(requests[0].0, requests[0].1.clone(), 200, None)
        .await;
    f.check(
        requests[1].0,
        requests[1].1.clone(),
        404,
        Some("record_not_found"),
    )
    .await;
    for (route, body) in &requests[2..] {
        f.check(route, body.clone(), 404, Some("record_not_found"))
            .await;
    }
    f.identity
        .revoke(
            &f.admin,
            administrator.credential.id,
            administrator.credential.revision,
        )
        .unwrap();
    let _permit = f.limits.acquire().ok().unwrap();
    for (route, body) in requests {
        f.check(route, body, 401, Some("unauthorized")).await;
    }
}

#[tokio::test]
async fn knowledge_read_capacity_does_not_bypass_scope_authorization() {
    let f = Fixture::start().await;
    let requests = [
        (
            "/api/v1/learning/knowledge/inspect",
            json!({"contract_version":2,"scope":scope(),"procedure_id":"missing"}),
        ),
        (
            "/api/v1/learning/knowledge/select",
            json!({"contract_version":2,"scope":scope(),"policy_id":"policy","context_id":"context","max_instruction_bytes":4096,"candidate_limit":10,"external_tool_identities":[]}),
        ),
    ];
    let _permit = f.limits.acquire().ok().unwrap();
    for (path, body) in requests {
        f.check(path, body.clone(), 503, Some("retrieval_busy"))
            .await;
        let mut forbidden = body;
        forbidden["scope"]["agent_id"] = "another-agent".into();
        f.check(path, forbidden, 403, Some("forbidden")).await;
    }
}

#[tokio::test]
async fn metadata_discovery_checks_live_company_authority_before_capacity() {
    let mut f = Fixture::start().await;
    let route = "/api/v1/learning/metadata/query";
    let body = json!({"contract_version":1,"query":{"kind":"policy"}});
    let permit = f.limits.acquire().ok().unwrap();
    f.check(route, body.clone(), 403, Some("forbidden")).await;
    let reader = f
        .identity
        .issue(
            &f.admin,
            CredentialSpec {
                scope_policy: None,
                subject_id: "metadata-reader".into(),
                capabilities: [Capability::LearningMetadataRead].into(),
                grants: vec![],
                expires_at_millis: None,
            },
        )
        .unwrap();
    f.token = reader.secret.clone();
    f.check(route, body.clone(), 503, Some("retrieval_busy"))
        .await;
    drop(permit);
    f.check(route, body.clone(), 200, None).await;
    f.identity
        .revoke(&f.admin, reader.credential.id, reader.credential.revision)
        .unwrap();
    let _permit = f.limits.acquire().ok().unwrap();
    f.check(route, body, 401, Some("unauthorized")).await;
}
