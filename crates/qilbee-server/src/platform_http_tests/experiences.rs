use super::*;

fn scope() -> Value {
    json!({"project_id":"project","mission_id":null,"agent_id":"agent","visibility":"shared"})
}
fn create_body() -> Value {
    json!({"contract_version":1,"scope":scope(),"request":{"id":"attempt-v1","context_id":"context-v1","reporter_subject_id":"observer","accounting_unit":"test-credit-v1","input":{"reference":"fixture:input","sha256":"a".repeat(64)},"parent":null}})
}
fn read_body() -> Value {
    json!({"contract_version":1,"scope":scope(),"attempt_id":"attempt-v1"})
}
fn report_body(digest: &str) -> Value {
    json!({"contract_version":1,"scope":scope(),"attempt_id":"attempt-v1","command":{"event_id":"observation-v1","expected_revision":1,"context_digest":digest,"outcome":"unknown","evidence":{"reference":"fixture:trace","sha256":"b".repeat(64)},"cost_units":null,"latency_ms":null}})
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
async fn configure(router: &Router, admin: &str) {
    let body = json!({"contract_version":1,"id":"context-v1","context":{"task":"task-v1","baseline_revision":"baseline-v1","model_provider":"external","model_revision":"model-v1","tools":{},"environment_revision":"env-v1","evaluation_contract":"verifier-v1","dataset_revision":"data-v1","harness_revision":"harness-v1","permissions_revision":"permissions-v1"}});
    assert_eq!(
        request(router, "POST", "/api/v1/learning/contexts", admin, body)
            .await
            .0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn experience_http_replays_original_events_and_recovers_after_reopen() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("tenant", "operator").unwrap();
    configure(&router, &admin.secret).await;
    let writer = issue(
        &identity,
        &admin.secret,
        "writer",
        &["experience_write", "experience_read"],
    );
    let reporter = issue(
        &identity,
        &admin.secret,
        "observer",
        &["experience_report", "experience_read"],
    );
    let initial = request(
        &router,
        "POST",
        "/api/v1/experiences",
        &writer.secret,
        create_body(),
    )
    .await;
    assert_eq!(initial.0, StatusCode::OK, "{}", initial.1);
    let command = report_body(initial.1["receipt"]["context_digest"].as_str().unwrap());
    let event = request(
        &router,
        "POST",
        "/api/v1/experiences/events",
        &reporter.secret,
        command.clone(),
    )
    .await;
    assert_eq!(event.0, StatusCode::OK, "{}", event.1);
    assert_eq!(event.1["event"]["actor"]["subject_id"], "observer");
    assert_eq!(
        event.1["event"]["record"]["reported_cost_units"],
        Value::Null
    );
    let mut done = command.clone();
    done["command"]["event_id"] = "done".into();
    done["command"]["expected_revision"] = 2.into();
    done["command"]["outcome"] = "succeeded".into();
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/experiences/events",
            &reporter.secret,
            done
        )
        .await
        .0,
        StatusCode::OK
    );
    drop(router);
    drop(identity);
    let (router, _) = app(dir.path());
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/experiences",
            &writer.secret,
            create_body()
        )
        .await,
        initial
    );
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/experiences/events",
            &reporter.secret,
            command
        )
        .await,
        event
    );
    let current = request(
        &router,
        "POST",
        "/api/v1/experiences/read",
        &writer.secret,
        read_body(),
    )
    .await;
    assert_eq!(current.1["experience"]["revision"], 3);
    assert_eq!(current.1["experience"]["outcome"], "succeeded");
    let mut old = read_body();
    old["event_id"] = "observation-v1".into();
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/experiences/events/read",
            &writer.secret,
            old
        )
        .await,
        event
    );
}

#[tokio::test]
async fn experience_http_enforces_current_credentials_roles_and_strict_bodies() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("tenant", "operator").unwrap();
    configure(&router, &admin.secret).await;
    let writer = issue(
        &identity,
        &admin.secret,
        "writer",
        &["experience_write", "experience_read"],
    );
    let reporter = issue(&identity, &admin.secret, "observer", &["experience_report"]);
    let other = issue(&identity, &admin.secret, "other", &["experience_report"]);
    let memory = issue(
        &identity,
        &admin.secret,
        "memory",
        &["memory_read", "memory_write"],
    );
    assert_eq!(
        request(&router, "POST", "/api/v1/experiences", "", create_body())
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    for key in [&admin.secret, &memory.secret, &reporter.secret] {
        assert_eq!(
            request(&router, "POST", "/api/v1/experiences", key, create_body())
                .await
                .0,
            StatusCode::FORBIDDEN
        );
    }
    let mut forged = create_body();
    forged["request"]["actor"] = json!("operator");
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/experiences",
            &writer.secret,
            forged
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    for reporter in ["invalid\nsubject".to_string(), "x".repeat(257)] {
        let mut invalid = create_body();
        invalid["request"]["reporter_subject_id"] = reporter.into();
        assert_eq!(
            request(
                &router,
                "POST",
                "/api/v1/experiences",
                &writer.secret,
                invalid
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
    }
    let initial = request(
        &router,
        "POST",
        "/api/v1/experiences",
        &writer.secret,
        create_body(),
    )
    .await;
    let body = report_body(initial.1["receipt"]["context_digest"].as_str().unwrap());
    for key in [&writer.secret, &other.secret] {
        assert_eq!(
            request(
                &router,
                "POST",
                "/api/v1/experiences/events",
                key,
                body.clone()
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
    }
    let event = request(
        &router,
        "POST",
        "/api/v1/experiences/events",
        &reporter.secret,
        body.clone(),
    )
    .await;
    assert_eq!(event.0, StatusCode::OK);
    let rotated = identity
        .rotate(&admin.secret, reporter.credential.id, 1)
        .unwrap();
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/experiences/events",
            &reporter.secret,
            body.clone()
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/experiences/events",
            &rotated.secret,
            body.clone()
        )
        .await,
        event
    );
    identity
        .revoke(&admin.secret, rotated.credential.id, 2)
        .unwrap();
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/experiences/events",
            &rotated.secret,
            body
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn experience_http_isolates_tenants_private_subjects_and_all_resource_fields() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("tenant", "operator").unwrap();
    configure(&router, &admin.secret).await;
    let foreign = identity.bootstrap_tenant("foreign", "operator").unwrap();
    configure(&router, &foreign.secret).await;
    let owner = issue(
        &identity,
        &admin.secret,
        "owner",
        &["experience_write", "experience_read"],
    );
    let second = issue(
        &identity,
        &admin.secret,
        "second",
        &["experience_write", "experience_read"],
    );
    let outsider = issue(
        &identity,
        &foreign.secret,
        "owner",
        &["experience_write", "experience_read"],
    );
    let mut body = create_body();
    body["scope"]["visibility"] = "private".into();
    body["request"]["reporter_subject_id"] = "owner".into();
    let mut invalid = body.clone();
    invalid["request"]["reporter_subject_id"] = "second".into();
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/experiences",
            &owner.secret,
            invalid
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/experiences",
            &owner.secret,
            body.clone()
        )
        .await
        .0,
        StatusCode::OK
    );
    let mut read = read_body();
    read["scope"]["visibility"] = "private".into();
    for key in [&second.secret, &outsider.secret] {
        assert_eq!(
            request(
                &router,
                "POST",
                "/api/v1/experiences/read",
                key,
                read.clone()
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );
    }
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/experiences",
            &outsider.secret,
            body.clone()
        )
        .await
        .0,
        StatusCode::OK
    );
    for field in ["project_id", "mission_id", "agent_id"] {
        let mut changed = read.clone();
        changed["scope"][field] = "different".into();
        assert_eq!(
            request(
                &router,
                "POST",
                "/api/v1/experiences/read",
                &owner.secret,
                changed
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
    }
}

#[tokio::test]
async fn experience_history_http_checks_authority_on_each_continuation() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("tenant", "operator").unwrap();
    configure(&router, &admin.secret).await;
    let client = issue(
        &identity,
        &admin.secret,
        "observer",
        &["experience_write", "experience_read", "experience_report"],
    );
    let receipt = request(
        &router,
        "POST",
        "/api/v1/experiences",
        &client.secret,
        create_body(),
    )
    .await
    .1;
    let report = report_body(receipt["receipt"]["context_digest"].as_str().unwrap());
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/experiences/events",
            &client.secret,
            report
        )
        .await
        .0,
        StatusCode::OK
    );
    let mut body = read_body();
    body["query"] = json!({"limit":1,"cursor":null});
    let page = request(
        &router,
        "POST",
        "/api/v1/experiences/history",
        &client.secret,
        body.clone(),
    )
    .await;
    assert_eq!(page.0, StatusCode::OK);
    assert_eq!(page.1["page"]["events"].as_array().unwrap().len(), 1);
    body["query"]["cursor"] = page.1["page"]["next_cursor"].clone();
    let denied = issue(&identity, &admin.secret, "observer", &["memory_read"]);
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/experiences/history",
            &denied.secret,
            body.clone()
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let mut private = body.clone();
    private["scope"]["visibility"] = "private".into();
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/experiences/history",
            &client.secret,
            private
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/experiences/history",
            &client.secret,
            body.clone()
        )
        .await
        .1["page"]["complete"],
        true
    );
    identity
        .revoke(&admin.secret, client.credential.id, 1)
        .unwrap();
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/experiences/history",
            &client.secret,
            body
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
}
