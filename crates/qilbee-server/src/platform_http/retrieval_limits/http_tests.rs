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
        f.check("/api/v1/memory/embeddings",json!({"contract_version":1,"scope":scope(),"idempotency_key":"limit","record_id":Uuid::new_v4(),"record_revision":1,"space":{"provider":"fixture","model":"capacity","revision":"v1","dimensions":dimensions},"vector":[1,0,0]}),400,Some("embedding_dimension_limit")).await;
    }
    for mode in ["lexical", "hybrid"] {
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
