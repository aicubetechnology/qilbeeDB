use super::*;

const ROUTE: &str = "/api/v2/memory/changes/audit";

fn body(visibility: &str) -> Value {
    json!({"contract_version":2,"scope":memory_scope(visibility),"query":{"after":null,"through":null,"limit":1}})
}

#[tokio::test]
async fn journal_audit_http_preserves_fence_and_restart_without_exposing_events() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("tenant", "operator").unwrap();
    let writer = memory_key(&identity, &admin.secret, "writer", true);
    let reader = memory_key(&identity, &admin.secret, "auditor", false);
    let inactive = request(&router, "POST", ROUTE, &reader, body("shared")).await;
    assert_eq!(inactive.0, StatusCode::OK);
    assert_eq!(inactive.1["audit"]["active"], false);
    assert_eq!(inactive.1["audit"]["links_checked"], 0);
    for n in 0..3 {
        assert_eq!(
            request(
                &router,
                "POST",
                "/api/v1/memory/commands",
                &writer,
                memory_create(&format!("event-{n}"), "sensitive memory body", "shared")
            )
            .await
            .0,
            StatusCode::OK
        );
    }
    let first = request(&router, "POST", ROUTE, &reader, body("shared")).await;
    assert_eq!(first.0, StatusCode::OK);
    assert_eq!(first.1["audit"]["links_checked"], 1);
    assert_eq!(first.1["audit"]["complete"], false);
    assert_eq!(first.1["audit"]["checked_after"]["sequence"], 0);
    assert_eq!(first.1["audit"]["checked_through"]["sequence"], 1);
    for field in ["changes", "record_id", "author", "sensitive memory body"] {
        assert!(!first.1.to_string().contains(field));
    }
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/memory/commands",
            &writer,
            memory_create("later", "later", "shared")
        )
        .await
        .0,
        StatusCode::OK
    );
    let mut next = body("shared");
    next["query"]["after"] = first.1["audit"]["checked_through"].clone();
    next["query"]["through"] = first.1["audit"]["high_watermark"].clone();
    next["query"]["limit"] = 2.into();
    drop(router);
    drop(identity);
    let (router, _) = app(dir.path());
    let last = request(&router, "POST", ROUTE, &reader, next.clone()).await;
    assert_eq!(last.0, StatusCode::OK);
    assert_eq!(last.1["audit"]["links_checked"], 2);
    assert_eq!(last.1["audit"]["complete"], true);
    assert_eq!(last.1["audit"]["checked_through"]["sequence"], 3);
    assert_eq!(last, request(&router, "POST", ROUTE, &reader, next).await);
    let tip = request(&router, "POST", ROUTE, &reader, body("shared")).await;
    assert_eq!(tip.1["audit"]["high_watermark"]["sequence"], 4);
}

#[tokio::test]
async fn journal_audit_http_reauthorizes_tenants_private_subjects_and_revoked_keys() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("tenant", "operator").unwrap();
    let foreign = identity.bootstrap_tenant("foreign", "operator").unwrap();
    let writer = memory_key(&identity, &admin.secret, "writer", true);
    let other = memory_key(&identity, &admin.secret, "other", false);
    let outside = memory_key(&identity, &foreign.secret, "writer", false);
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/memory/commands",
            &writer,
            memory_create("private", "private", "private")
        )
        .await
        .0,
        StatusCode::OK
    );
    let first = request(&router, "POST", ROUTE, &writer, body("private")).await;
    assert_eq!(first.0, StatusCode::OK);
    for key in [&other, &outside] {
        let empty = request(&router, "POST", ROUTE, key, body("private")).await;
        assert_eq!(empty.0, StatusCode::OK);
        assert_eq!(empty.1["audit"]["active"], false);
        let mut wrong = body("private");
        wrong["query"]["after"] = first.1["audit"]["checked_through"].clone();
        assert_eq!(
            request(&router, "POST", ROUTE, key, wrong).await.0,
            StatusCode::CONFLICT
        );
    }
    for field in ["project_id", "mission_id", "agent_id"] {
        let mut wrong = body("private");
        wrong["scope"][field] = "forbidden".into();
        assert_eq!(
            request(&router, "POST", ROUTE, &writer, wrong).await.0,
            StatusCode::FORBIDDEN
        );
    }
    for limit in [0, 257] {
        let mut wrong = body("private");
        wrong["query"]["limit"] = limit.into();
        assert_eq!(
            request(&router, "POST", ROUTE, &writer, wrong).await.0,
            StatusCode::BAD_REQUEST
        );
    }
    let mut wrong = body("private");
    wrong["contract_version"] = 1.into();
    assert_eq!(
        request(&router, "POST", ROUTE, &writer, wrong).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        request(&router, "POST", ROUTE, &admin.secret, body("private"))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let principal = identity.authenticate(&writer).unwrap();
    identity
        .revoke(&admin.secret, principal.id, principal.revision)
        .unwrap();
    assert_eq!(
        request(&router, "POST", ROUTE, &writer, body("private"))
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
}
