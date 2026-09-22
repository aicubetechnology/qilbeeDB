mod agent_profiles;
mod company_memory;
mod company_consumers;
mod company_learning;
mod learning_evidence;
mod memory_graph;
mod memory_relations;
mod typed_graph;
use crate::{
    http_server::create_router,
    security::identity::{Capability, CredentialSpec, IdentityStore, ResourceScope, Visibility},
};
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use qilbee_graph::Database;
use serde_json::{Value, json};
use std::sync::Arc;
use tempfile::TempDir;
use tower::Service;

fn app(path: &std::path::Path) -> (Router, Arc<IdentityStore>) {
    let database = Arc::new(Database::open_for_testing(path).unwrap());
    let identity = Arc::new(IdentityStore::new(Arc::new(database.storage().clone())));
    (create_router(database).unwrap(), identity)
}
async fn request(
    app: &Router,
    method: &str,
    path: &str,
    token: &str,
    body: Value,
) -> (StatusCode, Value) {
    let response = app
        .clone()
        .call(
            Request::builder()
                .method(method)
                .uri(path)
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
fn spec() -> CredentialSpec {
    CredentialSpec {
        scope_policy: None,
        subject_id: "agent-user".into(),
        capabilities: [Capability::MemoryRead, Capability::ProcedurePropose].into(),
        grants: vec![ResourceScope {
            project_id: "project".into(),
            mission_id: None,
            agent_id: "agent".into(),
            visibility: Visibility::Shared,
        }],
        expires_at_millis: None,
    }
}

#[tokio::test]
async fn platform_http_default_requires_explicit_credentials_and_hides_legacy_routes() {
    let dir = TempDir::new().unwrap();
    let (router, _) = app(dir.path());
    let (status, body) = request(&router, "GET", "/api/v1/identity", "", Value::Null).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["contract_version"], 1);
    for (method, path, body) in [
        (
            "POST",
            "/api/v1/auth/login",
            json!({"username":"admin","password":"SecureAdmin@123!"}),
        ),
        ("POST", "/graphs/test", Value::Null),
        ("GET", "/memory/agent/episodes/recent", Value::Null),
    ] {
        assert_eq!(
            request(&router, method, path, "", body).await.0,
            StatusCode::NOT_FOUND
        );
    }
    let (status, body) = request(&router, "GET", "/health", "", Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["contract_version"], 1);
}

#[tokio::test]
async fn platform_http_issues_credentials_without_trusting_tenant_payloads() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("tenant-a", "operator").unwrap();
    let payload = json!({"contract_version":1,"spec":spec()});
    let (status, issued) = request(
        &router,
        "POST",
        "/api/v1/credentials",
        &admin.secret,
        payload.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{issued}");
    let token = issued["secret"].as_str().unwrap();
    let (status, who) = request(&router, "GET", "/api/v1/identity", token, Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(who["credential"]["tenant_id"], "tenant-a");
    assert!(who.get("secret").is_none());
    assert!(!who.to_string().contains("verifier"));
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/credentials",
            token,
            payload.clone()
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let mut forged = payload.clone();
    forged["spec"]["tenant_id"] = "tenant-b".into();
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/credentials",
            &admin.secret,
            forged
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let mut unsupported = payload;
    unsupported["contract_version"] = 99.into();
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/credentials",
            &admin.secret,
            unsupported
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn platform_http_rotation_revocation_and_tenant_checks_survive_restart() {
    let dir = TempDir::new().unwrap();
    let (admin, id, old_token, rotated_token) = {
        let (router, identity) = app(dir.path());
        let admin = identity.bootstrap_tenant("tenant-a", "operator").unwrap();
        let other = identity.bootstrap_tenant("tenant-b", "operator").unwrap();
        let key = identity.issue(&admin.secret, spec()).unwrap();
        let path = format!("/api/v1/credentials/{}", key.credential.id);
        assert_eq!(
            request(&router, "GET", &path, &other.secret, Value::Null)
                .await
                .0,
            StatusCode::FORBIDDEN
        );
        let (status, rotated) = request(
            &router,
            "POST",
            &format!("{path}/rotate"),
            &admin.secret,
            json!({"contract_version":1,"expected_revision":1}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{rotated}");
        assert_eq!(
            request(&router, "GET", "/api/v1/identity", &key.secret, Value::Null)
                .await
                .0,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            request(
                &router,
                "POST",
                &format!("{path}/revoke"),
                &admin.secret,
                json!({"contract_version":1,"expected_revision":1})
            )
            .await
            .0,
            StatusCode::CONFLICT
        );
        (
            admin,
            key.credential.id,
            key.secret,
            rotated["secret"].as_str().unwrap().to_owned(),
        )
    };
    {
        let (router, _) = app(dir.path());
        assert_eq!(
            request(
                &router,
                "GET",
                "/api/v1/identity",
                &rotated_token,
                Value::Null
            )
            .await
            .0,
            StatusCode::OK
        );
        assert_eq!(
            request(&router, "GET", "/api/v1/identity", &old_token, Value::Null)
                .await
                .0,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            request(
                &router,
                "POST",
                &format!("/api/v1/credentials/{id}/revoke"),
                &admin.secret,
                json!({"contract_version":1,"expected_revision":2})
            )
            .await
            .0,
            StatusCode::OK
        );
    }
    let (router, _) = app(dir.path());
    assert_eq!(
        request(
            &router,
            "GET",
            "/api/v1/identity",
            &rotated_token,
            Value::Null
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn platform_http_transport_errors_are_versioned_and_secrets_are_not_cacheable() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("tenant-a", "operator").unwrap();
    let (status, body) = request(
        &router,
        "DELETE",
        "/api/v1/identity",
        &admin.secret,
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(body["contract_version"], 1);
    let oversized = json!({"contract_version":1,"spec":{"subject_id":"x".repeat(70_000)}});
    let (status, body) = request(
        &router,
        "POST",
        "/api/v1/credentials",
        &admin.secret,
        oversized,
    )
    .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(body["contract_version"], 1);
    let response = router
        .clone()
        .call(
            Request::builder()
                .uri("/api/v1/identity")
                .header("authorization", format!("Bearer {}", admin.secret))
                .header("authorization", format!("Bearer {}", admin.secret))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let response = router
        .clone()
        .call(
            Request::builder()
                .method("POST")
                .uri("/api/v1/credentials")
                .header("authorization", format!("Bearer {}", admin.secret))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"contract_version":1,"spec":spec()}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(response.headers()["cache-control"], "no-store");
}

fn memory_scope(visibility: &str) -> Value {
    json!({"project_id":"project","mission_id":null,"agent_id":"agent","visibility":visibility})
}
fn memory_create(key: &str, text: &str, visibility: &str) -> Value {
    json!({"contract_version":1,"idempotency_key":key,"scope":memory_scope(visibility),"operation":{"type":"create","record":{"episode_type":"Observation","event_time_millis":1_700_000_000_000i64,"content":{"primary":text,"data":{"sequence":7}},"tags":["integration"],"metadata":{"source_request_id":"test-source"}}}})
}
fn memory_key(identity: &IdentityStore, admin: &str, subject: &str, write: bool) -> String {
    let mut definition = spec();
    definition.subject_id = subject.into();
    definition.capabilities = [Capability::MemoryRead].into();
    if write {
        definition.capabilities.insert(Capability::MemoryWrite);
    }
    definition.grants.push(ResourceScope {
        visibility: Visibility::Private,
        ..definition.grants[0].clone()
    });
    identity.issue(admin, definition).unwrap().secret
}
fn record_url(id: &str, visibility: &str) -> String {
    format!(
        "/api/v1/memory/records/{id}?contract_version=1&project_id=project&agent_id=agent&visibility={visibility}"
    )
}
#[tokio::test]
async fn platform_http_memory_commands_preserve_receipts_revisions_and_deletions() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("tenant-a", "operator").unwrap();
    let token = memory_key(&identity, &admin.secret, "writer", true);
    let create = memory_create("first", "before", "shared");
    let (status, original) = request(
        &router,
        "POST",
        "/api/v1/memory/commands",
        &token,
        create.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{original}");
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/memory/commands",
            &token,
            create.clone()
        )
        .await
        .1,
        original
    );
    let id = original["receipt"]["record_id"].as_str().unwrap();
    let (status, found) = request(
        &router,
        "GET",
        &record_url(id, "shared"),
        &token,
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{found}");
    assert_eq!(found["record"]["payload"]["content"]["data"]["sequence"], 7);
    assert_eq!(
        found["record"]["payload"]["metadata"]["source_request_id"],
        "test-source"
    );
    let mut changed = create.clone();
    changed["operation"]["record"]["content"]["primary"] = "different".into();
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/memory/commands",
            &token,
            changed.clone()
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    changed["idempotency_key"] = "update".into();
    changed["operation"]["type"] = "update".into();
    changed["operation"]["record_id"] = id.into();
    changed["operation"]["expected_revision"] = 1.into();
    let (status, updated) = request(
        &router,
        "POST",
        "/api/v1/memory/commands",
        &token,
        changed.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["receipt"]["revision"], 2);
    changed["idempotency_key"] = "stale".into();
    assert_eq!(
        request(&router, "POST", "/api/v1/memory/commands", &token, changed)
            .await
            .0,
        StatusCode::CONFLICT
    );
    let delete = json!({"contract_version":1,"idempotency_key":"delete","scope":memory_scope("shared"),"operation":{"type":"delete","record_id":id,"expected_revision":2}});
    assert_eq!(
        request(&router, "POST", "/api/v1/memory/commands", &token, delete)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        request(&router, "POST", "/api/v1/memory/commands", &token, create)
            .await
            .1,
        original
    );
    assert_eq!(
        request(
            &router,
            "GET",
            &record_url(id, "shared"),
            &token,
            Value::Null
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
}
#[tokio::test]
async fn platform_http_memory_scope_and_permissions_apply_to_writes_reads_and_queries() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin_a = identity.bootstrap_tenant("tenant-a", "operator").unwrap();
    let admin_b = identity.bootstrap_tenant("tenant-b", "operator").unwrap();
    let alice = memory_key(&identity, &admin_a.secret, "alice", true);
    let bob = memory_key(&identity, &admin_a.secret, "bob", false);
    let foreign = memory_key(&identity, &admin_b.secret, "alice", true);
    let (_, created) = request(
        &router,
        "POST",
        "/api/v1/memory/commands",
        &alice,
        memory_create("shared", "private company data", "shared"),
    )
    .await;
    let id = created["receipt"]["record_id"]
        .as_str()
        .expect("record receipt");
    assert_eq!(
        request(&router, "GET", &record_url(id, "shared"), &bob, Value::Null)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        request(
            &router,
            "GET",
            &record_url(id, "shared"),
            &foreign,
            Value::Null
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/memory/commands",
            &bob,
            memory_create("attempt", "denied", "shared")
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let (_, private) = request(
        &router,
        "POST",
        "/api/v1/memory/commands",
        &alice,
        memory_create("private", "personal", "private"),
    )
    .await;
    let private_id = private["receipt"]["record_id"].as_str().unwrap();
    assert_eq!(
        request(
            &router,
            "GET",
            &record_url(private_id, "private"),
            &bob,
            Value::Null
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let query = json!({"contract_version":1,"scope":memory_scope("shared"),"filter":{"limit":10,"text_contains":"company"}});
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/memory/query",
            &alice,
            query.clone()
        )
        .await
        .1["page"]["records"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/memory/query",
            &foreign,
            query.clone()
        )
        .await
        .1["page"]["records"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    let mut forged = query;
    forged["scope"]["mission_id"] = "not-granted".into();
    assert_eq!(
        request(&router, "POST", "/api/v1/memory/query", &alice, forged)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
}

#[test]
#[ignore = "Subprocess fixture invoked by the platform memory crash-recovery test"]
fn platform_http_memory_child() {
    use std::io::Write;
    let path = std::env::var("QILBEE_TEST_CRASH_DATA").unwrap();
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (router, _) = app(std::path::Path::new(&path));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        println!("HTTP_CRASH_READY {}", listener.local_addr().unwrap().port());
        std::io::stdout().flush().unwrap();
        axum::serve(listener, router).await.unwrap();
    });
}
#[test]
fn platform_http_memory_acknowledged_commands_and_receipts_survive_process_kill() {
    use crate::http_server_tests::{crash_server_for, wire_request};
    let dir = TempDir::new().unwrap();
    let token = {
        let (_, identity) = app(dir.path());
        let admin = identity
            .bootstrap_tenant("crash-tenant", "operator")
            .unwrap();
        memory_key(&identity, &admin.secret, "writer", true)
    };
    let fixture = "platform_http_tests::platform_http_memory_child";
    let (mut server, port) = crash_server_for(dir.path(), fixture);
    let mut acknowledged = Vec::new();
    for sequence in 0..20 {
        let command = memory_create(
            &format!("crash-{sequence}"),
            &format!("value-{sequence}"),
            "shared",
        );
        let (status, result) = wire_request(
            port,
            "POST",
            "/api/v1/memory/commands",
            &token,
            command.clone(),
        );
        assert_eq!(status, 200, "{result}");
        acknowledged.push((command, result));
    }
    server.0.kill().unwrap();
    server.0.wait().unwrap();
    let (_restarted, port) = crash_server_for(dir.path(), fixture);
    for (sequence, (command, receipt)) in acknowledged.iter().enumerate() {
        let id = receipt["receipt"]["record_id"].as_str().unwrap();
        let (status, record) =
            wire_request(port, "GET", &record_url(id, "shared"), &token, Value::Null);
        assert_eq!(status, 200, "{record}");
        assert_eq!(
            record["record"]["payload"]["content"]["primary"],
            format!("value-{sequence}")
        );
        assert_eq!(
            wire_request(
                port,
                "POST",
                "/api/v1/memory/commands",
                &token,
                command.clone()
            )
            .1,
            *receipt
        );
    }
    let query = json!({"contract_version":1,"scope":memory_scope("shared"),"filter":{"limit":100}});
    let (status, page) = wire_request(port, "POST", "/api/v1/memory/query", &token, query);
    assert_eq!(status, 200);
    assert_eq!(
        page["page"]["records"].as_array().unwrap().len(),
        acknowledged.len()
    );
}

#[tokio::test]
async fn platform_http_memory_rejects_unrecognized_content_instead_of_dropping_it() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("tenant-a", "operator").unwrap();
    let token = memory_key(&identity, &admin.secret, "writer", true);
    let mut command = memory_create("unknown-field", "content", "shared");
    command["operation"]["record"]["content"]["silently_lost"] = "must not disappear".into();
    assert_eq!(
        request(&router, "POST", "/api/v1/memory/commands", &token, command)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn platform_http_openapi_describes_the_available_contracts() {
    let dir = TempDir::new().unwrap();
    let (router, _) = app(dir.path());
    let (status, spec) = request(&router, "GET", "/openapi.json", "", Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(spec["openapi"], "3.1.0");
    assert_eq!(spec["info"]["version"], env!("CARGO_PKG_VERSION"));
    for path in [
        "/api/v1/identity",
        "/api/v1/credentials",
        "/api/v1/credentials/{id}/rotate",
        "/api/v1/memory/commands",
        "/api/v1/memory/records/{id}",
        "/api/v1/memory/query",
    ] {
        assert!(spec["paths"].get(path).is_some(), "Missing route: {path}");
    }
    assert_eq!(
        spec["components"]["securitySchemes"]["platformCredential"]["scheme"],
        "bearer"
    );
    assert!(spec["paths"].get("/graphs").is_none());
    assert!(spec["paths"].get("/api/v1/tools/invoke").is_none());
}

mod learning;

#[tokio::test]
async fn platform_http_browser_reference_is_public_and_root_redirects_to_it() {
    let dir = TempDir::new().unwrap();
    let (router, _) = app(dir.path());
    let root = router
        .clone()
        .call(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(root.status(), StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(root.headers()["location"], "/docs");
    let response = router
        .clone()
        .call(Request::builder().uri("/docs").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response.headers()["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/html")
    );
    let html = String::from_utf8(
        to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(html.contains("QilbeeDB API Reference"));
    assert!(html.contains("/openapi.json"));
    assert!(!html.contains("https://cdn"));
    assert!(!html.contains("localStorage"));
}

mod tools;
mod semantic;
mod retrieval;
mod experiences;

mod changes;

mod memory_review;
mod derivation;

mod checkpoints;

mod verified_changes;

mod verified_checkpoints;
mod consumer_diagnostics;
mod batch_read;

mod hybrid_schema;
mod journal_audit;
mod history_errors;
mod strategies;
mod administration;

mod login;

mod directory;
mod scope_authority;
mod agents;

mod relation_changes;
mod graph_retrieval;
