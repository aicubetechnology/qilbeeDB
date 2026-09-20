use uuid::Uuid;
use super::*;
fn reviewer(identity: &IdentityStore, admin: &str, subject: &str) -> String {
    let mut s = spec();
    s.subject_id = subject.into();
    s.capabilities = [Capability::MemoryReview].into();
    s.grants = vec![
        serde_json::from_value(memory_scope("shared")).unwrap(),
        serde_json::from_value(memory_scope("private")).unwrap(),
    ];
    identity.issue(admin, s).unwrap().secret
}
fn review(id: Value, visibility: &str) -> Value {
    json!({"scope":memory_scope(visibility),"command":{"contract_version":1,"idempotency_key":"review","record_id":id,"expected_revision":1,"disposition":"rejected","evidence_ref":"test://evidence"}})
}
#[tokio::test]
async fn memory_review_separate_authority_scope_and_revocation() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("tenant", "operator").unwrap();
    let foreign = identity.bootstrap_tenant("foreign", "operator").unwrap();
    let writer = memory_key(&identity, &admin.secret, "writer", true);
    let key = reviewer(&identity, &admin.secret, "reviewer");
    let outside = reviewer(&identity, &foreign.secret, "reviewer");
    for visibility in ["shared", "private"] {
        let created = request(
            &router,
            "POST",
            "/api/v1/memory/commands",
            &writer,
            memory_create(visibility, "reviewed content", visibility),
        )
        .await;
        assert_eq!(created.0, StatusCode::OK);
        let id = created.1["receipt"]["record_id"].clone();
        let mut body = review(id.clone(), visibility);
        assert_eq!(
            request(
                &router,
                "POST",
                "/api/v1/memory/reviews",
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
                "/api/v1/memory/reviews",
                &outside,
                body.clone()
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );
        if visibility == "private" {
            assert_eq!(
                request(
                    &router,
                    "POST",
                    "/api/v1/memory/reviews",
                    &key,
                    body.clone()
                )
                .await
                .0,
                StatusCode::NOT_FOUND
            );
            continue;
        }
        for field in ["project_id", "mission_id", "agent_id"] {
            let mut wrong = body.clone();
            wrong["scope"][field] = "wrong".into();
            assert_eq!(
                request(&router, "POST", "/api/v1/memory/reviews", &key, wrong)
                    .await
                    .0,
                StatusCode::FORBIDDEN
            );
        }
        let accepted = request(
            &router,
            "POST",
            "/api/v1/memory/reviews",
            &key,
            body.clone(),
        )
        .await;
        assert_eq!(accepted.0, StatusCode::OK);
        assert_eq!(
            accepted,
            request(
                &router,
                "POST",
                "/api/v1/memory/reviews",
                &key,
                body.clone()
            )
            .await
        );
        assert_eq!(
            request(
                &router,
                "GET",
                &record_url(id.as_str().unwrap(), visibility),
                &writer,
                Value::Null
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );
        let read = json!({"contract_version":1,"scope":memory_scope(visibility),"record_id":id,"revision":2});
        assert_eq!(
            request(&router, "POST", "/api/v1/memory/reviews/read", &key, read).await,
            accepted
        );
        let state = json!({"contract_version":1,"scope":memory_scope(visibility),"record_id":id});
        assert_eq!(
            request(
                &router,
                "POST",
                "/api/v1/memory/reviews/state",
                &key,
                state.clone()
            )
            .await
            .1["state"]["revision"],
            2
        );
        assert_eq!(
            request(
                &router,
                "POST",
                "/api/v1/memory/reviews/state",
                &writer,
                state
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
        body["command"]["author"] = json!({"subject_id":"spoof"});
        assert_eq!(
            request(&router, "POST", "/api/v1/memory/reviews", &key, body)
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
    }
    let principal = identity.authenticate(&key).unwrap();
    identity
        .revoke(&admin.secret, principal.id, principal.revision)
        .unwrap();
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/memory/reviews",
            &key,
            review(json!(Uuid::new_v4()), "shared")
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
}
#[tokio::test]
async fn memory_review_receipts_and_hidden_state_survive_server_reopen() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("tenant", "operator").unwrap();
    let writer = memory_key(&identity, &admin.secret, "writer", true);
    let key = reviewer(&identity, &admin.secret, "writer");
    let created = request(
        &router,
        "POST",
        "/api/v1/memory/commands",
        &writer,
        memory_create("create", "private memory", "private"),
    )
    .await;
    let id = created.1["receipt"]["record_id"].clone();
    let command = review(id.clone(), "private");
    let receipt = request(
        &router,
        "POST",
        "/api/v1/memory/reviews",
        &key,
        command.clone(),
    )
    .await;
    assert_eq!(receipt.0, StatusCode::OK);
    drop(router);
    drop(identity);
    let (router, _) = app(dir.path());
    assert_eq!(
        request(&router, "POST", "/api/v1/memory/reviews", &key, command).await,
        receipt
    );
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/memory/reviews/state",
            &key,
            json!({"contract_version":1,"scope":memory_scope("private"),"record_id":id})
        )
        .await
        .1["state"]["review"]["disposition"],
        "rejected"
    );
}
