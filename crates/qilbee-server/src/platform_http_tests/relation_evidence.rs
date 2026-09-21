//! Additional inference context is checked through real HTTP and served schemas.
use super::*;
const GRAPH: &str = "/api/v1/memory/graph/typed";
const SEARCH: &str = "/api/v1/memory/search/graph";
const COMPANY_GRAPH: &str = "/api/v1/company/memory/graph/typed";
const COMPANY_INSPECT: &str = "/api/v1/company/memory/relations/inspect";

async fn create(http: &Client, token: &str, key: &str, text: &str) -> Value {
    http.call(
        "POST",
        MEMORY,
        MEMORY,
        token,
        memory_create(key, text, "private"),
        200,
    )
    .await["receipt"]
        .clone()
}
fn context_command(a: &Value, b: &Value, sources: Value) -> Value {
    let mut command = assertion(a, b, "private");
    command["operation"]["relation"]["kind"] = "same_entity".into();
    command["operation"]["relation"]["evidence_sources"] = sources;
    command
}
fn source(record: &Value) -> Value {
    json!({"record_id":record["record_id"],"revision":record["revision"]})
}

#[tokio::test]
async fn evidence_context_real_http_invalidates_lookup_company_traversal_and_search() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("company", "owner").unwrap();
    let writer = key(&identity, &admin.secret, "alice", true, true);
    let server = Server::start(router).await;
    let http = &server.http;
    let a = create(http, &writer, "a", "anchor").await;
    let b = create(http, &writer, "b", "neighbor").await;
    let c = create(http, &writer, "c", "context").await;
    let command = context_command(&a, &b, json!([source(&c)]));
    // Validate the new request property against the schema actually served over TCP.
    let schema = json!({"allOf":[http.api["paths"][COMMAND]["post"]["requestBody"]["content"]["application/json"]["schema"]],"components":http.api["components"]});
    jsonschema::draft202012::options()
        .build(&schema)
        .unwrap()
        .validate(&command)
        .unwrap();
    let receipt = http
        .call("POST", COMMAND, COMMAND, &writer, command.clone(), 200)
        .await;
    let id = &receipt["receipt"]["relation_id"];
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
    let workspace = &workspaces["page"]["workspaces"][0]["workspace_id"];
    let graph = json!({"contract_version":1,"scope":memory_scope("private"),"query":{"root_record_ids":[a["record_id"]]}});
    let company_graph =
        json!({"contract_version":1,"workspace_id":workspace,"query":graph["query"]});
    let company_inspect = json!({"contract_version":1,"workspace_id":workspace,"relation_id":id});
    let search = json!({"contract_version":1,"scope":memory_scope("private"),"query":{"limit":10,"ranking_version":"typed_path_balanced_v1","seed":{"mode":"lexical","text":"anchor"}}});
    for (route, body, token) in [
        (GRAPH, graph.clone(), writer.as_str()),
        (COMPANY_GRAPH, company_graph.clone(), admin.secret.as_str()),
    ] {
        let v = http.call("POST", route, route, token, body, 200).await;
        assert_eq!(v["graph"]["edges"].as_array().unwrap().len(), 1);
        assert_eq!(v["graph"]["nodes"].as_array().unwrap().len(), 2);
        assert_eq!(
            v["graph"]["coverage"]["dependency_work"]["records_examined"],
            1
        );
        assert_eq!(
            v["graph"]["edges"][0]["input"]["evidence_sources"],
            json!([source(&c)])
        );
    }
    assert_eq!(
        http.call("POST", SEARCH, SEARCH, &writer, search.clone(), 200)
            .await["page"]["hits"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    http.call("POST", READ, READ, &writer, read(id, "private"), 200)
        .await;
    let mut correction = memory_create("context-correction", "Updated context", "private");
    correction["operation"]["type"] = "update".into();
    correction["operation"]["record_id"] = c["record_id"].clone();
    correction["operation"]["expected_revision"] = 1.into();
    http.call("POST", MEMORY, MEMORY, &writer, correction, 200)
        .await;
    http.call("POST", READ, READ, &writer, read(id, "private"), 404)
        .await;
    for (route, body, token) in [
        (INSPECT, read(id, "private"), writer.as_str()),
        (COMPANY_INSPECT, company_inspect, admin.secret.as_str()),
    ] {
        let v = http.call("POST", route, route, token, body, 200).await;
        let e = &v["inspection"]["eligibility"];
        assert_eq!(e["reason"], "evidence_unavailable");
        assert_eq!(
            e["evidence_failure"],
            json!({"record_id":c["record_id"],"expected_revision":1,"actual_revision":2,"reason":"source_revision_changed"})
        );
        assert_eq!(v["inspection"]["relation"]["revision"], 1);
    }
    for (route, body, token) in [
        (GRAPH, graph, writer.as_str()),
        (COMPANY_GRAPH, company_graph, admin.secret.as_str()),
    ] {
        let v = http.call("POST", route, route, token, body, 200).await;
        assert!(v["graph"]["edges"].as_array().unwrap().is_empty());
        assert_eq!(v["graph"]["nodes"].as_array().unwrap().len(), 1);
        assert_eq!(v["graph"]["coverage"]["complete"], true);
    }
    let v = http
        .call("POST", SEARCH, SEARCH, &writer, search, 200)
        .await;
    assert_eq!(v["page"]["hits"].as_array().unwrap().len(), 1);
    assert!(v["page"]["relations"].as_array().unwrap().is_empty());
    let mut historical = read(id, "private");
    historical["revision"] = 1.into();
    let history = http
        .call("POST", HISTORY, HISTORY, &writer, historical, 200)
        .await;
    assert_eq!(history["history"]["receipt"], receipt["receipt"]);
    assert_eq!(
        http.call("POST", COMMAND, COMMAND, &writer, command.clone(), 200)
            .await,
        receipt
    );
    let mut late = command;
    late["idempotency_key"] = "late-worker".into();
    http.call("POST", COMMAND, COMMAND, &writer, late, 409)
        .await;
    http.call(
        "POST",
        COMMAND,
        COMMAND,
        &writer,
        change(id, 1, "retire", "private"),
        200,
    )
    .await;
    http.call(
        "POST",
        COMMAND,
        COMMAND,
        &writer,
        change(id, 2, "restore", "private"),
        409,
    )
    .await;
}

#[tokio::test]
async fn evidence_context_real_http_cannot_import_another_subject_company_or_scope() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("company", "owner").unwrap();
    let foreign = identity.bootstrap_tenant("other", "owner").unwrap();
    let alice = key(&identity, &admin.secret, "alice", true, true);
    let bob = key(&identity, &admin.secret, "bob", true, true);
    let other = key(&identity, &foreign.secret, "alice", true, true);
    let server = Server::start(router).await;
    let http = &server.http;
    let a = create(http, &alice, "a", "anchor").await;
    let b = create(http, &alice, "b", "neighbor").await;
    let c = create(http, &alice, "c", "context").await;
    let mut foreign_sources = vec![
        create(http, &bob, "c", "context").await,
        create(http, &other, "c", "context").await,
    ];
    for field in ["project_id", "agent_id", "mission_id", "visibility"] {
        let mut definition = spec();
        definition.subject_id = "alice".into();
        definition.capabilities = [Capability::MemoryRead, Capability::MemoryWrite].into();
        let mut scope = memory_scope("private");
        scope[field] = if field == "visibility" {
            "shared"
        } else {
            "different"
        }
        .into();
        definition.grants = vec![serde_json::from_value(scope.clone()).unwrap()];
        let scoped = identity.issue(&admin.secret, definition).unwrap();
        let mut request = memory_create(&format!("foreign-{field}"), "context", "private");
        request["scope"] = scope;
        foreign_sources.push(
            http.call("POST", MEMORY, MEMORY, &scoped.secret, request, 200)
                .await["receipt"]
                .clone(),
        );
    }
    let mut missing = source(&c);
    missing["record_id"] = uuid::Uuid::new_v4().to_string().into();
    let missing_error = http
        .call(
            "POST",
            COMMAND,
            COMMAND,
            &alice,
            context_command(&a, &b, json!([missing])),
            409,
        )
        .await;
    for foreign in foreign_sources {
        assert_eq!(
            http.call(
                "POST",
                COMMAND,
                COMMAND,
                &alice,
                context_command(&a, &b, json!([source(&foreign)])),
                409
            )
            .await,
            missing_error
        );
    }
    for invalid in [
        json!([source(&a)]),
        json!([source(&b)]),
        json!([source(&c), source(&c)]),
        json!([{"record_id":c["record_id"],"revision":0}]),
        Value::Null,
    ] {
        http.call(
            "POST",
            COMMAND,
            COMMAND,
            &alice,
            context_command(&a, &b, invalid),
            400,
        )
        .await;
    }
    let ok = context_command(&a, &b, json!([source(&c)]));
    http.call("POST", COMMAND, COMMAND, &alice, ok.clone(), 200)
        .await;
    let credential = identity.authenticate(&alice).unwrap().id;
    identity.revoke(&admin.secret, credential, 1).unwrap();
    http.call("POST", COMMAND, COMMAND, &alice, ok, 401).await;
}
