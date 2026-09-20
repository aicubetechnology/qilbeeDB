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
