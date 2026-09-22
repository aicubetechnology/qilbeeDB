//! Real TCP checks for external consolidation, scope and the served OpenAPI contract.
use super::administration::Client;
use super::*;
const COMMAND: &str = "/api/v1/memory/consolidation/commands";
const INSPECT: &str = "/api/v1/memory/consolidation/inspect";
const HISTORY: &str = "/api/v1/memory/consolidation/revision";
const QUERY: &str = "/api/v1/memory/consolidation/query";
const CONTEXT: &str = "/api/v1/memory/consolidation/context";
const MEMORY: &str = "/api/v1/memory/commands";
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
fn request(operation: Value, key: &str) -> Value {
    json!({"contract_version":1,"scope":memory_scope("private"),"idempotency_key":key,"operation":operation})
}
fn read(id: &Value) -> Value {
    json!({"contract_version":1,"scope":memory_scope("private"),"job_id":id})
}
fn reference(value: &Value) -> Value {
    json!({"record_id":value["record_id"],"revision":value["revision"]})
}
fn creation(sources: &[Value]) -> Value {
    request(
        json!({"type":"create","spec":{"sources":sources.iter().map(reference).collect::<Vec<_>>(),"objective":"Extract useful entity links","policy_ref":"policy://fixture/v1",
        "extractor":{"origin":"model_inference","method":"fixture","method_revision":"v1","evidence_ref":"trace://fixture","model":{"provider":"fixture","model":"extractor","revision":"v1"}},
        "max_relations":16,"max_attempts":3,"lease_millis":30000,"max_attempt_millis":60000}}),
        "create",
    )
}
async fn sources(http: &Client, token: &str) -> Vec<Value> {
    let mut result = Vec::new();
    for (key, text) in [("a", "anchor"), ("b", "neighbor"), ("c", "context")] {
        result.push(
            http.call(
                "POST",
                MEMORY,
                MEMORY,
                token,
                memory_create(key, text, "private"),
                200,
            )
            .await["receipt"]
                .clone(),
        );
    }
    result
}
fn output(a: &Value, b: &Value) -> Value {
    json!({"source":reference(a),"target":reference(b),"kind":"same_entity","evidence_ref":"trace://output"})
}
async fn setup(http: &Client, token: &str) -> (Vec<Value>, Value, Value) {
    let s = sources(http, token).await;
    let r = http
        .call("POST", COMMAND, COMMAND, token, creation(&s), 200)
        .await;
    let id = r["receipt"]["job_id"].clone();
    http.call(
        "POST",
        COMMAND,
        COMMAND,
        token,
        request(
            json!({"type":"claim","job_id":id,"expected_revision":1,"worker_id":"external-worker"}),
            "claim",
        ),
        200,
    )
    .await;
    let inspection = http
        .call("POST", INSPECT, INSPECT, token, read(&id), 200)
        .await["inspection"]
        .clone();
    (s, id, inspection)
}

#[tokio::test]
async fn consolidation_real_http_publishes_atomic_relations_and_preserves_exact_receipts() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("company", "owner").unwrap();
    let token = memory_key(&identity, &admin.secret, "alice", true);
    let server = Server::start(router).await;
    let http = &server.http;
    let (s, id, inspection) = setup(http, &token).await;
    assert_eq!(inspection["lease_active"], true);
    let fence = inspection["job"]["attempts"][0]["fence"].clone();
    let mut context = read(&id);
    context["expected_revision"] = 2.into();
    context["fence"] = fence.clone();
    let v = http
        .call("POST", CONTEXT, CONTEXT, &token, context, 200)
        .await;
    assert_eq!(v["context"]["records"].as_array().unwrap().len(), 3);
    for (i, record) in v["context"]["records"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        assert_eq!(record["record_id"], s[i]["record_id"]);
        assert_eq!(record["revision"], 1);
    }
    let publication = request(
        json!({"type":"publish","job_id":id,"expected_revision":2,"fence":fence,"assertions":[output(&s[0],&s[1]),output(&s[0],&s[2])],"usage":{"status":"reported","model_calls":1,"input_tokens":100,"output_tokens":20,"cost_microusd":null},"evidence_ref":"trace://provider-report"}),
        "publish",
    );
    let schema = json!({"allOf":[http.api["paths"][COMMAND]["post"]["requestBody"]["content"]["application/json"]["schema"]],"components":http.api["components"]});
    jsonschema::draft202012::options()
        .build(&schema)
        .unwrap()
        .validate(&publication)
        .unwrap();
    let receipt = http
        .call("POST", COMMAND, COMMAND, &token, publication.clone(), 200)
        .await;
    assert_eq!(receipt["receipt"]["revision"], 3);
    assert_eq!(
        http.call("POST", COMMAND, COMMAND, &token, publication, 200)
            .await,
        receipt
    );
    let current = http
        .call("POST", INSPECT, INSPECT, &token, read(&id), 200)
        .await;
    assert_eq!(current["inspection"]["job"]["status"], "published");
    assert_eq!(
        current["inspection"]["job"]["output_receipts"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let graph_route = "/api/v1/memory/graph/typed";
    let graph_request = json!({"contract_version":1,"scope":memory_scope("private"),"query":{"root_record_ids":[s[0]["record_id"]]}});
    let graph = http
        .call(
            "POST",
            graph_route,
            graph_route,
            &token,
            graph_request.clone(),
            200,
        )
        .await;
    assert_eq!(graph["graph"]["edges"].as_array().unwrap().len(), 2);
    assert_eq!(graph["graph"]["coverage"]["complete"], true);
    let mut revision = read(&id);
    revision["revision"] = 3.into();
    assert_eq!(
        http.call("POST", HISTORY, HISTORY, &token, revision, 200)
            .await["history"]["receipt"],
        receipt["receipt"]
    );
    let page=http.call("POST",QUERY,QUERY,&token,json!({"contract_version":1,"scope":memory_scope("private"),"query":{"limit":10,"scan_limit":10,"status":"published"}}),200).await;
    assert_eq!(page["page"]["jobs"].as_array().unwrap().len(), 1);
    assert!(page["page"]["next_after"].is_null());
    let mut correction = memory_create("correct", "changed context", "private");
    correction["operation"]["type"] = "update".into();
    correction["operation"]["record_id"] = s[2]["record_id"].clone();
    correction["operation"]["expected_revision"] = 1.into();
    http.call("POST", MEMORY, MEMORY, &token, correction, 200)
        .await;
    let graph = http
        .call("POST", graph_route, graph_route, &token, graph_request, 200)
        .await;
    assert!(graph["graph"]["edges"].as_array().unwrap().is_empty());
    let current = http
        .call("POST", INSPECT, INSPECT, &token, read(&id), 200)
        .await;
    assert_eq!(current["inspection"]["job"]["status"], "published");
    assert!(!current["inspection"]["source_failure"].is_null());
}

#[tokio::test]
async fn consolidation_real_http_enforces_owner_credential_scope_and_revocation() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("company", "owner").unwrap();
    let other_admin = identity.bootstrap_tenant("other", "owner").unwrap();
    let alice = memory_key(&identity, &admin.secret, "alice", true);
    let bob = memory_key(&identity, &admin.secret, "bob", true);
    let other = memory_key(&identity, &other_admin.secret, "alice", true);
    let second = memory_key(&identity, &admin.secret, "alice", true);
    let reader = memory_key(&identity, &admin.secret, "alice", false);
    let server = Server::start(router).await;
    let http = &server.http;
    let (s, id, inspection) = setup(http, &alice).await;
    let fence = &inspection["job"]["attempts"][0]["fence"];
    for key in [&bob, &other] {
        http.call("POST", INSPECT, INSPECT, key, read(&id), 404)
            .await;
        let mut history = read(&id);
        history["revision"] = 1.into();
        http.call("POST", HISTORY, HISTORY, key, history, 404).await;
        let v=http.call("POST",QUERY,QUERY,key,json!({"contract_version":1,"scope":memory_scope("private"),"query":{"limit":10,"scan_limit":100}}),200).await;
        assert!(v["page"]["jobs"].as_array().unwrap().is_empty());
        http.call("POST", COMMAND, COMMAND, key, creation(&s), 409)
            .await;
    }
    http.call("POST", COMMAND, COMMAND, &reader, creation(&s), 403)
        .await;
    let mut context = read(&id);
    context["expected_revision"] = 2.into();
    context["fence"] = fence.clone();
    http.call("POST", CONTEXT, CONTEXT, &second, context.clone(), 409)
        .await;
    http.call("POST", CONTEXT, CONTEXT, &reader, context.clone(), 403)
        .await;
    for field in ["project_id", "agent_id", "mission_id"] {
        let mut forged = context.clone();
        forged["scope"][field] = "unauthorized".into();
        http.call("POST", CONTEXT, CONTEXT, &alice, forged, 403)
            .await;
    }
    for field in ["company_id", "owner", "subject_id"] {
        let mut forged = read(&id);
        forged[field] = "other".into();
        http.call("POST", INSPECT, INSPECT, &alice, forged, 400)
            .await;
    }
    let cred = identity.authenticate(&alice).unwrap();
    identity
        .revoke(&admin.secret, cred.id, cred.revision)
        .unwrap();
    http.call("POST", CONTEXT, CONTEXT, &alice, context, 401)
        .await;
}

#[tokio::test]
async fn consolidation_real_http_cancellation_failures_and_usage_keep_history() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("company", "owner").unwrap();
    let key = memory_key(&identity, &admin.secret, "alice", true);
    let server = Server::start(router).await;
    let http = &server.http;
    let (_, id, inspection) = setup(http, &key).await;
    let fence = inspection["job"]["attempts"][0]["fence"].clone();
    let fail = request(
        json!({"type":"fail","job_id":id,"expected_revision":2,"fence":fence,"usage":{"status":"unknown"},"evidence_ref":"trace://partial-execution"}),
        "failed",
    );
    http.call("POST", COMMAND, COMMAND, &key, fail, 200).await;
    http.call(
        "POST",
        COMMAND,
        COMMAND,
        &key,
        request(
            json!({"type":"claim","job_id":id,"expected_revision":3,"worker_id":"retry-worker"}),
            "reclaim",
        ),
        200,
    )
    .await;
    http.call("POST",COMMAND,COMMAND,&key,request(json!({"type":"cancel","job_id":id,"expected_revision":4,"evidence_ref":"policy://cancel"}),"cancel"),200).await;
    let reconcile = request(
        json!({"type":"reconcile_usage","job_id":id,"expected_revision":5,"attempt_number":1,"usage":{"status":"reported","model_calls":1,"input_tokens":500,"output_tokens":0,"cost_microusd":7},"evidence_ref":"provider://receipt"}),
        "reconcile",
    );
    http.call("POST", COMMAND, COMMAND, &key, reconcile, 200)
        .await;
    let v = http
        .call("POST", INSPECT, INSPECT, &key, read(&id), 200)
        .await;
    assert_eq!(v["inspection"]["job"]["status"], "cancelled");
    assert_eq!(
        v["inspection"]["job"]["attempts"][0]["usage"]["status"],
        "reported"
    );
    assert_eq!(
        v["inspection"]["job"]["attempts"][1]["usage"]["status"],
        "unknown"
    );
    let mut historical = read(&id);
    historical["revision"] = 3.into();
    let old = http
        .call("POST", HISTORY, HISTORY, &key, historical, 200)
        .await;
    assert_eq!(
        old["history"]["job"]["attempts"][0]["usage"]["status"],
        "unknown"
    );
    let mut missing = read(&id);
    missing["revision"] = 100.into();
    http.call("POST", HISTORY, HISTORY, &key, missing, 404)
        .await;
    for field in ["limit", "scan_limit"] {
        let mut invalid = json!({"contract_version":1,"scope":memory_scope("private"),"query":{"limit":10,"scan_limit":100}});
        invalid["query"][field] = 0.into();
        http.call("POST", QUERY, QUERY, &key, invalid, 400).await;
    }
}

#[tokio::test]
async fn consolidation_company_administration_retains_owner_and_fences_actual_worker() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity
        .bootstrap_tenant("company", "company-admin")
        .unwrap();
    let other = identity
        .bootstrap_tenant("other-company", "other-admin")
        .unwrap();
    let token = memory_key(&identity, &admin.secret, "alice", true);
    let server = Server::start(router).await;
    let http = &server.http;
    let (_, id, _) = setup(http, &token).await;
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
    let workspace = workspaces["page"]["workspaces"][0]["workspace_id"].clone();
    let route = "/api/v1/company/memory/consolidation/query";
    let query =
        json!({"contract_version":1,"workspace_id":workspace,"query":{"limit":10,"scan_limit":10}});
    let page = http
        .call("POST", route, route, &admin.secret, query.clone(), 200)
        .await;
    assert_eq!(page["page"]["jobs"][0]["owner_id"], "alice");
    assert_eq!(page["page"]["jobs"][0]["summary"]["job_id"], id);
    http.call("POST", route, route, &token, query.clone(), 403)
        .await;
    http.call("POST", route, route, &other.secret, query, 404)
        .await;
    let base =
        json!({"contract_version":1,"workspace_id":workspace,"owner_id":"alice","job_id":id});
    let inspect = "/api/v1/company/memory/consolidation/inspect";
    http.call("POST", inspect, inspect, &admin.secret, base.clone(), 200)
        .await;
    let cancel = "/api/v1/company/memory/consolidation/cancel";
    let mut body = base.clone();
    body["expected_revision"] = 2.into();
    body["idempotency_key"] = "admin-cancel".into();
    body["evidence_ref"] = "incident://stop".into();
    http.call("POST", cancel, cancel, &token, body.clone(), 403)
        .await;
    http.call("POST", cancel, cancel, &other.secret, body.clone(), 404)
        .await;
    let receipt = http
        .call("POST", cancel, cancel, &admin.secret, body.clone(), 200)
        .await;
    assert_eq!(receipt["receipt"]["author"]["subject_id"], "company-admin");
    assert_eq!(
        http.call("POST", cancel, cancel, &admin.secret, body, 200)
            .await,
        receipt
    );
    let current = http
        .call("POST", INSPECT, INSPECT, &token, read(&id), 200)
        .await;
    assert_eq!(current["inspection"]["job"]["status"], "cancelled");
    assert_eq!(
        current["inspection"]["job"]["created_by"]["subject_id"],
        "alice"
    );
    assert_eq!(
        current["inspection"]["job"]["attempts"][0]["usage"]["status"],
        "unknown"
    );
    let history = "/api/v1/company/memory/consolidation/revision";
    let mut body = base;
    body["revision"] = 3.into();
    let rev = http
        .call("POST", history, history, &admin.secret, body.clone(), 200)
        .await;
    assert_eq!(rev["history"]["receipt"], receipt["receipt"]);
    body["revision"] = 99.into();
    http.call("POST", history, history, &admin.secret, body.clone(), 404)
        .await;
    body["revision"] = 3.into();
    body["owner_id"] = "wrong-owner".into();
    http.call("POST", history, history, &admin.secret, body, 404)
        .await;
    identity
        .revoke(
            &admin.secret,
            admin.credential.id,
            admin.credential.revision,
        )
        .unwrap();
    http.call(
        "POST",
        inspect,
        inspect,
        &admin.secret,
        json!({"contract_version":1,"workspace_id":workspace,"owner_id":"alice","job_id":id}),
        401,
    )
    .await;
}
