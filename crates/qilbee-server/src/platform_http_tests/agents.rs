use super::administration::Client;
use super::*;

fn integration(subject: &str) -> CredentialSpec {
    serde_json::from_value(json!({"subject_id":subject,"capabilities":["memory_read","memory_write"],"grants":[],"scope_policy":{
        "version":"company_scopes_v1", "projects":{"mode":"all"}, "agents":{"mode":"all"}, "missions":{"mode":"all"}, "allow_unassigned_mission":true,"visibilities":["private","shared"]
    }})).unwrap()
}
fn query(agent: &str) -> Value {
    let mut scope = memory_scope("private");
    scope["agent_id"] = agent.into();
    json!({"contract_version":1,"scope":scope,"filter":{"limit":10}})
}

#[tokio::test]
async fn agent_registration_real_http_records_successes_without_preprovisioning_or_scope_escalation()
 {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("company", "owner").unwrap();
    let other = identity.bootstrap_tenant("other", "owner").unwrap();
    let key = identity
        .issue(&admin.secret, integration("integration"))
        .unwrap();
    let other_key = identity
        .issue(&other.secret, integration("integration"))
        .unwrap();
    let second_subject = identity
        .issue(&admin.secret, integration("another-subject"))
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let base = format!("http://127.0.0.1:{port}");
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let api = client
        .get(format!("{base}/openapi.json"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let http = Client { client, base, api };
    let directory = "/api/v1/agents";
    let first_page = "/api/v1/agents?contract_version=1";
    let initial = http
        .call(
            "GET",
            first_page,
            directory,
            &admin.secret,
            Value::Null,
            200,
        )
        .await;
    assert_eq!(initial["page"]["agents"], json!([]));
    for invalid_query in [
        "contract_version=2",
        "contract_version=1&limit=0",
        "contract_version=1&limit=101",
        "contract_version=1&after_agent_id=%0A",
    ] {
        http.call(
            "GET",
            &format!("/api/v1/agents?{invalid_query}"),
            directory,
            &admin.secret,
            Value::Null,
            400,
        )
        .await;
    }
    http.call("GET", first_page, directory, &key.secret, Value::Null, 403)
        .await;
    let route = "/api/v1/memory/query";
    http.call("POST", route, route, "", query("unauthenticated"), 401)
        .await;
    let mut invalid = query("invalid-query");
    invalid["filter"]["limit"] = 0.into();
    http.call("POST", route, route, &key.secret, invalid, 400)
        .await;
    let mut ungranted = spec();
    ungranted.capabilities = [Capability::MemoryWrite].into();
    let no_read = identity.issue(&admin.secret, ungranted).unwrap();
    http.call(
        "POST",
        route,
        route,
        &no_read.secret,
        query("forbidden"),
        403,
    )
    .await;
    assert_eq!(
        http.call(
            "GET",
            first_page,
            directory,
            &admin.secret,
            Value::Null,
            200
        )
        .await,
        initial
    );

    // An empty but successful query registers the unchanged external ID.
    let external = "consumer-owned/α";
    assert_eq!(
        http.call("POST", route, route, &key.secret, query(external), 200)
            .await["page"]["records"],
        json!([])
    );
    let first = http
        .call(
            "GET",
            first_page,
            directory,
            &admin.secret,
            Value::Null,
            200,
        )
        .await;
    let agent = first["page"]["agents"][0].clone();
    assert_eq!(agent["agent_id"], external);
    assert_eq!(
        agent["registered_by"]["credential_id"],
        key.credential.id.to_string()
    );
    assert_eq!(agent["private_subject_id"], "integration");
    assert_eq!(agent["trigger"]["kind"], "successful_resource_request");
    assert!(agent.get("first_namespace").is_none());
    // Discovery metadata never substitutes for an exact grant or capability.
    let restricted = identity.issue(&admin.secret, spec()).unwrap();
    http.call(
        "POST",
        route,
        route,
        &restricted.secret,
        query(external),
        403,
    )
    .await;
    http.call("POST", route, route, &no_read.secret, query(external), 403)
        .await;
    let mut new_project = query(external);
    new_project["scope"]["project_id"] = "different-project".into();
    http.call(
        "POST",
        route,
        route,
        &second_subject.secret,
        new_project,
        200,
    )
    .await;
    assert_eq!(
        http.call(
            "GET",
            first_page,
            directory,
            &admin.secret,
            Value::Null,
            200
        )
        .await,
        first
    );
    http.call(
        "POST",
        route,
        route,
        &other_key.secret,
        query(external),
        200,
    )
    .await;
    let foreign = http
        .call(
            "GET",
            first_page,
            directory,
            &other.secret,
            Value::Null,
            200,
        )
        .await;
    assert_eq!(foreign["company_id"], "other");
    assert_eq!(foreign["page"]["agents"].as_array().unwrap().len(), 1);
    assert_ne!(
        foreign["page"]["agents"][0]["registered_by"]["credential_id"],
        agent["registered_by"]["credential_id"]
    );
    http.call(
        "GET",
        "/api/v1/agents?contract_version=1&company_id=other",
        directory,
        &admin.secret,
        Value::Null,
        400,
    )
    .await;

    // Simultaneous first writes with one receipt key have one memory and association.
    let mut command = memory_create(
        "parallel-first-write",
        "A new external agent writes without enrollment",
        "shared",
    );
    command["scope"]["agent_id"] = "concurrent-new-agent".into();
    let jobs: Vec<_> = (0..10)
        .map(|_| {
            let token = key.secret.clone();
            let body = command.clone();
            tokio::task::spawn_blocking(move || {
                crate::http_server_tests::wire_request(
                    port,
                    "POST",
                    "/api/v1/memory/commands",
                    &token,
                    body,
                )
            })
        })
        .collect();
    let mut receipts = vec![];
    for job in jobs {
        let (status, receipt) = job.await.unwrap();
        assert_eq!(status, 200);
        receipts.push(receipt);
    }
    assert!(receipts.iter().all(|receipt| receipt == &receipts[0]));
    let after = http
        .call(
            "GET",
            first_page,
            directory,
            &admin.secret,
            Value::Null,
            200,
        )
        .await;
    assert_eq!(after["page"]["agents"].as_array().unwrap().len(), 2);
    let write_agent = after["page"]["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|agent| agent["agent_id"] == "concurrent-new-agent")
        .unwrap();
    assert_eq!(write_agent["trigger"]["kind"], "memory_command");
    assert_eq!(
        write_agent["trigger"]["record_id"],
        receipts[0]["receipt"]["record_id"]
    );
    let first = http
        .call(
            "GET",
            "/api/v1/agents?contract_version=1&limit=1",
            directory,
            &admin.secret,
            Value::Null,
            200,
        )
        .await;
    let cursor = first["page"]["next_after_agent_id"].as_str().unwrap();
    let next = http
        .call(
            "GET",
            &format!("/api/v1/agents?contract_version=1&limit=1&after_agent_id={cursor}"),
            directory,
            &admin.secret,
            Value::Null,
            200,
        )
        .await;
    assert!(next["page"]["next_after_agent_id"].is_null());
    assert_ne!(
        first["page"]["agents"][0]["agent_id"],
        next["page"]["agents"][0]["agent_id"]
    );
    identity
        .revoke(&admin.secret, key.credential.id, 1)
        .unwrap();
    http.call(
        "POST",
        route,
        route,
        &key.secret,
        query("revoked-request"),
        401,
    )
    .await;
    assert_eq!(
        http.call(
            "GET",
            first_page,
            directory,
            &admin.secret,
            Value::Null,
            200
        )
        .await,
        after
    );
    server.abort();
}

#[test]
fn agent_registration_and_first_memory_acknowledgement_survive_process_kill() {
    use crate::http_server_tests::{crash_server_for, wire_request};
    let dir = TempDir::new().unwrap();
    let (admin, key) = {
        let (_, identity) = app(dir.path());
        let admin = identity.bootstrap_tenant("company", "owner").unwrap();
        let key = identity
            .issue(&admin.secret, integration("integration"))
            .unwrap();
        (admin, key)
    };
    let fixture = "platform_http_tests::platform_http_memory_child";
    let (mut process, port) = crash_server_for(dir.path(), fixture);
    let first = memory_create("first", "Memory and agent registration", "private");
    let (status, receipt) = wire_request(
        port,
        "POST",
        "/api/v1/memory/commands",
        &key.secret,
        first.clone(),
    );
    assert_eq!(status, 200);
    let (status, _) = wire_request(
        port,
        "POST",
        "/api/v1/memory/query",
        &key.secret,
        query("read-only-registration"),
    );
    assert_eq!(status, 200);
    let (status, agents) = wire_request(
        port,
        "GET",
        "/api/v1/agents?contract_version=1",
        &admin.secret,
        Value::Null,
    );
    assert_eq!(status, 200);
    assert_eq!(agents["page"]["agents"].as_array().unwrap().len(), 2);
    assert_eq!(
        agents["page"]["agents"]
            .as_array()
            .unwrap()
            .iter()
            .find(|agent| agent["trigger"]["kind"] == "memory_command")
            .unwrap()["trigger"]["record_id"],
        receipt["receipt"]["record_id"]
    );
    process.0.kill().unwrap();
    process.0.wait().unwrap();
    let (_restarted, port) = crash_server_for(dir.path(), fixture);
    assert_eq!(
        wire_request(
            port,
            "GET",
            "/api/v1/agents?contract_version=1",
            &admin.secret,
            Value::Null
        ),
        (200, agents)
    );
    assert_eq!(
        wire_request(port, "POST", "/api/v1/memory/commands", &key.secret, first),
        (200, receipt)
    );
}
