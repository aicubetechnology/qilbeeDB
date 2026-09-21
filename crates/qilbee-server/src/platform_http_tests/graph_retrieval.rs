//! Real HTTP schemas and authorization for experimental typed-path retrieval.
use super::administration::Client;
use super::*;
const SEARCH: &str = "/api/v1/memory/search/graph";
const CATALOG: &str = "/api/v1/memory/graph-ranking-profiles";
const COMMANDS: &str = "/api/v1/memory/commands";
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
fn query() -> Value {
    json!({"contract_version":1,"scope":memory_scope("private"),"query":{"ranking_version":"typed_path_balanced_v1","seed":{"mode":"lexical","text":"anchor"},"limit":10}})
}
async fn create(http: &Client, key: &str, id: &str, text: &str) -> Value {
    http.call(
        "POST",
        COMMANDS,
        COMMANDS,
        key,
        memory_create(id, text, "private"),
        200,
    )
    .await["receipt"]
        .clone()
}
#[tokio::test]
async fn graph_search_all_profiles_and_seed_modes_validate_against_served_openapi() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("company", "owner").unwrap();
    let key = memory_key(&identity, &admin.secret, "writer", true);
    let server = Server::start(router).await;
    let http = &server.http;
    let a = create(http, &key, "a", "anchor").await;
    let b = create(http, &key, "b", "neighbor").await;
    let relation = "/api/v1/memory/relations/commands";
    let assertion = json!({"contract_version":1,"scope":memory_scope("private"),"idempotency_key":"ab","operation":{"type":"assert","relation":{"source":{"record_id":a["record_id"],"revision":1},"target":{"record_id":b["record_id"],"revision":1},"kind":"same_entity","provenance":{"origin":"tool_observation","method":"fixture","method_revision":"v1","evidence_ref":"trace://graph-search"}}}});
    let edge = http
        .call("POST", relation, relation, &key, assertion, 200)
        .await;
    let space = json!({"provider":"fixture","model":"vectors","revision":"v1","dimensions":3});
    let embedding = "/api/v1/memory/embeddings";
    http.call("POST",embedding,embedding,&key,json!({"contract_version":1,"scope":memory_scope("private"),"idempotency_key":"vector","record_id":a["record_id"],"record_revision":1,"space":space,"vector":[1,0,0]}),200).await;
    let catalog = http
        .call("GET", CATALOG, CATALOG, &key, Value::Null, 200)
        .await;
    assert_eq!(catalog["graph_profiles"].as_array().unwrap().len(), 4);
    for profile in catalog["graph_profiles"].as_array().unwrap() {
        for mode in ["lexical", "semantic", "hybrid"] {
            let mut body = query();
            body["query"]["ranking_version"] = profile["version"].clone();
            body["query"]["seed"] = match mode {
                "semantic" => json!({"mode":mode,"space":space,"vector":[1,0,0]}),
                "hybrid" => {
                    json!({"mode":mode,"text":"anchor","space":space,"vector":[1,0,0],"ranking_version":"weighted_rrf_v2"})
                }
                _ => json!({"mode":mode,"text":"anchor"}),
            };
            let value = http.call("POST", SEARCH, SEARCH, &key, body, 200).await;
            let page = &value["page"];
            assert_eq!(page["ranking"], *profile);
            assert_eq!(page["hits"].as_array().unwrap().len(), 2);
            assert_eq!(page["hits"][0]["score"], json!(1.0 / 3.0));
            assert_eq!(
                page["hits"][1]["graph"]["steps"][0]["relation_id"],
                edge["receipt"]["relation_id"]
            );
            assert_eq!(page["seed"]["mode"], mode);
            assert_eq!(page["coverage"]["embeddings_complete"], mode == "lexical");
        }
    }
    // Old cosine continues to expose cosine, not the graph's maximum 1/3 score.
    let old = "/api/v1/memory/search";
    let old_page=http.call("POST",old,old,&key,json!({"contract_version":1,"scope":memory_scope("private"),"query":{"space":space,"vector":[1,0,0],"limit":10}}),200).await;
    assert_eq!(old_page["page"]["hits"][0]["score"], 1.0);
}

#[tokio::test]
async fn graph_search_cannot_mix_tenants_private_subjects_or_unauthorized_scopes() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("company", "owner").unwrap();
    let other = identity.bootstrap_tenant("other-company", "owner").unwrap();
    let alice = memory_key(&identity, &admin.secret, "alice", true);
    let bob = memory_key(&identity, &admin.secret, "bob", true);
    let foreign = memory_key(&identity, &other.secret, "alice", true);
    let server = Server::start(router).await;
    let http = &server.http;
    for key in [&alice, &bob, &foreign] {
        let own = create(http, key, "one", "anchor").await;
        let page = http.call("POST", SEARCH, SEARCH, key, query(), 200).await;
        assert_eq!(page["page"]["hits"].as_array().unwrap().len(), 1);
        assert_eq!(
            page["page"]["hits"][0]["record"]["record_id"],
            own["record_id"]
        );
    }
    for field in ["project_id", "agent_id", "mission_id"] {
        let mut body = query();
        body["scope"][field] = "outside".into();
        http.call("POST", SEARCH, SEARCH, &alice, body, 403).await;
    }
    http.call("GET", CATALOG, CATALOG, &admin.secret, Value::Null, 403)
        .await;
    http.call("POST", SEARCH, SEARCH, &admin.secret, query(), 403)
        .await;
    http.call("POST", SEARCH, SEARCH, "", query(), 401).await;
    let credential = identity.authenticate(&alice).unwrap();
    identity
        .revoke(&admin.secret, credential.id, credential.revision)
        .unwrap();
    http.call("POST", SEARCH, SEARCH, &alice, query(), 401)
        .await;
    http.call("GET", CATALOG, CATALOG, &alice, Value::Null, 401)
        .await;
}

#[tokio::test]
async fn graph_search_rejects_unversioned_weights_unknown_fields_and_invalid_budgets() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("company", "owner").unwrap();
    let key = memory_key(&identity, &admin.secret, "writer", true);
    let server = Server::start(router).await;
    let http = &server.http;
    for (field, value) in [
        ("ranking_version", json!("latest")),
        ("weights", json!([0.5, 0.5])),
        ("limit", json!(0)),
        ("scan_limit", json!(10001)),
        ("scan_bytes_limit", json!(0)),
        ("expansion", json!({"max_depth":100})),
    ] {
        let mut body = query();
        body["query"][field] = value;
        http.call("POST", SEARCH, SEARCH, &key, body, 400).await;
    }
    let mut body = query();
    body["query"]["seed"]["vector"] = json!([1, 0, 0]);
    http.call("POST", SEARCH, SEARCH, &key, body, 400).await;
    let huge = " ".repeat(2 * 1024 * 1024 + 1);
    let response = http
        .client
        .post(format!("{}{SEARCH}", http.base))
        .bearer_auth(&key)
        .header("content-type", "application/json")
        .body(huge)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 413);
    assert_eq!(response.headers()["cache-control"], "no-store");
}
