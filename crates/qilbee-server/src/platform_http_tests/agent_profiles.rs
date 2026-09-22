//! Real HTTP profile commands, current reads and immutable audit without delegated keys.
use super::administration::Client;
use super::*;
const QUERY: &str = "/api/v1/company/agents/query";
const READ: &str = "/api/v1/company/agents/read";
const WRITE: &str = "/api/v1/company/agents/commands";
const HISTORY: &str = "/api/v1/company/agents/history";
fn query() -> Value {
    json!({"contract_version":1,"query":{}})
}
fn command(revision: u64, name: Value, id: &str) -> Value {
    json!({"contract_version":1,"command":{"agent_id":"agent","expected_revision":revision,"display_name":name,"idempotency_key":id}})
}
async fn call(http: &Client, path: &str, token: &str, body: Value, status: u16) -> Value {
    http.call("POST", path, path, token, body, status).await
}
#[tokio::test]
async fn agent_profile_http_preserves_external_identity_and_current_authority_with_exact_replay() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("company", "owner").unwrap();
    let other = identity.bootstrap_tenant("other", "owner").unwrap();
    let writer = memory_key(&identity, &admin.secret, "writer", true);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap();
    let api = client
        .get(format!("{base}/openapi.json"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let http = Client { client, base, api };
    let body = command(0, json!("Research assistant"), "save-1");
    call(&http, WRITE, &admin.secret, body.clone(), 404).await;
    assert!(
        call(&http, QUERY, &admin.secret, query(), 200).await["page"]["agents"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    http.call(
        "POST",
        "/api/v1/memory/commands",
        "/api/v1/memory/commands",
        &writer,
        memory_create("first", "Unchanged memory payload", "private"),
        200,
    )
    .await;
    let initial = http
        .call(
            "GET",
            "/api/v1/agents?contract_version=1",
            "/api/v1/agents",
            &admin.secret,
            Value::Null,
            200,
        )
        .await;
    let entries = call(&http, QUERY, &admin.secret, query(), 200).await;
    assert_eq!(entries["page"]["agents"][0]["agent_id"], "agent");
    assert!(entries["page"]["agents"][0]["profile"].is_null());
    for (token, status) in [(writer.as_str(), 403), ("", 401)] {
        for (path, body) in [
            (QUERY, query()),
            (READ, json!({"contract_version":1,"agent_id":"agent"})),
            (WRITE, body.clone()),
            (HISTORY, json!({"contract_version":1,"agent_id":"agent"})),
        ] {
            call(&http, path, token, body, status).await;
        }
    }
    call(&http, WRITE, &other.secret, body.clone(), 404).await;
    let saved = call(&http, WRITE, &admin.secret, body.clone(), 200).await;
    assert_eq!(saved["result"]["receipt"]["profile"]["revision"], 1);
    assert_eq!(saved["result"]["replayed"], false);
    let replay = call(&http, WRITE, &admin.secret, body.clone(), 200).await;
    assert_eq!(replay["result"]["receipt"], saved["result"]["receipt"]);
    assert_eq!(replay["result"]["replayed"], true);
    let foreign_writer = memory_key(&identity, &other.secret, "writer", true);
    http.call(
        "POST",
        "/api/v1/memory/commands",
        "/api/v1/memory/commands",
        &foreign_writer,
        memory_create("first", "Foreign memory", "private"),
        200,
    )
    .await;
    call(
        &http,
        WRITE,
        &other.secret,
        command(0, json!("Foreign analyst"), "save-1"),
        200,
    )
    .await;
    let foreign = call(
        &http,
        READ,
        &other.secret,
        json!({"contract_version":1,"agent_id":"agent"}),
        200,
    )
    .await;
    assert_eq!(
        foreign["agent"]["profile"]["display_name"],
        "Foreign analyst"
    );
    let local = call(
        &http,
        READ,
        &admin.secret,
        json!({"contract_version":1,"agent_id":"agent"}),
        200,
    )
    .await;
    assert_eq!(
        local["agent"]["profile"]["display_name"],
        "Research assistant"
    );
    let old = http
        .call(
            "GET",
            "/api/v1/agents?contract_version=1",
            "/api/v1/agents",
            &admin.secret,
            Value::Null,
            200,
        )
        .await;
    assert_eq!(initial, old);
    let conflict = call(
        &http,
        WRITE,
        &admin.secret,
        command(0, json!("Other"), "save-1"),
        409,
    )
    .await;
    assert_eq!(conflict["error"]["code"], "idempotency_conflict");
    let conflict = call(
        &http,
        WRITE,
        &admin.secret,
        command(0, json!("Other"), "save-2"),
        409,
    )
    .await;
    assert_eq!(conflict["error"]["code"], "revision_conflict");
    let mut missing = command(1, Value::Null, "clear");
    missing["command"]
        .as_object_mut()
        .unwrap()
        .remove("display_name");
    call(&http, WRITE, &admin.secret, missing, 400).await;
    call(
        &http,
        WRITE,
        &admin.secret,
        command(1, json!(" renamed "), "space"),
        400,
    )
    .await;
    let cleared = call(
        &http,
        WRITE,
        &admin.secret,
        command(1, Value::Null, "clear"),
        200,
    )
    .await;
    assert_eq!(cleared["result"]["receipt"]["profile"]["revision"], 2);
    let replay = call(&http, WRITE, &admin.secret, body, 200).await;
    assert_eq!(replay["result"]["receipt"]["profile"]["revision"], 1);
    let current = call(
        &http,
        READ,
        &admin.secret,
        json!({"contract_version":1,"agent_id":"agent"}),
        200,
    )
    .await;
    assert!(current["agent"]["profile"]["display_name"].is_null());
    assert_eq!(current["agent"]["profile"]["revision"], 2);
    let first = call(
        &http,
        HISTORY,
        &admin.secret,
        json!({"contract_version":1,"agent_id":"agent","limit":1}),
        200,
    )
    .await;
    assert_eq!(first["page"]["receipts"][0], saved["result"]["receipt"]);
    assert_eq!(first["page"]["next_after_revision"], 1);
    let last = call(
        &http,
        HISTORY,
        &admin.secret,
        json!({"contract_version":1,"agent_id":"agent","after_revision":1}),
        200,
    )
    .await;
    assert_eq!(last["page"]["receipts"].as_array().unwrap().len(), 1);
    assert!(last["page"]["next_after_revision"].is_null());
    call(
        &http,
        HISTORY,
        &admin.secret,
        json!({"contract_version":1,"agent_id":"agent","after_revision":3}),
        409,
    )
    .await;
    let mut forged = query();
    forged["company_id"] = json!("other");
    call(&http, QUERY, &admin.secret, forged, 400).await;
    for (path, base_body) in [
        (QUERY, query()),
        (READ, json!({"contract_version":1,"agent_id":"agent"})),
        (WRITE, command(2, json!("Later"), "oversize")),
        (HISTORY, json!({"contract_version":1,"agent_id":"agent"})),
    ] {
        let bytes = format!("{}{}", " ".repeat(65536), base_body);
        let response = http
            .client
            .post(format!("{}{path}", http.base))
            .bearer_auth(&admin.secret)
            .header("Content-Type", "application/json")
            .body(bytes)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), 413);
        let body: Value = response.json().await.unwrap();
        let schema = json!({"allOf":[http.api["paths"][path]["post"]["responses"]["413"]["content"]["application/json"]["schema"]],"components":http.api["components"]});
        jsonschema::validator_for(&schema)
            .unwrap()
            .validate(&body)
            .unwrap();
    }
    let current_writer = identity.authenticate(&writer).unwrap();
    identity
        .revoke(&admin.secret, current_writer.id, current_writer.revision)
        .unwrap();
    assert_eq!(
        call(&http, QUERY, &admin.secret, query(), 200).await["page"]["agents"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let revoked = identity.authenticate(&admin.secret).unwrap();
    identity
        .revoke(&admin.secret, revoked.id, revoked.revision)
        .unwrap();
    call(&http, QUERY, &admin.secret, query(), 401).await;
    call(
        &http,
        WRITE,
        &admin.secret,
        command(2, json!("Forbidden"), "revoked"),
        401,
    )
    .await;
    task.abort();
}

#[test]
fn agent_profile_guide_examples_match_the_published_request_contracts() {
    let api: Value =
        serde_json::from_str(include_str!("../../../../docs/api/openapi.json")).unwrap();
    let guide = include_str!("../../../../docs/security/agent-display-profiles.md");
    let mut count = 0;
    for block in guide.split("```json\n").skip(1) {
        let value: Value = serde_json::from_str(block.split("\n```").next().unwrap()).unwrap();
        let kind = if value.get("query").is_some() {
            "Query"
        } else if value.get("command").is_some() {
            "Command"
        } else if value.get("after_revision").is_some() {
            "History"
        } else {
            "Read"
        };
        let schema = json!({"$ref":format!("#/components/schemas/CompanyAgent{kind}Request"),"components":api["components"]});
        jsonschema::validator_for(&schema)
            .unwrap()
            .validate(&value)
            .unwrap();
        count += 1;
    }
    assert_eq!(count, 4);
}
