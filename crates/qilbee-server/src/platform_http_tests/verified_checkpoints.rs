use super::*;
fn key(identity: &IdentityStore, admin: &str, subject: &str, caps: Vec<Capability>) -> String {
    let mut s = spec();
    s.subject_id = subject.into();
    s.capabilities = caps.into_iter().collect();
    s.grants = vec![
        serde_json::from_value(memory_scope("shared")).unwrap(),
        serde_json::from_value(memory_scope("private")).unwrap(),
    ];
    identity.issue(admin, s).unwrap().secret
}
#[tokio::test]
async fn verified_checkpoint_http_requires_independent_roles_and_owns_progress_by_subject() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("tenant", "operator").unwrap();
    let foreign = identity.bootstrap_tenant("foreign", "operator").unwrap();
    let writer = memory_key(&identity, &admin.secret, "writer", true);
    let consumer = key(
        &identity,
        &admin.secret,
        "consumer",
        vec![Capability::MemoryRead, Capability::MemoryCheckpoint],
    );
    let other = key(
        &identity,
        &admin.secret,
        "other",
        vec![Capability::MemoryRead, Capability::MemoryCheckpoint],
    );
    let outside = key(
        &identity,
        &foreign.secret,
        "consumer",
        vec![Capability::MemoryRead, Capability::MemoryCheckpoint],
    );
    let no_read = key(
        &identity,
        &admin.secret,
        "consumer",
        vec![Capability::MemoryCheckpoint],
    );
    request(
        &router,
        "POST",
        "/api/v1/memory/commands",
        &writer,
        memory_create("source", "source", "shared"),
    )
    .await;
    let page = request(
        &router,
        "POST",
        "/api/v2/memory/changes",
        &consumer,
        json!({"contract_version":2,"scope":memory_scope("shared"),"query":{"limit":10}}),
    )
    .await;
    let cursor = page.1["page"]["next_cursor"].clone();
    let body = json!({"scope":memory_scope("shared"),"command":{"contract_version":2,"idempotency_key":"checkpoint","consumer_id":"cache","expected_revision":0,"cursor":cursor}});
    for denied in [&writer, &no_read] {
        assert_eq!(
            request(
                &router,
                "POST",
                "/api/v2/memory/checkpoints",
                denied,
                body.clone()
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v2/memory/checkpoints",
            &outside,
            body.clone()
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let receipt = request(
        &router,
        "POST",
        "/api/v2/memory/checkpoints",
        &consumer,
        body.clone(),
    )
    .await;
    assert_eq!(receipt.0, StatusCode::OK);
    assert_eq!(receipt.1["receipt"]["checkpoint"]["revision"], 1);
    let read = json!({"contract_version":2,"scope":memory_scope("shared"),"consumer_id":"cache"});
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v2/memory/checkpoints/read",
            &other,
            read.clone()
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    for field in ["project_id", "mission_id", "agent_id"] {
        let mut wrong = read.clone();
        wrong["scope"][field] = "other".into();
        assert_eq!(
            request(
                &router,
                "POST",
                "/api/v2/memory/checkpoints/read",
                &consumer,
                wrong
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
    }
    let mut private = read.clone();
    private["scope"]["visibility"] = "private".into();
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v2/memory/checkpoints/read",
            &consumer,
            private
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let mut spoof = body.clone();
    spoof["subject_id"] = "other".into();
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v2/memory/checkpoints",
            &consumer,
            spoof
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    drop(router);
    drop(identity);
    let (router, identity) = app(dir.path());
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v2/memory/checkpoints",
            &consumer,
            body.clone()
        )
        .await,
        receipt
    );
    let latest = request(
        &router,
        "POST",
        "/api/v2/memory/checkpoints/read",
        &consumer,
        read.clone(),
    )
    .await;
    assert_eq!(latest.1["checkpoint"], receipt.1["receipt"]["checkpoint"]);
    let principal = identity.authenticate(&consumer).unwrap();
    let rotated = identity
        .rotate(&admin.secret, principal.id, principal.revision)
        .unwrap();
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v2/memory/checkpoints/read",
            &consumer,
            read.clone()
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v2/memory/checkpoints",
            &rotated.secret,
            body
        )
        .await,
        receipt
    );
    identity
        .revoke(
            &admin.secret,
            rotated.credential.id,
            rotated.credential.revision,
        )
        .unwrap();
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v2/memory/checkpoints/read",
            &rotated.secret,
            read
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
}
#[tokio::test]
async fn checkpoint_recovery_http_records_history_with_current_subject_authorization() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("tenant", "operator").unwrap();
    let writer = memory_key(&identity, &admin.secret, "writer", true);
    let consumer = key(
        &identity,
        &admin.secret,
        "consumer",
        vec![Capability::MemoryRead, Capability::MemoryCheckpoint],
    );
    let other = key(
        &identity,
        &admin.secret,
        "other",
        vec![Capability::MemoryRead, Capability::MemoryCheckpoint],
    );
    request(
        &router,
        "POST",
        "/api/v1/memory/commands",
        &writer,
        memory_create("first", "content", "shared"),
    )
    .await;
    let page = request(
        &router,
        "POST",
        "/api/v2/memory/changes",
        &consumer,
        json!({"contract_version":2,"scope":memory_scope("shared"),"query":{"limit":10}}),
    )
    .await
    .1["page"]
        .clone();
    let cp=request(&router,"POST","/api/v2/memory/checkpoints",&consumer,json!({"scope":memory_scope("shared"),"command":{"contract_version":2,"idempotency_key":"initial","consumer_id":"cache","expected_revision":0,"cursor":page["high_watermark"]}})).await.1["receipt"]["checkpoint"].clone();
    let body = json!({"scope":memory_scope("shared"),"command":{"contract_version":2,"idempotency_key":"recover","consumer_id":"cache","expected_revision":cp["revision"],"expected_checkpoint_digest":cp["checkpoint_digest"],"cursor":page["baseline"],"evidence_ref":"fixture://reconciled"}});
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v2/memory/checkpoints/recover",
            &writer,
            body.clone()
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v2/memory/checkpoints/recover",
            &other,
            body.clone()
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let receipt = request(
        &router,
        "POST",
        "/api/v2/memory/checkpoints/recover",
        &consumer,
        body.clone(),
    )
    .await;
    assert_eq!(receipt.0, StatusCode::OK);
    assert_eq!(receipt.1["receipt"]["checkpoint"]["cursor"]["sequence"], 0);
    let history = json!({"contract_version":2,"scope":memory_scope("shared"),"consumer_id":"cache","revision":2});
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v2/memory/checkpoints/recoveries/read",
            &other,
            history.clone()
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v2/memory/checkpoints/recoveries/read",
            &consumer,
            history.clone()
        )
        .await,
        receipt
    );
    let principal = identity.authenticate(&consumer).unwrap();
    identity
        .revoke(&admin.secret, principal.id, principal.revision)
        .unwrap();
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v2/memory/checkpoints/recover",
            &consumer,
            body
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v2/memory/checkpoints/recoveries/read",
            &consumer,
            history
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
}
