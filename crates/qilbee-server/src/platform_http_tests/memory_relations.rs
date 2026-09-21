//! Real HTTP assertions, lifecycle authority and response/OpenAPI conformance.
use super::administration::Client;
use super::*;
const COMMAND: &str = "/api/v1/memory/relations/commands";
const READ: &str = "/api/v1/memory/relations/read";
const INSPECT: &str = "/api/v1/memory/relations/inspect";
const HISTORY: &str = "/api/v1/memory/relations/revision";
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
fn key(identity: &IdentityStore, admin: &str, subject: &str, write: bool, review: bool) -> String {
    let mut definition = spec();
    definition.subject_id = subject.into();
    definition.capabilities = [Capability::MemoryRead].into();
    if write {
        definition.capabilities.insert(Capability::MemoryWrite);
    }
    if review {
        definition.capabilities.insert(Capability::MemoryReview);
    }
    definition.grants.push(ResourceScope {
        visibility: Visibility::Private,
        ..definition.grants[0].clone()
    });
    identity.issue(admin, definition).unwrap().secret
}
fn assertion(source: &Value, target: &Value, visibility: &str) -> Value {
    json!({"contract_version":1,"scope":memory_scope(visibility),"idempotency_key":"relation-create","operation":{"type":"assert","relation":{"source":{"record_id":source["record_id"],"revision":source["revision"]},"target":{"record_id":target["record_id"],"revision":target["revision"]},"kind":"causal_claim","provenance":{"origin":"model_inference","method":"fixture-extraction","method_revision":"prompt-v1","evidence_ref":"trace://fixture","model":{"provider":"fixture","model":"extractor","revision":"v1"}},"valid_from_millis":null,"valid_until_millis":null}}})
}
fn read(id: &Value, visibility: &str) -> Value {
    json!({"contract_version":1,"scope":memory_scope(visibility),"relation_id":id})
}
fn change(id: &Value, revision: u64, action: &str, visibility: &str) -> Value {
    json!({"contract_version":1,"scope":memory_scope(visibility),"idempotency_key":format!("{action}-{revision}"),"operation":{"type":action,"relation_id":id,"expected_revision":revision,"evidence_ref":"trace://decision"}})
}
#[tokio::test]
async fn typed_relations_real_http_enforce_scope_model_identity_review_and_lifecycle() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("company", "owner").unwrap();
    let foreign = identity.bootstrap_tenant("other", "owner").unwrap();
    let writer = key(&identity, &admin.secret, "alice", true, false);
    let reviewer = key(&identity, &admin.secret, "alice", false, true);
    let reader = key(&identity, &admin.secret, "alice", false, false);
    let bob = key(&identity, &admin.secret, "bob", true, true);
    let other = key(&identity, &foreign.secret, "alice", true, true);
    let server = Server::start(router).await;
    let http = &server.http;
    let a = http
        .call(
            "POST",
            MEMORY,
            MEMORY,
            &writer,
            memory_create("source", "Source", "private"),
            200,
        )
        .await["receipt"]
        .clone();
    let b = http
        .call(
            "POST",
            MEMORY,
            MEMORY,
            &writer,
            memory_create("target", "Target", "private"),
            200,
        )
        .await["receipt"]
        .clone();
    let command = assertion(&a, &b, "private");
    http.call("POST", COMMAND, COMMAND, "", command.clone(), 401)
        .await;
    http.call("POST", COMMAND, COMMAND, &reader, command.clone(), 403)
        .await;
    http.call("POST", COMMAND, COMMAND, &reviewer, command.clone(), 403)
        .await;
    for invalid in [
        "missing-model",
        "same-node",
        "unknown-field",
        "future-version",
        "invalid-time",
    ] {
        let mut body = command.clone();
        match invalid {
            "missing-model" => {
                body["operation"]["relation"]["provenance"]["model"] = Value::Null;
            }
            "same-node" => {
                body["operation"]["relation"]["target"] =
                    body["operation"]["relation"]["source"].clone();
            }
            "unknown-field" => {
                body["company_id"] = "other".into();
            }
            "future-version" => {
                body["contract_version"] = 2.into();
            }
            _ => {
                body["operation"]["relation"]["valid_from_millis"] = 10.into();
                body["operation"]["relation"]["valid_until_millis"] = 5.into();
            }
        };
        http.call("POST", COMMAND, COMMAND, &writer, body, 400)
            .await;
    }
    let created = http
        .call("POST", COMMAND, COMMAND, &writer, command.clone(), 200)
        .await;
    let id = &created["receipt"]["relation_id"];
    assert_eq!(
        http.call("POST", COMMAND, COMMAND, &writer, command.clone(), 200)
            .await,
        created
    );
    let current = http
        .call("POST", READ, READ, &reader, read(id, "private"), 200)
        .await;
    assert_eq!(current["relation"]["input"]["kind"], "causal_claim");
    assert_eq!(current["relation"]["reported_by"]["subject_id"], "alice");
    assert!(current["relation"]["review"].is_null());
    for token in [&bob, &other] {
        http.call("POST", READ, READ, token, read(id, "private"), 404)
            .await;
        http.call("POST", INSPECT, INSPECT, token, read(id, "private"), 404)
            .await;
        let mut history = read(id, "private");
        history["revision"] = 1.into();
        http.call("POST", HISTORY, HISTORY, token, history, 404)
            .await;
        http.call("POST", COMMAND, COMMAND, token, command.clone(), 409)
            .await;
    }
    http.call("POST", READ, READ, &writer, read(id, "shared"), 404)
        .await;
    for field in ["project_id", "agent_id", "mission_id"] {
        let mut body = read(id, "private");
        body["scope"][field] = "different".into();
        http.call("POST", READ, READ, &writer, body, 403).await;
    }
    http.call("POST", INSPECT, INSPECT, &writer, read(id, "private"), 403)
        .await;
    let mut conflict = command.clone();
    conflict["operation"]["relation"]["kind"] = "supports".into();
    http.call("POST", COMMAND, COMMAND, &writer, conflict, 409)
        .await;
    let mut review = change(id, 1, "review", "private");
    review["operation"]["disposition"] = "rejected".into();
    http.call("POST", COMMAND, COMMAND, &writer, review.clone(), 403)
        .await;
    let rejected = http
        .call("POST", COMMAND, COMMAND, &reviewer, review, 200)
        .await;
    assert_eq!(rejected["receipt"]["revision"], 2);
    http.call("POST", READ, READ, &reader, read(id, "private"), 404)
        .await;
    let inspected = http
        .call(
            "POST",
            INSPECT,
            INSPECT,
            &reviewer,
            read(id, "private"),
            200,
        )
        .await;
    assert_eq!(inspected["inspection"]["eligibility"]["reason"], "rejected");
    http.call(
        "POST",
        COMMAND,
        COMMAND,
        &writer,
        change(id, 1, "retire", "private"),
        409,
    )
    .await;
    http.call(
        "POST",
        COMMAND,
        COMMAND,
        &writer,
        change(id, 2, "retire", "private"),
        200,
    )
    .await;
    http.call(
        "POST",
        COMMAND,
        COMMAND,
        &writer,
        change(id, 3, "restore", "private"),
        200,
    )
    .await;
    assert_eq!(
        http.call(
            "POST",
            INSPECT,
            INSPECT,
            &reviewer,
            read(id, "private"),
            200
        )
        .await["inspection"]["eligibility"]["reason"],
        "rejected"
    );
    let mut approve = change(id, 4, "review", "private");
    approve["operation"]["disposition"] = "approved".into();
    http.call("POST", COMMAND, COMMAND, &reviewer, approve, 200)
        .await;
    http.call("POST", READ, READ, &reader, read(id, "private"), 200)
        .await;
    let mut history = read(id, "private");
    history["revision"] = 1.into();
    let historical = http
        .call("POST", HISTORY, HISTORY, &reviewer, history.clone(), 200)
        .await;
    assert_eq!(historical["history"]["receipt"], created["receipt"]);
    assert_eq!(historical["history"]["relation"], current["relation"]);
    history["revision"] = 999.into();
    http.call("POST", HISTORY, HISTORY, &reviewer, history.clone(), 404)
        .await;
    history["revision"] = 0.into();
    http.call("POST", HISTORY, HISTORY, &reviewer, history, 400)
        .await;
    let mut update = memory_create("correct", "Changed source", "private");
    update["operation"]["type"] = "update".into();
    update["operation"]["record_id"] = a["record_id"].clone();
    update["operation"]["expected_revision"] = 1.into();
    http.call("POST", MEMORY, MEMORY, &writer, update, 200)
        .await;
    http.call("POST", READ, READ, &reader, read(id, "private"), 404)
        .await;
    assert_eq!(
        http.call(
            "POST",
            INSPECT,
            INSPECT,
            &reviewer,
            read(id, "private"),
            200
        )
        .await["inspection"]["eligibility"]["reason"],
        "endpoint_revision_changed"
    );
    assert_eq!(
        http.call("POST", COMMAND, COMMAND, &writer, command, 200)
            .await,
        created
    );
    let credential = identity.authenticate(&writer).unwrap();
    identity
        .revoke(&admin.secret, credential.id, credential.revision)
        .unwrap();
    http.call("POST", READ, READ, &writer, read(id, "private"), 401)
        .await;
}

#[tokio::test]
async fn typed_relations_reject_oversized_bodies_and_isolate_authorized_partitions() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("company", "owner").unwrap();
    let mut definition = spec();
    definition.subject_id = "alice".into();
    definition.capabilities = [
        Capability::MemoryRead,
        Capability::MemoryWrite,
        Capability::MemoryReview,
    ]
    .into();
    for field in ["project_id", "agent_id", "mission_id"] {
        let mut grant = serde_json::to_value(&definition.grants[0]).unwrap();
        grant[field] = "other".into();
        definition
            .grants
            .push(serde_json::from_value(grant).unwrap());
    }
    let token = identity.issue(&admin.secret, definition).unwrap().secret;
    let server = Server::start(router).await;
    let http = &server.http;
    let a = http
        .call(
            "POST",
            MEMORY,
            MEMORY,
            &token,
            memory_create("a", "Identical content", "shared"),
            200,
        )
        .await["receipt"]
        .clone();
    let b = http
        .call(
            "POST",
            MEMORY,
            MEMORY,
            &token,
            memory_create("b", "Identical content", "shared"),
            200,
        )
        .await["receipt"]
        .clone();
    let command = assertion(&a, &b, "shared");
    let receipt = http
        .call("POST", COMMAND, COMMAND, &token, command.clone(), 200)
        .await["receipt"]
        .clone();
    // A different subject can read shared claims, but only inside its granted scope.
    let bob = key(&identity, &admin.secret, "bob", false, false);
    http.call(
        "POST",
        READ,
        READ,
        &bob,
        read(&receipt["relation_id"], "shared"),
        200,
    )
    .await;
    for field in ["project_id", "agent_id", "mission_id"] {
        let mut foreign_scope = memory_scope("shared");
        foreign_scope[field] = "other".into();
        let mut same_text = memory_create("a", "Identical content", "shared");
        same_text["scope"] = foreign_scope.clone();
        http.call("POST", MEMORY, MEMORY, &token, same_text, 200)
            .await;
        for route in [READ, INSPECT, HISTORY] {
            let mut body = read(&receipt["relation_id"], "shared");
            body["scope"] = foreign_scope.clone();
            if route == HISTORY {
                body["revision"] = 1.into();
            }
            http.call("POST", route, route, &token, body, 404).await;
        }
        let mut attempted_cross_scope = command.clone();
        attempted_cross_scope["scope"] = foreign_scope;
        http.call("POST", COMMAND, COMMAND, &token, attempted_cross_scope, 409)
            .await;
    }
    for route in [COMMAND, READ, INSPECT, HISTORY] {
        let body = if route == COMMAND {
            let mut body = command.clone();
            body["idempotency_key"] = "x".repeat(65_536).into();
            body
        } else {
            let mut body = read(&receipt["relation_id"], "shared");
            body["relation_id"] = "x".repeat(65_536).into();
            if route == HISTORY {
                body["revision"] = 1.into();
            }
            body
        };
        http.call("POST", route, route, &token, body, 413).await;
    }
    for (field, value) in [
        ("kind", json!("causal_fact")),
        ("confidence", json!(1.0)),
        (
            "source",
            json!({"record_id":a["record_id"],"revision":1,"company_id":"other"}),
        ),
    ] {
        let mut invalid = command.clone();
        invalid["operation"]["relation"][field] = value;
        http.call("POST", COMMAND, COMMAND, &token, invalid, 400)
            .await;
    }
}
#[test]
fn typed_relations_http_acknowledgements_survive_process_kill_with_history_and_invalidation() {
    use crate::http_server_tests::{crash_server_for, wire_request};
    let dir = TempDir::new().unwrap();
    let token = {
        let (_, identity) = app(dir.path());
        let admin = identity.bootstrap_tenant("company", "owner").unwrap();
        key(&identity, &admin.secret, "alice", true, true)
    };
    let fixture = "platform_http_tests::platform_http_memory_child";
    let (mut process, port) = crash_server_for(dir.path(), fixture);
    let (status, a) = wire_request(
        port,
        "POST",
        MEMORY,
        &token,
        memory_create("a", "Source", "private"),
    );
    assert_eq!(status, 200);
    let (status, b) = wire_request(
        port,
        "POST",
        MEMORY,
        &token,
        memory_create("b", "Target", "private"),
    );
    assert_eq!(status, 200);
    let command = assertion(&a["receipt"], &b["receipt"], "private");
    let (status, first) = wire_request(port, "POST", COMMAND, &token, command.clone());
    assert_eq!(status, 200);
    let id = &first["receipt"]["relation_id"];
    let (status, retired) = wire_request(
        port,
        "POST",
        COMMAND,
        &token,
        change(id, 1, "retire", "private"),
    );
    assert_eq!(status, 200);
    process.0.kill().unwrap();
    process.0.wait().unwrap();
    let (_restarted, port) = crash_server_for(dir.path(), fixture);
    assert_eq!(
        wire_request(port, "POST", COMMAND, &token, command).1,
        first
    );
    assert_eq!(
        wire_request(port, "POST", READ, &token, read(id, "private")).0,
        404
    );
    let mut history = read(id, "private");
    history["revision"] = 2.into();
    let (status, after) = wire_request(port, "POST", HISTORY, &token, history);
    assert_eq!(status, 200);
    assert_eq!(after["history"]["receipt"], retired["receipt"]);
    assert_eq!(
        wire_request(
            port,
            "POST",
            COMMAND,
            &token,
            change(id, 2, "restore", "private")
        )
        .0,
        200
    );
    assert_eq!(
        wire_request(port, "POST", READ, &token, read(id, "private")).0,
        200
    );
    let mut update = memory_create("update", "Corrected source", "private");
    update["operation"]["type"] = "update".into();
    update["operation"]["record_id"] = a["receipt"]["record_id"].clone();
    update["operation"]["expected_revision"] = 1.into();
    assert_eq!(wire_request(port, "POST", MEMORY, &token, update).0, 200);
    assert_eq!(
        wire_request(port, "POST", READ, &token, read(id, "private")).0,
        404
    );
}

#[path = "relation_evidence.rs"]
mod evidence;
