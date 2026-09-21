//! Real HTTP conformance for history-bound relation delivery and consumer progress.
use super::administration::Client;
use super::*;
use uuid::Uuid;
const CHANGES: &str = "/api/v1/memory/relations/changes";
const ACTIVATE: &str = "/api/v1/memory/relations/changes/activate";
const CHECKPOINT: &str = "/api/v1/memory/relations/checkpoints";
const READ: &str = "/api/v1/memory/relations/checkpoints/read";
const REVISION: &str = "/api/v1/memory/relations/checkpoints/revision";
const DIAGNOSE: &str = "/api/v1/memory/relations/consumers/diagnose";
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
        Capability::MemoryCheckpoint,
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

fn scoped() -> Value {
    json!({"contract_version":1,"scope":memory_scope("private")})
}
fn query(after: Value, through: Value, limit: usize) -> Value {
    let mut body = scoped();
    body["query"] = json!({"after":after,"through":through,"limit":limit});
    body
}
fn read_body() -> Value {
    let mut body = scoped();
    body["consumer_id"] = "context-cache".into();
    body
}
fn checkpoint(cursor: Value, previous: &Value, key: &str) -> Value {
    json!({"scope":memory_scope("private"),"command":{"contract_version":1,"idempotency_key":key,"consumer_id":"context-cache","expected_revision":previous["revision"].as_u64().unwrap_or(0),"expected_checkpoint_digest":previous["checkpoint_digest"],"operation":{"type":"advance","cursor":cursor}}})
}
#[tokio::test]
async fn relation_changes_http_preserves_scope_progress_history_and_current_receipt_semantics() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("company", "owner").unwrap();
    let foreign = identity.bootstrap_tenant("other", "owner").unwrap();
    let alice = token(&identity, &admin.secret, "alice");
    let bob = token(&identity, &admin.secret, "bob");
    let other = token(&identity, &foreign.secret, "alice");
    let server = Server::start(router).await;
    let http = &server.http;
    let inactive = http
        .call(
            "POST",
            CHANGES,
            CHANGES,
            &alice,
            query(Value::Null, Value::Null, 1),
            200,
        )
        .await;
    assert_eq!(inactive["page"]["active"], false);
    let baseline = http
        .call("POST", ACTIVATE, ACTIVATE, &alice, scoped(), 200)
        .await["baseline"]
        .clone();
    let a = create(http, &alice, "a").await;
    let b = create(http, &alice, "b").await;
    let receipt = http
        .call(
            "POST",
            RELATIONS,
            RELATIONS,
            &alice,
            assertion(&a, &b, "link", "supports"),
            200,
        )
        .await["receipt"]
        .clone();
    let first = http
        .call(
            "POST",
            CHANGES,
            CHANGES,
            &alice,
            query(baseline.clone(), Value::Null, 1),
            200,
        )
        .await["page"]
        .clone();
    assert_eq!(
        first["changes"][0]["change"]["receipt_digest"],
        receipt["receipt_digest"]
    );
    let initial_command = checkpoint(baseline.clone(), &Value::Null, "initial");
    let initial = http
        .call(
            "POST",
            CHECKPOINT,
            CHECKPOINT,
            &alice,
            initial_command.clone(),
            200,
        )
        .await["receipt"]
        .clone();
    let advanced = http
        .call(
            "POST",
            CHECKPOINT,
            CHECKPOINT,
            &alice,
            checkpoint(
                first["next_cursor"].clone(),
                &initial["checkpoint"],
                "advance",
            ),
            200,
        )
        .await["receipt"]
        .clone();
    let rotated = token(&identity, &admin.secret, "alice");
    let old = http
        .call(
            "POST",
            CHECKPOINT,
            CHECKPOINT,
            &rotated,
            initial_command,
            200,
        )
        .await;
    assert_eq!(old["receipt"], initial);
    let current = http
        .call("POST", READ, READ, &rotated, read_body(), 200)
        .await;
    assert_eq!(current["checkpoint"], advanced["checkpoint"]);
    let mut history = read_body();
    history["revision"] = 1.into();
    assert_eq!(
        http.call("POST", REVISION, REVISION, &alice, history.clone(), 200)
            .await["receipt"],
        initial
    );
    history["revision"] = 99.into();
    http.call("POST", REVISION, REVISION, &alice, history.clone(), 404)
        .await;
    for isolated in [&bob, &other] {
        assert_eq!(
            http.call(
                "POST",
                CHANGES,
                CHANGES,
                isolated,
                query(Value::Null, Value::Null, 100),
                200
            )
            .await["page"]["active"],
            false
        );
        http.call(
            "POST",
            CHANGES,
            CHANGES,
            isolated,
            query(first["next_cursor"].clone(), Value::Null, 100),
            409,
        )
        .await;
        http.call("POST", READ, READ, isolated, read_body(), 404)
            .await;
        history["revision"] = 1.into();
        http.call("POST", REVISION, REVISION, isolated, history.clone(), 404)
            .await;
    }
    let mut rejected = scoped();
    rejected["idempotency_key"] = "reject".into();
    rejected["operation"] = json!({"type":"review","relation_id":receipt["relation_id"],"expected_revision":1,"disposition":"rejected","evidence_ref":"trace://reject"});
    http.call("POST", RELATIONS, RELATIONS, &alice, rejected, 200)
        .await;
    let latest = http
        .call(
            "POST",
            CHANGES,
            CHANGES,
            &alice,
            query(first["next_cursor"].clone(), Value::Null, 100),
            200,
        )
        .await["page"]
        .clone();
    assert_eq!(latest["changes"][0]["change"]["kind"], "reviewed");
    let mut diagnosis = read_body();
    diagnosis["witness"] = latest["high_watermark"].clone();
    let observed = http
        .call("POST", DIAGNOSE, DIAGNOSE, &alice, diagnosis, 200)
        .await;
    assert_eq!(observed["diagnostics"]["pending_positions"], 1);
    assert_eq!(
        observed["diagnostics"]["checkpoint_relative_to_witness"],
        "before"
    );
    let mut reconcile = checkpoint(baseline.clone(), &advanced["checkpoint"], "reconcile");
    reconcile["command"]["operation"]["type"] = "reconcile".into();
    reconcile["command"]["operation"]["evidence_ref"] = "trace://cache-rebuilt".into();
    let recovered = http
        .call("POST", CHECKPOINT, CHECKPOINT, &alice, reconcile, 200)
        .await;
    assert_eq!(recovered["receipt"]["previous"], advanced["checkpoint"]);
    assert_eq!(recovered["receipt"]["checkpoint"]["revision"], 3);
    assert_eq!(
        http.call("POST", ACTIVATE, ACTIVATE, &alice, scoped(), 200)
            .await["baseline"],
        baseline
    );
    let principal = identity.authenticate(&alice).unwrap();
    identity
        .revoke(&admin.secret, principal.id, principal.revision)
        .unwrap();
    for (path, body) in [
        (CHANGES, query(Value::Null, Value::Null, 1)),
        (ACTIVATE, scoped()),
        (READ, read_body()),
        (REVISION, history),
        (DIAGNOSE, read_body()),
        (CHECKPOINT, checkpoint(baseline, &Value::Null, "revoked")),
    ] {
        http.call("POST", path, path, &alice, body, 401).await;
    }
}
#[tokio::test]
async fn relation_changes_http_rejects_foreign_scopes_capabilities_and_malformed_contracts() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("company", "owner").unwrap();
    let alice = token(&identity, &admin.secret, "alice");
    let mut spec = spec();
    spec.subject_id = "alice".into();
    spec.capabilities = [Capability::MemoryRead].into();
    spec.grants.push(ResourceScope {
        visibility: Visibility::Private,
        ..spec.grants[0].clone()
    });
    let reader = identity.issue(&admin.secret, spec).unwrap().secret;
    let server = Server::start(router).await;
    let http = &server.http;
    let baseline = http
        .call("POST", ACTIVATE, ACTIVATE, &alice, scoped(), 200)
        .await["baseline"]
        .clone();
    let mut revision = read_body();
    revision["revision"] = 1.into();
    let bodies = [
        (CHANGES, query(Value::Null, Value::Null, 1)),
        (ACTIVATE, scoped()),
        (READ, read_body()),
        (REVISION, revision),
        (DIAGNOSE, read_body()),
        (
            CHECKPOINT,
            checkpoint(baseline.clone(), &Value::Null, "test"),
        ),
    ];
    for (path, body) in &bodies {
        http.call("POST", path, path, "", body.clone(), 401).await;
        if *path != CHANGES {
            http.call("POST", path, path, &reader, body.clone(), 403)
                .await;
        }
        for field in ["project_id", "mission_id", "agent_id"] {
            let mut denied = body.clone();
            denied["scope"][field] = "unauthorized".into();
            http.call("POST", path, path, &alice, denied, 403).await;
        }
        let mut unknown = body.clone();
        unknown["subject_id"] = "bob".into();
        http.call("POST", path, path, &alice, unknown, 400).await;
        let mut huge = body.clone();
        huge["padding"] = "x".repeat(70000).into();
        http.call("POST", path, path, &alice, huge, 413).await;
    }
    for limit in [0, 257] {
        http.call(
            "POST",
            CHANGES,
            CHANGES,
            &alice,
            query(Value::Null, Value::Null, limit),
            400,
        )
        .await;
    }
    for field in ["version", "stream", "prefix_digest"] {
        let mut bad = baseline.clone();
        bad[field] = if field == "version" {
            json!(2)
        } else {
            json!("bad")
        };
        http.call(
            "POST",
            CHANGES,
            CHANGES,
            &alice,
            query(bad, Value::Null, 1),
            400,
        )
        .await;
    }
    let mut unknown = baseline.clone();
    unknown["prefix_digest"] = "f".repeat(64).into();
    http.call(
        "POST",
        CHANGES,
        CHANGES,
        &alice,
        query(unknown.clone(), Value::Null, 1),
        409,
    )
    .await;
    let mut diagnose = read_body();
    diagnose["witness"] = unknown;
    assert_eq!(
        http.call("POST", DIAGNOSE, DIAGNOSE, &alice, diagnose, 200)
            .await["diagnostics"]["witness_status"],
        "history_incompatible"
    );
    let mut bad = checkpoint(baseline, &Value::Null, "bad");
    bad["command"]["operation"]["type"] = "reconcile".into();
    bad["command"]["operation"]["evidence_ref"] = "trace://invalid-initialization".into();
    http.call("POST", CHECKPOINT, CHECKPOINT, &alice, bad, 400)
        .await;
}
mod recovery;
