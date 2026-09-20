use super::*;
#[tokio::test]
async fn derived_memory_requires_read_and_write_and_preserves_scope_on_source_validation() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("tenant", "operator").unwrap();
    let foreign = identity.bootstrap_tenant("foreign", "operator").unwrap();
    let writer = memory_key(&identity, &admin.secret, "owner", true);
    let reader = memory_key(&identity, &admin.secret, "reader", false);
    let outside = memory_key(&identity, &foreign.secret, "owner", true);
    let other = memory_key(&identity, &admin.secret, "other", true);
    let mut s = spec();
    s.subject_id = "write-only".into();
    s.capabilities = [Capability::MemoryWrite].into();
    s.grants = vec![serde_json::from_value(memory_scope("shared")).unwrap()];
    let write_only = identity.issue(&admin.secret, s).unwrap().secret;
    for visibility in ["shared", "private"] {
        let origin = request(
            &router,
            "POST",
            "/api/v1/memory/commands",
            &writer,
            memory_create(visibility, "origin", visibility),
        )
        .await;
        assert_eq!(origin.0, StatusCode::OK);
        let id = origin.1["receipt"]["record_id"].clone();
        let mut derive = memory_create(&format!("derived-{visibility}"), "synthesis", visibility);
        derive["operation"]["type"] = "derive".into();
        derive["operation"]["derivation"] = json!({"sources":[{"record_id":id,"revision":1}],"method":"summarizer","method_revision":"v1","evidence_ref":"trace://run"});
        for denied in [&reader, &write_only] {
            assert_eq!(
                request(
                    &router,
                    "POST",
                    "/api/v1/memory/commands",
                    denied,
                    derive.clone()
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
                "/api/v1/memory/commands",
                &outside,
                derive.clone()
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
                    "/api/v1/memory/commands",
                    &other,
                    derive.clone()
                )
                .await
                .0,
                StatusCode::NOT_FOUND
            );
        }
        let accepted = request(
            &router,
            "POST",
            "/api/v1/memory/commands",
            &writer,
            derive.clone(),
        )
        .await;
        assert_eq!(accepted.0, StatusCode::OK);
        assert_eq!(accepted.1["receipt"]["action"], "derived");
        let derived_id = accepted.1["receipt"]["record_id"].as_str().unwrap();
        let read = request(
            &router,
            "GET",
            &record_url(derived_id, visibility),
            &writer,
            Value::Null,
        )
        .await;
        assert_eq!(
            read.1["record"]["derivation"]["sources"][0]["record_id"],
            id
        );
        let delete = json!({"contract_version":1,"scope":memory_scope(visibility),"idempotency_key":format!("delete-{visibility}"),"operation":{"type":"delete","record_id":id,"expected_revision":1}});
        assert_eq!(
            request(&router, "POST", "/api/v1/memory/commands", &writer, delete)
                .await
                .0,
            StatusCode::OK
        );
        assert_eq!(
            request(
                &router,
                "GET",
                &record_url(derived_id, visibility),
                &writer,
                Value::Null
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            request(&router, "POST", "/api/v1/memory/commands", &writer, derive).await,
            accepted
        );
    }
}
