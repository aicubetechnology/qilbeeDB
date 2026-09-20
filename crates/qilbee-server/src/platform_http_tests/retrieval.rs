use super::*;
fn lexical() -> Value {
    json!({"contract_version":1,"scope":memory_scope("shared"),"mode":"lexical","query":{"text":"ZX17","limit":10}})
}
fn hybrid() -> Value {
    let mut body = lexical();
    body["mode"] = "hybrid".into();
    body["query"]["ranking_version"] = "weighted_rrf_v1".into();
    body["query"]["space"] =
        json!({"provider":"fixture","model":"fixture-embedding","revision":"v1","dimensions":3});
    body["query"]["vector"] = json!([1.0, 0.0, 0.0]);
    body
}
#[tokio::test]
async fn retrieval_http_preserves_scope_and_evidence_across_restart() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("tenant", "admin").unwrap();
    let writer = memory_key(&identity, &admin.secret, "writer", true);
    let reader = memory_key(&identity, &admin.secret, "reader", false);
    let foreign_admin = identity.bootstrap_tenant("foreign", "admin").unwrap();
    let foreign = memory_key(&identity, &foreign_admin.secret, "writer", true);
    let (_, created) = request(
        &router,
        "POST",
        "/api/v1/memory/commands",
        &writer,
        memory_create("source", "ZX17", "shared"),
    )
    .await;
    let id = created["receipt"]["record_id"].clone();
    let embedding = json!({"contract_version":1,"scope":memory_scope("shared"),"idempotency_key":"vector","record_id":id,"record_revision":1,"space":hybrid()["query"]["space"],"vector":[1.0,0.0,0.0]});
    let (_, receipt) = request(
        &router,
        "POST",
        "/api/v1/memory/embeddings",
        &writer,
        embedding,
    )
    .await;
    request(
        &router,
        "POST",
        "/api/v1/memory/commands",
        &writer,
        memory_create("private", "ZX17", "private"),
    )
    .await;
    for (path, body) in [
        ("/api/v1/memory/search/lexical", lexical()),
        ("/api/v1/memory/search/hybrid", hybrid()),
    ] {
        let (status, result) = request(&router, "POST", path, &reader, body.clone()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(result["page"]["hits"][0]["record"]["record_id"], id);
        assert_eq!(result["page"]["corpus_records"], 1);
        assert_eq!(
            request(&router, "POST", path, &foreign, body.clone())
                .await
                .1["page"]["hits"],
            json!([])
        );
        assert_eq!(
            request(&router, "POST", path, &admin.secret, body.clone())
                .await
                .0,
            StatusCode::FORBIDDEN
        );
        let mut private = body.clone();
        private["scope"]["visibility"] = "private".into();
        assert_eq!(
            request(&router, "POST", path, &reader, private).await.1["page"]["hits"],
            json!([])
        );
        let mut denied = body;
        denied["scope"]["mission_id"] = "ungranted".into();
        assert_eq!(
            request(&router, "POST", path, &reader, denied).await.0,
            StatusCode::FORBIDDEN
        );
    }
    drop(router);
    drop(identity);
    let (router, _) = app(dir.path());
    let (status, result) = request(
        &router,
        "POST",
        "/api/v1/memory/search/hybrid",
        &reader,
        hybrid(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(result["page"]["hits"][0]["embedding"], receipt["receipt"]);
    assert_eq!(result["page"]["hits"][0]["semantic"]["rank"], 1);
    assert_eq!(
        result["page"]["hits"][0]["score"].as_f64().unwrap(),
        1.0 / 61.0
    );
}
#[tokio::test]
async fn retrieval_http_rejects_invalid_contracts_and_documents_both_routes() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("tenant", "admin").unwrap();
    let reader = memory_key(&identity, &admin.secret, "reader", false);
    for (path, body) in [
        ("/api/v1/memory/search/lexical", lexical()),
        ("/api/v1/memory/search/hybrid", hybrid()),
    ] {
        assert_eq!(
            request(&router, "POST", path, "", body.clone()).await.0,
            StatusCode::UNAUTHORIZED
        );
        for (field, value) in [
            ("tenant", json!("foreign")),
            ("text", json!("!!!")),
            ("scan_limit", json!(10001)),
            ("limit", json!(0)),
        ] {
            let mut invalid = body.clone();
            invalid["query"][field] = value;
            assert_eq!(
                request(&router, "POST", path, &reader, invalid).await.0,
                StatusCode::BAD_REQUEST
            );
        }
        let mut invalid = body.clone();
        invalid["contract_version"] = 2.into();
        assert_eq!(
            request(&router, "POST", path, &reader, invalid).await.0,
            StatusCode::BAD_REQUEST
        );
    }
    for (field, value) in [
        ("vector", json!([0, 0, 0])),
        ("semantic_weight", json!(1.1)),
        ("candidate_limit", json!(2)),
    ] {
        let mut invalid = hybrid();
        invalid["query"][field] = value;
        assert_eq!(
            request(
                &router,
                "POST",
                "/api/v1/memory/search/hybrid",
                &reader,
                invalid
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
    }
    let (_, spec) = request(&router, "GET", "/openapi.json", "", Value::Null).await;
    for (name, path) in [
        ("Lexical", "/api/v1/memory/search/lexical"),
        ("Hybrid", "/api/v1/memory/search/hybrid"),
    ] {
        assert!(spec["paths"].get(path).is_some());
        assert!(
            spec["components"]["schemas"]
                .get(format!("{name}Query"))
                .is_some()
        );
    }
}

#[tokio::test]
async fn retrieval_http_identical_text_and_vectors_are_isolated_before_ranking() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("tenant", "admin").unwrap();
    let foreign = identity.bootstrap_tenant("other-tenant", "admin").unwrap();
    let mut fixtures = vec![];
    for (index, (project, agent, mission, visibility, subject, is_foreign)) in [
        ("project", "agent", None, "shared", "one", false),
        ("other-project", "agent", None, "shared", "one", false),
        ("project", "other-agent", None, "shared", "one", false),
        (
            "project",
            "agent",
            Some("mission-one"),
            "shared",
            "one",
            false,
        ),
        (
            "project",
            "agent",
            Some("mission-two"),
            "shared",
            "one",
            false,
        ),
        ("project", "agent", None, "private", "one", false),
        ("project", "agent", None, "private", "two", false),
        ("project", "agent", None, "shared", "one", true),
    ]
    .into_iter()
    .enumerate()
    {
        let scope = json!({"project_id":project,"agent_id":agent,"mission_id":mission,"visibility":visibility});
        let mut definition = spec();
        definition.subject_id = subject.into();
        definition.capabilities = [Capability::MemoryRead, Capability::MemoryWrite].into();
        definition.grants = vec![serde_json::from_value(scope.clone()).unwrap()];
        let issued = identity
            .issue(
                if is_foreign {
                    &foreign.secret
                } else {
                    &admin.secret
                },
                definition,
            )
            .unwrap();
        let mut create = memory_create(
            &format!("source-{index}"),
            "ZX17 identical text",
            visibility,
        );
        create["scope"] = scope.clone();
        let (status, receipt) = request(
            &router,
            "POST",
            "/api/v1/memory/commands",
            &issued.secret,
            create,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let id = receipt["receipt"]["record_id"].clone();
        let embedding = json!({"contract_version":1,"scope":scope,"idempotency_key":"vector","record_id":id,"record_revision":1,"space":hybrid()["query"]["space"],"vector":[1.0,0.0,0.0]});
        assert_eq!(
            request(
                &router,
                "POST",
                "/api/v1/memory/embeddings",
                &issued.secret,
                embedding
            )
            .await
            .0,
            StatusCode::OK
        );
        fixtures.push((issued, scope, id, is_foreign));
    }
    for (issued, scope, id, is_foreign) in &fixtures {
        for (path, mut body) in [
            ("/api/v1/memory/search/lexical", lexical()),
            ("/api/v1/memory/search/hybrid", hybrid()),
        ] {
            body["scope"] = scope.clone();
            let (status, response) =
                request(&router, "POST", path, &issued.secret, body.clone()).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(response["page"]["hits"].as_array().unwrap().len(), 1);
            assert_eq!(response["page"]["hits"][0]["record"]["record_id"], *id);
            assert_eq!(response["page"]["corpus_records"], 1);
            if path.ends_with("hybrid") {
                assert_eq!(response["page"]["embedding_coverage"], "complete");
                assert_eq!(
                    response["page"]["hits"][0]["lexical"]["score"]
                        .as_f64()
                        .unwrap(),
                    (4.0_f64 / 3.0).ln()
                );
            } else {
                assert_eq!(
                    response["page"]["hits"][0]["score"].as_f64().unwrap(),
                    (4.0_f64 / 3.0).ln()
                );
            }
        }
        identity
            .revoke(
                if *is_foreign {
                    &foreign.secret
                } else {
                    &admin.secret
                },
                issued.credential.id,
                1,
            )
            .unwrap();
        for (path, mut body) in [
            ("/api/v1/memory/search/lexical", lexical()),
            ("/api/v1/memory/search/hybrid", hybrid()),
        ] {
            body["scope"] = scope.clone();
            assert_eq!(
                request(&router, "POST", path, &issued.secret, body).await.0,
                StatusCode::UNAUTHORIZED
            );
        }
    }
}

#[tokio::test]
async fn ranking_catalog_requires_live_read_authority_and_matches_execution() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("tenant", "admin").unwrap();
    let reader = memory_key(&identity, &admin.secret, "reader", false);
    let path = "/api/v1/memory/ranking-profiles";
    for (token, expected) in [
        ("", StatusCode::UNAUTHORIZED),
        (admin.secret.as_str(), StatusCode::FORBIDDEN),
    ] {
        assert_eq!(
            request(&router, "GET", path, token, Value::Null).await.0,
            expected
        );
    }
    let (status, catalog) = request(&router, "GET", path, &reader, Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(catalog["contract_version"], 1);
    assert_eq!(
        catalog["component_versions"],
        json!({"lexical":"bm25_v1","semantic":"cosine_exact_v1"})
    );
    assert_eq!(catalog["hybrid_profiles"].as_array().unwrap().len(), 2);
    for profile in catalog["hybrid_profiles"].as_array().unwrap() {
        let mut query = hybrid();
        query["query"]["ranking_version"] = profile["version"].clone();
        let (status, result) = request(
            &router,
            "POST",
            "/api/v1/memory/search/hybrid",
            &reader,
            query,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(*profile, result["page"]["ranking"]);
        assert_eq!(result["ranking_version"], profile["version"]);
        assert_eq!(profile["experimental"], true);
    }
    let principal = identity.authenticate(&reader).unwrap();
    identity
        .revoke(&admin.secret, principal.id, principal.revision)
        .unwrap();
    assert_eq!(
        request(&router, "GET", path, &reader, Value::Null).await.0,
        StatusCode::UNAUTHORIZED
    );
    let (_, spec) = request(&router, "GET", "/openapi.json", "", Value::Null).await;
    assert!(spec["paths"].get(path).is_some());
}
