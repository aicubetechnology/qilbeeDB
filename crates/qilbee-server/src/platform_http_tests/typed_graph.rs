//! Real HTTP, tenant administration and recovery of typed graph projections.
use super::administration::Client;
use super::*;
use uuid::Uuid;
mod recovery;
const GRAPH: &str = "/api/v1/memory/graph/typed";
const COMPANY: &str = "/api/v1/company/memory/graph/typed";
const INSPECT: &str = "/api/v1/company/memory/relations/inspect";
const HISTORY: &str = "/api/v1/company/memory/relations/revision";
const MEMORY: &str = "/api/v1/memory/commands";
const RELATIONS: &str = "/api/v1/memory/relations/commands";
struct Server {
    http: Client,
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
        let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
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
            .json()
            .await
            .unwrap();
        Self {
            http: Client { client, base, api },
            task,
        }
    }
}
fn token(identity: &IdentityStore, admin: &str, subject: &str) -> String {
    let mut definition = spec();
    definition.subject_id = subject.into();
    definition.capabilities = [
        Capability::MemoryRead,
        Capability::MemoryWrite,
        Capability::MemoryReview,
    ]
    .into();
    definition.grants.push(ResourceScope {
        visibility: Visibility::Private,
        ..definition.grants[0].clone()
    });
    for field in ["project_id", "agent_id", "mission_id"] {
        let mut grant = serde_json::to_value(&definition.grants[1]).unwrap();
        grant[field] = "other".into();
        definition
            .grants
            .push(serde_json::from_value(grant).unwrap());
    }
    identity.issue(admin, definition).unwrap().secret
}
fn request_graph(ids: Value) -> Value {
    json!({"contract_version":1,"scope":memory_scope("private"),"query":{"root_record_ids":ids}})
}
fn assertion(a: &Value, b: &Value, key: &str, kind: &str) -> Value {
    json!({"contract_version":1,"scope":memory_scope("private"),"idempotency_key":key,"operation":{"type":"assert","relation":{"source":{"record_id":a["record_id"],"revision":a["revision"]},"target":{"record_id":b["record_id"],"revision":b["revision"]},"kind":kind,"provenance":{"origin":"tool_observation","method":"fixture","method_revision":"v1","evidence_ref":"trace://typed-graph"}}}})
}
async fn create(http: &Client, token: &str, key: &str) -> Value {
    http.call(
        "POST",
        MEMORY,
        MEMORY,
        token,
        memory_create(key, "Same fixture content", "private"),
        200,
    )
    .await["receipt"]
        .clone()
}

#[tokio::test]
async fn typed_graph_real_http_preserves_direction_coverage_and_authorized_scope() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("company", "owner").unwrap();
    let foreign = identity.bootstrap_tenant("other", "owner").unwrap();
    let alice = token(&identity, &admin.secret, "alice");
    let bob = token(&identity, &admin.secret, "bob");
    let other = token(&identity, &foreign.secret, "alice");
    let server = Server::start(router).await;
    let http = &server.http;
    let a = create(http, &alice, "a").await;
    let b = create(http, &alice, "b").await;
    let c = create(http, &alice, "c").await;
    let ab = http
        .call(
            "POST",
            RELATIONS,
            RELATIONS,
            &alice,
            assertion(&a, &b, "ab", "supports"),
            200,
        )
        .await["receipt"]
        .clone();
    let cb = http
        .call(
            "POST",
            RELATIONS,
            RELATIONS,
            &alice,
            assertion(&c, &b, "cb", "causal_claim"),
            200,
        )
        .await["receipt"]
        .clone();
    // Identical content in other companies and private subjects cannot contribute candidates.
    for key in [&bob, &other] {
        create(http, key, "a").await;
        create(http, key, "b").await;
    }
    let request = request_graph(json!([a["record_id"]]));
    let full = http
        .call("POST", GRAPH, GRAPH, &alice, request.clone(), 200)
        .await;
    assert_eq!(full["graph"]["nodes"].as_array().unwrap().len(), 3);
    assert_eq!(full["graph"]["edges"].as_array().unwrap().len(), 2);
    assert_eq!(full["graph"]["coverage"]["complete"], true);
    assert_eq!(full["graph"]["coverage"]["adjacency_entries_examined"], 4);
    assert_eq!(full["graph"]["coverage"]["relations_examined"], 2);
    let edges = full["graph"]["edges"].as_array().unwrap();
    assert!(edges.iter().any(|e| e["relation_id"] == ab["relation_id"]));
    assert!(edges.iter().any(|e| e["relation_id"] == cb["relation_id"]
        && e["input"]["kind"] == "causal_claim"
        && e["input"]["source"]["record_id"] == c["record_id"]));
    for key in [&bob, &other] {
        let hidden = http
            .call("POST", GRAPH, GRAPH, key, request.clone(), 200)
            .await;
        assert_eq!(hidden["graph"]["roots"][0]["status"], "unavailable");
        assert_eq!(hidden["graph"]["coverage"]["relations_examined"], 0);
        assert_eq!(hidden["graph"]["coverage"]["record_bytes"], 0);
    }
    for field in ["project_id", "agent_id", "mission_id"] {
        let mut body = request.clone();
        body["scope"][field] = "other".into();
        let hidden = http
            .call("POST", GRAPH, GRAPH, &alice, body.clone(), 200)
            .await;
        assert_eq!(hidden["graph"]["nodes"], json!([]));
        body["scope"][field] = "forbidden".into();
        http.call("POST", GRAPH, GRAPH, &alice, body, 403).await;
    }
    let mut outgoing = request.clone();
    outgoing["query"]["direction"] = "outgoing".into();
    let out = http.call("POST", GRAPH, GRAPH, &alice, outgoing, 200).await;
    assert_eq!(out["graph"]["nodes"].as_array().unwrap().len(), 2);
    assert_eq!(out["graph"]["edges"].as_array().unwrap().len(), 1);
    let mut incoming = request_graph(json!([b["record_id"]]));
    incoming["query"]["direction"] = "incoming".into();
    let back = http.call("POST", GRAPH, GRAPH, &alice, incoming, 200).await;
    assert_eq!(back["graph"]["edges"].as_array().unwrap().len(), 2);
    let mut filtered = request.clone();
    filtered["query"]["relation_kinds"] = json!(["causal_claim"]);
    let result = http.call("POST", GRAPH, GRAPH, &alice, filtered, 200).await;
    assert_eq!(result["graph"]["edges"], json!([]));
    assert_eq!(result["graph"]["coverage"]["complete"], true);
    let mut zero = request.clone();
    zero["query"]["max_depth"] = 0.into();
    let cut = http.call("POST", GRAPH, GRAPH, &alice, zero, 200).await;
    assert_eq!(
        cut["graph"]["coverage"]["stop_reasons"],
        json!(["depth_limit"])
    );
    let schema = json!({"$ref":"#/components/schemas/TypedMemoryGraphResponse","components":http.api["components"]});
    let validator = jsonschema::draft202012::options().build(&schema).unwrap();
    let mut lie = cut.clone();
    lie["graph"]["coverage"]["complete"] = true.into();
    assert!(!validator.is_valid(&lie));
    let mut all = request_graph(json!([a["record_id"], b["record_id"], c["record_id"]]));
    all["query"]["max_depth"] = 0.into();
    let all = http.call("POST", GRAPH, GRAPH, &alice, all, 200).await;
    assert_eq!(all["graph"]["coverage"]["complete"], true);
    assert_eq!(all["graph"]["edges"].as_array().unwrap().len(), 2);
    let mut limit = request_graph(json!([Uuid::new_v4(), a["record_id"]]));
    limit["query"]["node_limit"] = 1.into();
    let roots = http.call("POST", GRAPH, GRAPH, &alice, limit, 200).await;
    assert_eq!(roots["graph"]["roots"][0]["status"], "unavailable");
    assert_eq!(roots["graph"]["roots"][1]["status"], "not_examined");
    let mut limit = request.clone();
    limit["query"]["scan_limit"] = 1.into();
    let cut = http.call("POST", GRAPH, GRAPH, &alice, limit, 200).await;
    assert_eq!(cut["graph"]["coverage"]["adjacency_entries_examined"], 1);
    assert_eq!(cut["graph"]["coverage"]["complete"], false);
    http.call("POST", GRAPH, GRAPH, "", request.clone(), 401)
        .await;
    let credential = identity.authenticate(&alice).unwrap();
    identity
        .revoke(&admin.secret, credential.id, credential.revision)
        .unwrap();
    http.call("POST", GRAPH, GRAPH, &alice, request, 401).await;
}

#[tokio::test]
async fn typed_graph_company_admin_reads_private_claims_and_retained_history_without_delegation() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("company", "owner").unwrap();
    let other = identity.bootstrap_tenant("other", "owner").unwrap();
    let alice = token(&identity, &admin.secret, "alice");
    let server = Server::start(router).await;
    let http = &server.http;
    let a = create(http, &alice, "a").await;
    let b = create(http, &alice, "b").await;
    let edge = http
        .call(
            "POST",
            RELATIONS,
            RELATIONS,
            &alice,
            assertion(&a, &b, "edge", "same_entity"),
            200,
        )
        .await["receipt"]
        .clone();
    let directory = "/api/v1/company/memory/workspaces";
    let workspaces = http
        .call(
            "GET",
            &format!("{directory}?contract_version=1"),
            directory,
            &admin.secret,
            Value::Null,
            200,
        )
        .await;
    let workspace = &workspaces["page"]["workspaces"][0]["workspace_id"];
    let graph = json!({"contract_version":1,"workspace_id":workspace,"query":{"root_record_ids":[a["record_id"]]}});
    let inspection =
        json!({"contract_version":1,"workspace_id":workspace,"relation_id":edge["relation_id"]});
    let mut revision = inspection.clone();
    revision["revision"] = 1.into();
    let credentials = "/api/v1/credentials";
    let before = http
        .call(
            "GET",
            &format!("{credentials}?contract_version=1"),
            credentials,
            &admin.secret,
            Value::Null,
            200,
        )
        .await;
    for (route, body) in [
        (COMPANY, &graph),
        (INSPECT, &inspection),
        (HISTORY, &revision),
    ] {
        http.call("POST", route, route, &alice, body.clone(), 403)
            .await;
        http.call("POST", route, route, &other.secret, body.clone(), 404)
            .await;
        http.call("POST", route, route, "", body.clone(), 401).await;
    }
    let result = http
        .call("POST", COMPANY, COMPANY, &admin.secret, graph.clone(), 200)
        .await;
    assert_eq!(result["workspace"]["private_subject_id"], "alice");
    assert_eq!(result["graph"]["edges"].as_array().unwrap().len(), 1);
    let review = json!({"contract_version":1,"scope":memory_scope("private"),"idempotency_key":"reject","operation":{"type":"review","relation_id":edge["relation_id"],"expected_revision":1,"disposition":"rejected","evidence_ref":"trace://rejection"}});
    http.call("POST", RELATIONS, RELATIONS, &alice, review, 200)
        .await;
    let retained = http
        .call(
            "POST",
            INSPECT,
            INSPECT,
            &admin.secret,
            inspection.clone(),
            200,
        )
        .await;
    assert_eq!(retained["inspection"]["eligibility"]["reason"], "rejected");
    assert_eq!(retained["inspection"]["relation"]["revision"], 2);
    let history = http
        .call(
            "POST",
            HISTORY,
            HISTORY,
            &admin.secret,
            revision.clone(),
            200,
        )
        .await;
    assert_eq!(history["history"]["receipt"], edge);
    assert!(history["history"]["relation"]["review"].is_null());
    let no_edge = http
        .call("POST", COMPANY, COMPANY, &admin.secret, graph.clone(), 200)
        .await;
    assert_eq!(no_edge["graph"]["edges"], json!([]));
    assert_eq!(no_edge["graph"]["coverage"]["complete"], true);
    revision["revision"] = 999.into();
    http.call(
        "POST",
        HISTORY,
        HISTORY,
        &admin.secret,
        revision.clone(),
        404,
    )
    .await;
    revision["revision"] = 0.into();
    http.call("POST", HISTORY, HISTORY, &admin.secret, revision, 400)
        .await;
    let after = http
        .call(
            "GET",
            &format!("{credentials}?contract_version=1"),
            credentials,
            &admin.secret,
            Value::Null,
            200,
        )
        .await;
    assert_eq!(
        after, before,
        "Administrative reads must not issue or rotate any credential"
    );
    let principal = identity.authenticate(&alice).unwrap();
    identity
        .revoke(&admin.secret, principal.id, principal.revision)
        .unwrap();
    http.call("POST", INSPECT, INSPECT, &admin.secret, inspection, 200)
        .await;
    http.call("POST", COMPANY, COMPANY, &admin.secret, graph, 200)
        .await;
}

#[tokio::test]
async fn typed_graph_http_rejects_unknown_fields_invalid_bounds_and_oversized_bodies() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("company", "owner").unwrap();
    let key = token(&identity, &admin.secret, "alice");
    let server = Server::start(router).await;
    let http = &server.http;
    let id = Uuid::new_v4();
    let request = request_graph(json!([id]));
    for (field, value) in [
        ("root_record_ids", json!([])),
        ("root_record_ids", json!([id, id])),
        ("direction", json!("undirected")),
        ("relation_kinds", json!([])),
        ("relation_kinds", json!(["supports", "supports"])),
        ("max_depth", json!(9)),
        ("node_limit", json!(257)),
        ("edge_limit", json!(0)),
        ("scan_limit", json!(4097)),
        ("ranking_weight", json!(0.5)),
    ] {
        let mut body = request.clone();
        body["query"][field] = value;
        http.call("POST", GRAPH, GRAPH, &key, body, 400).await;
    }
    let mut body = request.clone();
    body["company_id"] = "other".into();
    http.call("POST", GRAPH, GRAPH, &key, body, 400).await;
    let mut body = request.clone();
    body["query"]["relation_kinds"] = json!(["x".repeat(65536)]);
    http.call("POST", GRAPH, GRAPH, &key, body, 413).await;
    for route in [COMPANY, INSPECT, HISTORY] {
        let mut body = if route == COMPANY {
            json!({"contract_version":1,"workspace_id":"0".repeat(64),"query":{"root_record_ids":[id]}})
        } else {
            json!({"contract_version":1,"workspace_id":"0".repeat(64),"relation_id":id})
        };
        if route == HISTORY {
            body["revision"] = 1.into();
        }
        body["workspace_id"] = "x".repeat(65536).into();
        http.call("POST", route, route, &admin.secret, body, 413)
            .await;
    }
}
