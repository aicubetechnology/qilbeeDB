use super::*;
fn body(visibility: &str) -> Value {
    json!({"contract_version":1,"scope":memory_scope(visibility),"query":{"after":null,"through":null,"limit":1}})
}
#[tokio::test]
async fn memory_change_feed_preserves_resume_fence_and_restart() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("tenant", "operator").unwrap();
    let token = memory_key(&identity, &admin.secret, "writer", true);
    for n in 0..3 {
        let input = memory_create(
            &n.to_string(),
            "private payload not copied to journal",
            "shared",
        );
        let first = request(
            &router,
            "POST",
            "/api/v1/memory/commands",
            &token,
            input.clone(),
        )
        .await;
        assert_eq!(first.0, StatusCode::OK);
        assert_eq!(
            first,
            request(&router, "POST", "/api/v1/memory/commands", &token, input).await
        );
    }
    let mut query = body("shared");
    let first = request(
        &router,
        "POST",
        "/api/v1/memory/changes",
        &token,
        query.clone(),
    )
    .await;
    assert_eq!(first.0, StatusCode::OK);
    assert_eq!(first.1["page"]["changes"].as_array().unwrap().len(), 1);
    assert!(!first.1.to_string().contains("private payload"));
    query["query"]["after"] = first.1["page"]["next_cursor"].clone();
    query["query"]["through"] = first.1["page"]["high_watermark"].clone();
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/memory/commands",
            &token,
            memory_create("later", "later", "shared")
        )
        .await
        .0,
        StatusCode::OK
    );
    drop(router);
    drop(identity);
    let (router, _) = app(dir.path());
    let second = request(
        &router,
        "POST",
        "/api/v1/memory/changes",
        &token,
        query.clone(),
    )
    .await;
    assert_eq!(second.0, StatusCode::OK);
    assert_eq!(second.1["page"]["changes"][0]["cursor"]["sequence"], 2);
    query["query"]["after"] = second.1["page"]["next_cursor"].clone();
    let last = request(
        &router,
        "POST",
        "/api/v1/memory/changes",
        &token,
        query.clone(),
    )
    .await;
    assert_eq!(last.1["page"]["complete"], true);
    assert_eq!(last.1["page"]["changes"][0]["cursor"]["sequence"], 3);
    query["query"]["after"] = last.1["page"]["next_cursor"].clone();
    query["query"]["through"] = Value::Null;
    assert_eq!(
        request(&router, "POST", "/api/v1/memory/changes", &token, query)
            .await
            .1["page"]["changes"][0]["cursor"]["sequence"],
        4
    );
}
#[tokio::test]
async fn memory_change_feed_isolates_tenants_subjects_grants_and_revoked_credentials() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("tenant", "operator").unwrap();
    let foreign = identity.bootstrap_tenant("foreign", "operator").unwrap();
    let token = memory_key(&identity, &admin.secret, "writer", true);
    let other = memory_key(&identity, &admin.secret, "other", true);
    let outside = memory_key(&identity, &foreign.secret, "writer", true);
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/memory/commands",
            &token,
            memory_create("private", "private", "private")
        )
        .await
        .0,
        StatusCode::OK
    );
    let page = request(
        &router,
        "POST",
        "/api/v1/memory/changes",
        &token,
        body("private"),
    )
    .await;
    assert_eq!(page.1["page"]["changes"].as_array().unwrap().len(), 1);
    for key in [&other, &outside] {
        let empty = request(
            &router,
            "POST",
            "/api/v1/memory/changes",
            key,
            body("private"),
        )
        .await;
        assert_eq!(empty.0, StatusCode::OK);
        assert!(empty.1["page"]["changes"].as_array().unwrap().is_empty());
        let mut q = body("private");
        q["query"]["after"] = page.1["page"]["next_cursor"].clone();
        assert_eq!(
            request(&router, "POST", "/api/v1/memory/changes", key, q)
                .await
                .0,
            StatusCode::CONFLICT
        );
    }
    for field in ["project_id", "mission_id", "agent_id"] {
        let mut q = body("shared");
        q["scope"][field] = "other".into();
        assert_eq!(
            request(&router, "POST", "/api/v1/memory/changes", &token, q)
                .await
                .0,
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/memory/changes",
            &admin.secret,
            body("private")
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let who = identity.authenticate(&token).unwrap();
    identity
        .revoke(&admin.secret, who.id, who.revision)
        .unwrap();
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/memory/changes",
            &token,
            body("private")
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
}
