use super::*;

fn scope() -> Value {
    json!({"project_id":"project","mission_id":null,"agent_id":"agent","visibility":"shared"})
}
fn artifact(id: &str) -> Value {
    json!({"id":id,"source":"def run(value):\n    return value\n","dependency_lock":"","runtime_image_digest":format!("sha256:{}","a".repeat(64)),"entrypoint":"tool:run","input_schema":{"type":"object"},"output_schema":true,"source_refs":["development:request-v1"],"parent_artifact_id":null,"repair_evidence_ref":null})
}
fn profile() -> Value {
    json!({"contract_version":1,"profile":{"id":"worker-v1","subject_id":"worker","runtime_image_digest":format!("sha256:{}","a".repeat(64)),"environment_revision":"sandbox-v1","permissions_revision":"no-network-v1","max_cost_units":100,"max_latency_ms":60000}})
}
fn development() -> Value {
    json!({"contract_version":1,"scope":scope(),"request":{"id":"request-v1","executor_id":"worker-v1","objective":"Develop an identity tool and run its tests.","parent_artifact_id":null,"repair_evidence_ref":null}})
}
fn read() -> Value {
    json!({"contract_version":1,"scope":scope(),"request_id":"request-v1"})
}
fn report(digest: &str) -> Value {
    json!({"contract_version":1,"scope":scope(),"request_id":"request-v1","command":{"event_id":"success-v1","expected_revision":1,"action":{"type":"report","report":{"executor_profile_digest":digest,"outcome":"succeeded","evidence_ref":"test-run-v1","detail":null,"cost_units":10,"latency_ms":100,"artifact":artifact("candidate-v1")}}}})
}
fn issue(
    identity: &IdentityStore,
    admin: &str,
    subject: &str,
    capabilities: &[&str],
) -> crate::security::identity::IssuedCredential {
    let mut client = spec();
    client.subject_id = subject.into();
    client.capabilities = serde_json::from_value(json!(capabilities)).unwrap();
    let mut private = client.grants[0].clone();
    private.visibility = Visibility::Private;
    client.grants.push(private);
    identity.issue(admin, client).unwrap()
}
async fn configure(
    router: &Router,
    identity: &IdentityStore,
    admin: &str,
) -> (String, String, String) {
    let administrator = issue(identity, admin, "tool-admin", &["tool_admin"]);
    let developer = issue(identity, admin, "developer", &["tool_develop", "tool_read"]);
    let worker = issue(identity, admin, "worker", &["tool_report", "tool_read"]);
    let (status, body) = request(
        router,
        "POST",
        "/api/v1/tools/executors",
        &administrator.secret,
        profile(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let digest = body["executor"]["profile_digest"]
        .as_str()
        .unwrap()
        .to_owned();
    (developer.secret, worker.secret, digest)
}
#[tokio::test]
async fn tool_http_success_recovery_and_original_event_replay() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("tenant", "admin").unwrap();
    let (developer, worker, digest) = configure(&router, &identity, &admin.secret).await;
    let original = request(
        &router,
        "POST",
        "/api/v1/tools/development/requests",
        &developer,
        development(),
    )
    .await;
    assert_eq!(original.0, StatusCode::OK);
    let event = request(
        &router,
        "POST",
        "/api/v1/tools/development/commands",
        &worker,
        report(&digest),
    )
    .await;
    assert_eq!(event.0, StatusCode::OK);
    assert_eq!(event.1["event"]["record"]["state"], "succeeded");
    assert_eq!(event.1["event"]["actor"]["subject_id"], "worker");
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/tools/development/requests",
            &developer,
            development()
        )
        .await
        .1,
        original.1
    );
    drop(router);
    drop(identity);
    let (router, _) = app(dir.path());
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/tools/development/commands",
            &worker,
            report(&digest)
        )
        .await
        .1,
        event.1
    );
    let current = request(
        &router,
        "POST",
        "/api/v1/tools/development/read",
        &developer,
        read(),
    )
    .await;
    assert_eq!(current.1["development"]["revision"], 2);
    let artifact = request(
        &router,
        "POST",
        "/api/v1/tools/artifacts/read",
        &developer,
        json!({"contract_version":1,"scope":scope(),"artifact_id":"candidate-v1"}),
    )
    .await;
    assert_eq!(artifact.0, StatusCode::OK);
    assert_eq!(
        artifact.1["artifact"]["artifact_digest"],
        current.1["development"]["artifact_digest"]
    );
    let mut event_read = read();
    event_read["event_id"] = "success-v1".into();
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/tools/development/events/read",
            &developer,
            event_read
        )
        .await
        .1,
        event.1
    );
}
#[tokio::test]
async fn tool_http_enforces_independent_roles_and_strict_actor_inputs() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("tenant", "admin").unwrap();
    let (developer, worker, digest) = configure(&router, &identity, &admin.secret).await;
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/tools/executors",
            &developer,
            profile()
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/tools/development/requests",
            &worker,
            development()
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/tools/development/requests",
            &developer,
            development()
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/tools/development/commands",
            &developer,
            report(&digest)
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let intruder = issue(&identity, &admin.secret, "intruder", &["tool_report"]);
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/tools/development/commands",
            &intruder.secret,
            report(&digest)
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let mut forged = report(&digest);
    forged["command"]["action"]["report"]["actor"] = "worker".into();
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/tools/development/commands",
            &worker,
            forged
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let memory = identity.issue(&admin.secret, spec()).unwrap();
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/tools/development/read",
            &memory.secret,
            read()
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
}
#[tokio::test]
async fn tool_http_cancellation_revocation_and_unknown_cost_are_observable() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("tenant", "admin").unwrap();
    let (developer, worker, digest) = configure(&router, &identity, &admin.secret).await;
    request(
        &router,
        "POST",
        "/api/v1/tools/development/requests",
        &developer,
        development(),
    )
    .await;
    let mut unknown = report(&digest);
    unknown["command"]["action"]["report"]["cost_units"] = Value::Null;
    let event = request(
        &router,
        "POST",
        "/api/v1/tools/development/commands",
        &worker,
        unknown,
    )
    .await;
    assert_eq!(event.1["event"]["record"]["state"], "pending_or_unknown");
    assert!(event.1["event"]["record"]["cost_units"].is_null());
    let cancel = json!({"contract_version":1,"scope":scope(),"request_id":"request-v1","command":{"event_id":"cancel","expected_revision":2,"action":{"type":"request_cancellation","reason":"Mission ended"}}});
    let intruder = issue(&identity, &admin.secret, "intruder", &["tool_develop"]);
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/tools/development/commands",
            &intruder.secret,
            cancel.clone()
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let event = request(
        &router,
        "POST",
        "/api/v1/tools/development/commands",
        &developer,
        cancel,
    )
    .await;
    assert_eq!(
        event.1["event"]["record"]["state"],
        "cancellation_requested"
    );
    let credential = identity.authenticate(&worker).unwrap();
    identity
        .revoke(&admin.secret, credential.id, credential.revision)
        .unwrap();
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/tools/development/commands",
            &worker,
            report(&digest)
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/tools/development/read",
            &developer,
            read()
        )
        .await
        .1["development"]["state"],
        "cancellation_requested"
    );
}
#[tokio::test]
async fn tool_http_artifacts_and_development_preserve_tenant_and_private_scope() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("tenant", "admin").unwrap();
    let (developer, worker, digest) = configure(&router, &identity, &admin.secret).await;
    let registered = request(
        &router,
        "POST",
        "/api/v1/tools/artifacts",
        &developer,
        json!({"contract_version":1,"scope":scope(),"artifact":artifact("manual-v1")}),
    )
    .await;
    assert_eq!(registered.0, StatusCode::OK);
    let foreign_admin = identity.bootstrap_tenant("foreign", "admin").unwrap();
    let foreign = issue(
        &identity,
        &foreign_admin.secret,
        "developer",
        &["tool_read"],
    );
    let artifact_read = json!({"contract_version":1,"scope":scope(),"artifact_id":"manual-v1"});
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/tools/artifacts/read",
            &foreign.secret,
            artifact_read
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let mut private = development();
    private["scope"]["visibility"] = "private".into();
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/tools/development/requests",
            &developer,
            private
        )
        .await
        .0,
        StatusCode::OK
    );
    let mut private_report = report(&digest);
    private_report["scope"]["visibility"] = "private".into();
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/tools/development/commands",
            &worker,
            private_report
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let mut forged = development();
    forged["tenant_id"] = "foreign".into();
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/tools/development/requests",
            &developer,
            forged
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
}
