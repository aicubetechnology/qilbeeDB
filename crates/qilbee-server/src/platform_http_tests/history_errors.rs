//! Exercise machine-readable failures against the contract served by a real HTTP listener.
use super::*;

async fn start(
    path: &std::path::Path,
) -> (
    reqwest::Client,
    String,
    Value,
    String,
    tokio::task::JoinHandle<()>,
) {
    let (router, identity) = app(path);
    let admin = identity.bootstrap_tenant("tenant", "operator").unwrap();
    let mut grant = spec();
    grant.capabilities = [
        Capability::MemoryRead,
        Capability::MemoryWrite,
        Capability::MemoryCheckpoint,
    ]
    .into();
    grant.grants = vec![
        serde_json::from_value(memory_scope("shared")).unwrap(),
        serde_json::from_value(memory_scope("private")).unwrap(),
    ];
    let token = identity.issue(&admin.secret, grant).unwrap().secret;
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
    (client, base, api, token, server)
}

async fn call(
    client: &reqwest::Client,
    base: &str,
    api: &Value,
    route: &str,
    token: &str,
    body: Value,
    status: u16,
    code: Option<&str>,
) -> Value {
    let response = client
        .post(format!("{base}{route}"))
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), status, "{route}");
    assert_eq!(response.headers()["cache-control"], "no-store");
    let result: Value = response.json().await.unwrap();
    if let Some(code) = code {
        assert_eq!(result["error"]["code"], code, "{route}: {result}");
        assert_eq!(result["contract_version"], 1);
    }
    let response_schema = &api["paths"][route]["post"]["responses"][status.to_string()]["content"]
        ["application/json"]["schema"];
    assert!(
        !response_schema.is_null(),
        "Missing {route} {status} schema"
    );
    let schema = json!({"$schema":"https://json-schema.org/draft/2020-12/schema","allOf":[response_schema],"components":api["components"]});
    let validator = jsonschema::draft202012::options().build(&schema).unwrap();
    let errors: Vec<_> = validator
        .iter_errors(&result)
        .map(|error| error.to_string())
        .collect();
    assert!(errors.is_empty(), "{route}: {errors:?}");
    result
}

#[tokio::test]
async fn history_error_http_distinguishes_reconciliation_regression_cas_and_idempotency() {
    let dir = TempDir::new().unwrap();
    let (client, base, api, token, server) = start(dir.path()).await;
    let scope = memory_scope("shared");
    let page_body = json!({"contract_version":2,"scope":scope,"query":{"limit":10}});
    for n in 0..2 {
        call(
            &client,
            &base,
            &api,
            "/api/v1/memory/commands",
            &token,
            memory_create(&format!("source-{n}"), "content", "shared"),
            200,
            None,
        )
        .await;
    }
    let page = call(
        &client,
        &base,
        &api,
        "/api/v2/memory/changes",
        &token,
        page_body.clone(),
        200,
        None,
    )
    .await;
    let cursor = page["page"]["high_watermark"].clone();
    let baseline = page["page"]["baseline"].clone();
    let mut mismatched = cursor.clone();
    mismatched["prefix_digest"] = "f".repeat(64).into();
    for route in ["/api/v2/memory/changes", "/api/v2/memory/changes/audit"] {
        let mut body = page_body.clone();
        body["query"]["after"] = mismatched.clone();
        call(
            &client,
            &base,
            &api,
            route,
            &token,
            body.clone(),
            409,
            Some("journal_history_conflict"),
        )
        .await;
        body["scope"] = memory_scope("private");
        call(
            &client,
            &base,
            &api,
            route,
            &token,
            body,
            409,
            Some("journal_history_conflict"),
        )
        .await;
        let mut malformed = page_body.clone();
        malformed["query"]["after"] = cursor.clone();
        malformed["query"]["after"]["prefix_digest"] = "invalid".into();
        call(
            &client,
            &base,
            &api,
            route,
            &token,
            malformed,
            400,
            Some("invalid_request"),
        )
        .await;
    }
    let create = json!({"scope":scope,"command":{"contract_version":2,"idempotency_key":"initial","consumer_id":"cache","expected_revision":0,"expected_checkpoint_digest":null,"cursor":cursor}});
    let mut malformed = create.clone();
    malformed["scope"] = memory_scope("private");
    malformed["command"]["cursor"]["prefix_digest"] = "invalid".into();
    call(
        &client,
        &base,
        &api,
        "/api/v2/memory/checkpoints",
        &token,
        malformed,
        400,
        Some("invalid_request"),
    )
    .await;
    let receipt = call(
        &client,
        &base,
        &api,
        "/api/v2/memory/checkpoints",
        &token,
        create.clone(),
        200,
        None,
    )
    .await;
    let mut next = create.clone();
    next["command"]["idempotency_key"] = "advance".into();
    next["command"]["expected_revision"] = 1.into();
    next["command"]["expected_checkpoint_digest"] =
        receipt["receipt"]["checkpoint"]["checkpoint_digest"].clone();
    let mut bad = next.clone();
    bad["command"]["cursor"] = mismatched.clone();
    call(
        &client,
        &base,
        &api,
        "/api/v2/memory/checkpoints",
        &token,
        bad.clone(),
        409,
        Some("journal_history_conflict"),
    )
    .await;
    bad["command"]["expected_revision"] = 0.into();
    bad["command"]["expected_checkpoint_digest"] = Value::Null;
    bad["scope"] = memory_scope("private");
    call(
        &client,
        &base,
        &api,
        "/api/v2/memory/checkpoints",
        &token,
        bad,
        409,
        Some("journal_history_conflict"),
    )
    .await;
    let mut backward = next.clone();
    backward["command"]["cursor"] = baseline.clone();
    call(
        &client,
        &base,
        &api,
        "/api/v2/memory/checkpoints",
        &token,
        backward,
        409,
        Some("checkpoint_regression"),
    )
    .await;
    let mut stale = next.clone();
    stale["command"]["expected_checkpoint_digest"] = "f".repeat(64).into();
    call(
        &client,
        &base,
        &api,
        "/api/v2/memory/checkpoints",
        &token,
        stale,
        409,
        Some("revision_conflict"),
    )
    .await;
    let mut changed = create.clone();
    changed["command"]["cursor"] = baseline.clone();
    call(
        &client,
        &base,
        &api,
        "/api/v2/memory/checkpoints",
        &token,
        changed,
        409,
        Some("idempotency_conflict"),
    )
    .await;
    let mut recovery = next.clone();
    recovery["command"]["idempotency_key"] = "recover".into();
    recovery["command"]["evidence_ref"] = "fixture://consumer/reconciled".into();
    recovery["command"]["cursor"] = mismatched;
    call(
        &client,
        &base,
        &api,
        "/api/v2/memory/checkpoints/recover",
        &token,
        recovery.clone(),
        409,
        Some("journal_history_conflict"),
    )
    .await;
    recovery["command"]["cursor"] = baseline;
    let recovered = call(
        &client,
        &base,
        &api,
        "/api/v2/memory/checkpoints/recover",
        &token,
        recovery.clone(),
        200,
        None,
    )
    .await;
    let mut missing = recovery.clone();
    missing["command"]["consumer_id"] = "missing".into();
    missing["command"]["idempotency_key"] = "missing-consumer".into();
    call(
        &client,
        &base,
        &api,
        "/api/v2/memory/checkpoints/recover",
        &token,
        missing,
        409,
        Some("revision_conflict"),
    )
    .await;
    let mut changed = recovery.clone();
    changed["command"]["evidence_ref"] = "fixture://different".into();
    call(
        &client,
        &base,
        &api,
        "/api/v2/memory/checkpoints/recover",
        &token,
        changed,
        409,
        Some("idempotency_conflict"),
    )
    .await;
    let mut stale = recovery.clone();
    stale["command"]["idempotency_key"] = "stale".into();
    call(
        &client,
        &base,
        &api,
        "/api/v2/memory/checkpoints/recover",
        &token,
        stale,
        409,
        Some("revision_conflict"),
    )
    .await;
    assert_eq!(
        call(
            &client,
            &base,
            &api,
            "/api/v2/memory/checkpoints",
            &token,
            create,
            200,
            None
        )
        .await,
        receipt
    );
    assert_eq!(
        call(
            &client,
            &base,
            &api,
            "/api/v2/memory/checkpoints/recover",
            &token,
            recovery,
            200,
            None
        )
        .await,
        recovered
    );
    let current = call(
        &client,
        &base,
        &api,
        "/api/v2/memory/checkpoints/read",
        &token,
        json!({"contract_version":2,"scope":scope,"consumer_id":"cache"}),
        200,
        None,
    )
    .await;
    assert_eq!(current["checkpoint"], recovered["receipt"]["checkpoint"]);
    // The legacy endpoint keeps its existing error category.
    let legacy = json!({"contract_version":1,"scope":memory_scope("private"),"query":{"limit":1,"after":{"journal_id":cursor["journal_id"],"sequence":cursor["sequence"]}}});
    call(
        &client,
        &base,
        &api,
        "/api/v1/memory/changes",
        &token,
        legacy,
        409,
        Some("idempotency_conflict"),
    )
    .await;
    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn history_error_http_unknown_consumers_match_the_served_openapi() {
    let dir = TempDir::new().unwrap();
    let (client, base, api, token, server) = start(dir.path()).await;
    for version in [1, 2] {
        call(&client, &base, &api, &format!("/api/v{version}/memory/checkpoints/read"), &token,
            json!({"contract_version":version,"scope":memory_scope("shared"),"consumer_id":"missing"}), 404, Some("checkpoint_not_found")).await;
    }
    call(&client, &base, &api, "/api/v2/memory/checkpoints/recoveries/read", &token,
        json!({"contract_version":2,"scope":memory_scope("shared"),"consumer_id":"missing","revision":1}), 404, Some("recovery_not_found")).await;
    server.abort();
    let _ = server.await;
}
