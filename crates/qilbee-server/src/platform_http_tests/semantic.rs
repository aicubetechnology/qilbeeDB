use super::*;
fn space() -> Value {
    json!({"provider":"fixture","model":"fixture-embedding","revision":"v1","dimensions":3})
}
fn embedding(id: &str) -> Value {
    json!({"contract_version":1,"scope":memory_scope("shared"),"idempotency_key":"embedding-v1","record_id":id,"record_revision":1,"space":space(),"vector":[1.0,0.0,0.0]})
}
fn query() -> Value {
    json!({"contract_version":1,"scope":memory_scope("shared"),"query":{"space":space(),"vector":[1.0,0.0,0.0],"limit":10}})
}
#[tokio::test]
async fn semantic_http_ranks_model_bound_embeddings_and_replays_after_reopen() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("tenant", "admin").unwrap();
    let writer = memory_key(&identity, &admin.secret, "writer", true);
    let (_, created) = request(
        &router,
        "POST",
        "/api/v1/memory/commands",
        &writer,
        memory_create(
            "semantic-create",
            "Related meaning through an external embedding",
            "shared",
        ),
    )
    .await;
    let id = created["receipt"]["record_id"].as_str().unwrap();
    let first = request(
        &router,
        "POST",
        "/api/v1/memory/embeddings",
        &writer,
        embedding(id),
    )
    .await;
    assert_eq!(first.0, StatusCode::OK);
    let result = request(&router, "POST", "/api/v1/memory/search", &writer, query()).await;
    assert_eq!(result.0, StatusCode::OK);
    assert_eq!(result.1["page"]["hits"][0]["record"]["record_id"], id);
    assert_eq!(result.1["page"]["hits"][0]["score"], 1.0);
    assert_eq!(result.1["page"]["exhaustive"], true);
    drop(router);
    drop(identity);
    let (router, _) = app(dir.path());
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/memory/embeddings",
            &writer,
            embedding(id)
        )
        .await
        .1,
        first.1
    );
    let mut other_model = query();
    other_model["query"]["space"]["revision"] = "v2".into();
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/memory/search",
            &writer,
            other_model
        )
        .await
        .1["page"]["hits"],
        json!([])
    );
}
#[tokio::test]
async fn semantic_http_denies_scope_escalation_and_rejects_stale_vectors() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("tenant", "admin").unwrap();
    let writer = memory_key(&identity, &admin.secret, "writer", true);
    let reader = memory_key(&identity, &admin.secret, "reader", false);
    let (_, created) = request(
        &router,
        "POST",
        "/api/v1/memory/commands",
        &writer,
        memory_create("create", "content", "shared"),
    )
    .await;
    let id = created["receipt"]["record_id"].as_str().unwrap();
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/memory/embeddings",
            &reader,
            embedding(id)
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/memory/embeddings",
            &writer,
            embedding(id)
        )
        .await
        .0,
        StatusCode::OK
    );
    let foreign_admin = identity.bootstrap_tenant("foreign", "admin").unwrap();
    let foreign = memory_key(&identity, &foreign_admin.secret, "writer", true);
    assert_eq!(
        request(&router, "POST", "/api/v1/memory/search", &foreign, query())
            .await
            .1["page"]["hits"],
        json!([])
    );
    let mut unauthorized = query();
    unauthorized["scope"]["project_id"] = "ungranted".into();
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/memory/search",
            &writer,
            unauthorized
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let mut invalid = query();
    invalid["query"]["vector"] = json!([0.0, 0.0, 0.0]);
    assert_eq!(
        request(&router, "POST", "/api/v1/memory/search", &writer, invalid)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    let mut forged = query();
    forged["query"]["tenant"] = "foreign".into();
    assert_eq!(
        request(&router, "POST", "/api/v1/memory/search", &writer, forged)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    let mut update = memory_create("update", "new content", "shared");
    update["operation"]["type"] = "update".into();
    update["operation"]["record_id"] = id.into();
    update["operation"]["expected_revision"] = 1.into();
    assert_eq!(
        request(&router, "POST", "/api/v1/memory/commands", &writer, update)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        request(&router, "POST", "/api/v1/memory/search", &reader, query())
            .await
            .1["page"]["hits"],
        json!([])
    );
    let mut stale = embedding(id);
    stale["idempotency_key"] = "stale".into();
    assert_eq!(
        request(&router, "POST", "/api/v1/memory/embeddings", &writer, stale)
            .await
            .0,
        StatusCode::CONFLICT
    );
}
