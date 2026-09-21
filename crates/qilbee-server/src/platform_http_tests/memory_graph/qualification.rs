use super::*;

async fn derive(
    http: &Client,
    key: &str,
    idempotency: &str,
    source: &Value,
    visibility: &str,
) -> Value {
    let mut command = memory_create(idempotency, "Derived context", visibility);
    command["operation"]["type"] = "derive".into();
    command["operation"]["derivation"] = json!({"sources":[{"record_id":source["record_id"],"revision":source["revision"]}],"method":"fixture-extractor","method_revision":"v1","evidence_ref":"trace://fixture"});
    http.call(
        "POST",
        "/api/v1/memory/commands",
        "/api/v1/memory/commands",
        key,
        command,
        200,
    )
    .await["receipt"]
        .clone()
}

#[tokio::test]
async fn evidence_graph_http_preserves_scope_source_versions_coverage_and_strict_schemas() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("company", "owner").unwrap();
    let foreign = identity.bootstrap_tenant("foreign", "owner").unwrap();
    let alice = memory_key(&identity, &admin.secret, "alice", true);
    let bob = memory_key(&identity, &admin.secret, "bob", true);
    let other = memory_key(&identity, &foreign.secret, "alice", true);
    let server = Server::start(router).await;
    let http = &server.http;
    let commands = "/api/v1/memory/commands";
    let a = http
        .call(
            "POST",
            commands,
            commands,
            &alice,
            memory_create("source", "Identical content", "private"),
            200,
        )
        .await["receipt"]
        .clone();
    let b = http
        .call(
            "POST",
            commands,
            commands,
            &bob,
            memory_create("source", "Identical content", "private"),
            200,
        )
        .await["receipt"]
        .clone();
    let f = http
        .call(
            "POST",
            commands,
            commands,
            &other,
            memory_create("source", "Identical content", "private"),
            200,
        )
        .await["receipt"]
        .clone();
    let derived = derive(http, &alice, "derived", &a, "private").await;
    let request = scoped(
        json!([derived["record_id"], b["record_id"], f["record_id"]]),
        "private",
    );
    let result = http
        .call("POST", SCOPED, SCOPED, &alice, request.clone(), 200)
        .await;
    let graph = &result["graph"];
    assert_eq!(graph["nodes"].as_array().unwrap().len(), 2);
    assert_eq!(
        graph["edges"][0]["source"]["record_id"],
        derived["record_id"]
    );
    assert_eq!(graph["edges"][0]["target"]["record_id"], a["record_id"]);
    assert_eq!(graph["edges"][0]["target"]["revision"], 1);
    assert_eq!(graph["roots"][1]["status"], "unavailable");
    assert_eq!(graph["roots"][2]["status"], "unavailable");
    assert_eq!(graph["coverage"]["complete"], true);
    for key in [&bob, &other] {
        let hidden = http
            .call(
                "POST",
                SCOPED,
                SCOPED,
                key,
                scoped(json!([derived["record_id"], a["record_id"]]), "private"),
                200,
            )
            .await;
        assert_eq!(hidden["graph"]["nodes"], json!([]));
    }
    let shared = http
        .call(
            "POST",
            SCOPED,
            SCOPED,
            &alice,
            scoped(json!([derived["record_id"]]), "shared"),
            200,
        )
        .await;
    assert_eq!(shared["graph"]["nodes"], json!([]));
    for field in ["project_id", "agent_id", "mission_id"] {
        let mut outside = request.clone();
        outside["scope"][field] = "outside".into();
        http.call("POST", SCOPED, SCOPED, &alice, outside.clone(), 403)
            .await;
        let mut definition = spec();
        definition.subject_id = "alice".into();
        definition.capabilities = [Capability::MemoryRead, Capability::MemoryWrite].into();
        definition.grants = vec![serde_json::from_value(outside["scope"].clone()).unwrap()];
        let outside_key = identity.issue(&admin.secret, definition).unwrap().secret;
        let mut create = memory_create("source", "Identical content", "private");
        create["scope"] = outside["scope"].clone();
        let own = http
            .call("POST", commands, commands, &outside_key, create, 200)
            .await;
        outside["query"]["root_record_ids"] =
            json!([derived["record_id"], own["receipt"]["record_id"]]);
        let isolated = http
            .call("POST", SCOPED, SCOPED, &outside_key, outside, 200)
            .await;
        assert_eq!(isolated["graph"]["roots"][0]["status"], "unavailable");
        assert_eq!(isolated["graph"]["nodes"].as_array().unwrap().len(), 1);
        assert_eq!(
            isolated["graph"]["nodes"][0]["record"]["record_id"],
            own["receipt"]["record_id"]
        );
    }
    let directory = "/api/v1/company/memory/workspaces";
    let workspaces = http
        .call(
            "GET",
            &format!("{directory}?contract_version=1"),
            directory,
            &admin.secret,
            Value::Null,
            200,
        )
        .await;
    for (subject, root, count) in [
        ("alice", derived["record_id"].clone(), 2),
        ("bob", b["record_id"].clone(), 1),
    ] {
        let workspace = workspaces["page"]["workspaces"]
            .as_array()
            .unwrap()
            .iter()
            .find(|w| w["private_subject_id"] == subject && w["scope"] == memory_scope("private"))
            .unwrap();
        let admin_graph = http.call("POST", COMPANY, COMPANY, &admin.secret, json!({"contract_version":1,"workspace_id":workspace["workspace_id"],"query":{"root_record_ids":[root]}}), 200).await;
        assert_eq!(
            admin_graph["graph"]["nodes"].as_array().unwrap().len(),
            count
        );
    }
    let mut cut = request.clone();
    cut["query"]["max_depth"] = 0.into();
    let shallow = http.call("POST", SCOPED, SCOPED, &alice, cut, 200).await;
    assert_eq!(
        shallow["graph"]["coverage"]["stop_reasons"],
        json!(["depth_limit"])
    );
    assert_eq!(shallow["graph"]["coverage"]["complete"], false);
    let mut cut = request.clone();
    cut["query"]["node_limit"] = 1.into();
    let limited = http.call("POST", SCOPED, SCOPED, &alice, cut, 200).await;
    assert_eq!(limited["graph"]["roots"][1]["status"], "not_examined");
    assert_eq!(
        limited["graph"]["coverage"]["stop_reasons"],
        json!(["node_limit"])
    );
    let schema = json!({"allOf":[http.api["paths"][SCOPED]["post"]["responses"]["200"]["content"]["application/json"]["schema"]],"components":http.api["components"]});
    let validator = jsonschema::draft202012::options().build(&schema).unwrap();
    for (pointer, value) in [
        ("/graph/coverage/record_bytes", json!(8_388_609)),
        ("/graph/edges/0/relation", json!("causes")),
        ("/graph/edges/0/target/revision", json!(0)),
        ("/graph/nodes/0/record/payload", Value::Null),
        ("/graph/coverage/stop_reasons", json!(["node_limit"])),
    ] {
        let mut bad = result.clone();
        *bad.pointer_mut(pointer).unwrap() = value;
        assert!(!validator.is_valid(&bad), "{pointer}");
    }
    let mut update = memory_create("source-edit", "Corrected content", "private");
    update["operation"]["type"] = "update".into();
    update["operation"]["record_id"] = a["record_id"].clone();
    update["operation"]["expected_revision"] = 1.into();
    http.call("POST", commands, commands, &alice, update, 200)
        .await;
    let current = http
        .call("POST", SCOPED, SCOPED, &alice, request.clone(), 200)
        .await;
    assert_eq!(current["graph"]["nodes"], json!([]));
    assert_eq!(current["graph"]["roots"][0]["status"], "unavailable");
    let credential = identity.authenticate(&alice).unwrap();
    identity
        .revoke(&admin.secret, credential.id, credential.revision)
        .unwrap();
    http.call("POST", SCOPED, SCOPED, &alice, request, 401)
        .await;
}

#[tokio::test]
async fn evidence_graph_http_rejects_invalid_bodies_and_company_authority_overrides() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("company", "owner").unwrap();
    let foreign = identity.bootstrap_tenant("foreign", "owner").unwrap();
    let key = memory_key(&identity, &admin.secret, "alice", true);
    let server = Server::start(router).await;
    let http = &server.http;
    let commands = "/api/v1/memory/commands";
    let source = http
        .call(
            "POST",
            commands,
            commands,
            &key,
            memory_create("source", "Company source", "private"),
            200,
        )
        .await["receipt"]
        .clone();
    let derived = derive(http, &key, "derived", &source, "private").await;
    let valid = scoped(json!([derived["record_id"]]), "private");
    let mut malformed = vec![];
    for ids in [
        json!([]),
        json!([Uuid::nil(), Uuid::nil()]),
        json!(["not-a-uuid"]),
        json!((0..17).map(|_| Uuid::new_v4()).collect::<Vec<_>>()),
    ] {
        let mut bad = valid.clone();
        bad["query"]["root_record_ids"] = ids;
        malformed.push(bad);
    }
    for (field, value) in [
        ("max_depth", json!(9)),
        ("max_depth", json!(-1)),
        ("node_limit", json!(0)),
        ("node_limit", json!(257)),
        ("ranking_version", json!("new")),
    ] {
        let mut bad = valid.clone();
        bad["query"][field] = value;
        malformed.push(bad);
    }
    for (field, value) in [
        ("contract_version", json!(2)),
        ("company_id", json!("foreign")),
        ("private_subject_id", json!("bob")),
    ] {
        let mut bad = valid.clone();
        bad[field] = value;
        malformed.push(bad);
    }
    for bad in malformed {
        http.call("POST", SCOPED, SCOPED, &key, bad, 400).await;
    }
    http.call("POST", SCOPED, SCOPED, "", valid.clone(), 401)
        .await;
    let mut oversized = valid.clone();
    oversized["padding"] = json!("x".repeat(65_536));
    http.call("POST", SCOPED, SCOPED, &key, oversized, 413)
        .await;
    let directory = "/api/v1/company/memory/workspaces";
    let workspaces = http
        .call(
            "GET",
            &format!("{directory}?contract_version=1"),
            directory,
            &admin.secret,
            Value::Null,
            200,
        )
        .await;
    let body = json!({"contract_version":1,"workspace_id":workspaces["page"]["workspaces"][0]["workspace_id"],"query":valid["query"]});
    let credentials = "/api/v1/credentials";
    let before = http
        .call(
            "GET",
            &format!("{credentials}?contract_version=1&limit=100"),
            credentials,
            &admin.secret,
            Value::Null,
            200,
        )
        .await;
    let company = http
        .call("POST", COMPANY, COMPANY, &admin.secret, body.clone(), 200)
        .await;
    assert_eq!(company["graph"]["nodes"].as_array().unwrap().len(), 2);
    assert_eq!(company["workspace"]["private_subject_id"], "alice");
    http.call("POST", COMPANY, COMPANY, &key, body.clone(), 403)
        .await;
    let hidden = http
        .call("POST", COMPANY, COMPANY, &foreign.secret, body.clone(), 404)
        .await;
    let mut missing = body.clone();
    missing["workspace_id"] = json!("0".repeat(64));
    assert_eq!(
        http.call("POST", COMPANY, COMPANY, &foreign.secret, missing, 404)
            .await,
        hidden
    );
    for field in ["company_id", "private_subject_id"] {
        let mut bad = body.clone();
        bad[field] = json!("foreign");
        http.call("POST", COMPANY, COMPANY, &admin.secret, bad, 400)
            .await;
    }
    let mut bad = body.clone();
    bad["workspace_id"] = json!("invalid");
    http.call("POST", COMPANY, COMPANY, &admin.secret, bad, 400)
        .await;
    assert_eq!(
        http.call(
            "GET",
            &format!("{credentials}?contract_version=1&limit=100"),
            credentials,
            &admin.secret,
            Value::Null,
            200
        )
        .await,
        before
    );
    let principal = identity.authenticate(&admin.secret).unwrap();
    identity
        .revoke(&admin.secret, principal.id, principal.revision)
        .unwrap();
    http.call("POST", COMPANY, COMPANY, &admin.secret, body, 401)
        .await;
}
