//! Company administrators must discover retained data independently of grants.
use super::administration::Client;
use super::*;

#[tokio::test]
async fn company_inventory_survives_revoked_grants_and_keeps_private_subjects_separate() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("company", "owner").unwrap();
    let first = memory_key(&identity, &admin.secret, "first-subject", true);
    let second = memory_key(&identity, &admin.secret, "second-subject", true);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
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
    let commands = "/api/v1/memory/commands";
    let first_receipt = http
        .call(
            "POST",
            commands,
            commands,
            &first,
            memory_create("one", "First private memory", "private"),
            200,
        )
        .await;
    let second_receipt = http
        .call(
            "POST",
            commands,
            commands,
            &second,
            memory_create("two", "Second private memory", "private"),
            200,
        )
        .await;
    for key in [&first, &second] {
        let credential = identity.authenticate(key).unwrap();
        identity
            .revoke(&admin.secret, credential.id, credential.revision)
            .unwrap();
    }
    let route = "/api/v1/company/memory/workspaces";
    let page = http
        .call(
            "GET",
            &format!("{route}?contract_version=1"),
            route,
            &admin.secret,
            Value::Null,
            200,
        )
        .await;
    assert_eq!(page["page"]["workspaces"].as_array().unwrap().len(), 2);
    let subjects: std::collections::BTreeSet<_> = page["page"]["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["private_subject_id"].as_str().unwrap())
        .collect();
    assert_eq!(subjects, ["first-subject", "second-subject"].into());
    let query_route = "/api/v1/company/memory/query";
    let read_route = "/api/v1/company/memory/read";
    let workspaces = page["page"]["workspaces"].as_array().unwrap();
    let first_workspace = workspaces
        .iter()
        .find(|w| w["private_subject_id"] == "first-subject")
        .unwrap()["workspace_id"]
        .as_str()
        .unwrap();
    let second_workspace = workspaces
        .iter()
        .find(|w| w["private_subject_id"] == "second-subject")
        .unwrap()["workspace_id"]
        .as_str()
        .unwrap();
    let request = json!({"contract_version":1,"workspace_id":first_workspace,"filter":{}});
    let credentials_before = http
        .call(
            "GET",
            "/api/v1/credentials?contract_version=1&limit=100",
            "/api/v1/credentials",
            &admin.secret,
            Value::Null,
            200,
        )
        .await;
    let result = http
        .call(
            "POST",
            query_route,
            query_route,
            &admin.secret,
            request.clone(),
            200,
        )
        .await;
    assert_eq!(
        result["page"]["entries"][0]["record"]["record_id"],
        first_receipt["receipt"]["record_id"]
    );
    assert_eq!(
        result["page"]["entries"][0]["eligibility"]["eligible"],
        true
    );
    assert!(result["page"]["next_after"].is_null());
    assert_eq!(result["page"]["stop_reason"], "exhausted");
    let inspected = http.call("POST", read_route, read_route, &admin.secret, json!({"contract_version":1,"workspace_id":second_workspace,"record_id":second_receipt["receipt"]["record_id"]}), 200).await;
    assert_eq!(
        inspected["entry"]["record"]["payload"]["content"]["primary"],
        "Second private memory"
    );
    http.call("POST", read_route, read_route, &admin.secret, json!({"contract_version":1,"workspace_id":first_workspace,"record_id":second_receipt["receipt"]["record_id"]}), 404).await;
    // Native administration performs no credential synthesis or agent enrollment.
    assert_eq!(
        http.call(
            "GET",
            "/api/v1/credentials?contract_version=1&limit=100",
            "/api/v1/credentials",
            &admin.secret,
            Value::Null,
            200
        )
        .await,
        credentials_before
    );
    let reader = memory_key(&identity, &admin.secret, "reader", false);
    http.call(
        "POST",
        query_route,
        query_route,
        &reader,
        request.clone(),
        403,
    )
    .await;
    http.call(
        "GET",
        &format!("{route}?contract_version=1"),
        route,
        &reader,
        Value::Null,
        403,
    )
    .await;
    http.call(
        "POST",
        query_route,
        query_route,
        &first,
        request.clone(),
        401,
    )
    .await;
    let other = identity.bootstrap_tenant("other-company", "owner").unwrap();
    let other_key = memory_key(&identity, &other.secret, "first-subject", true);
    http.call(
        "POST",
        commands,
        commands,
        &other_key,
        memory_create("one", "First private memory", "private"),
        200,
    )
    .await;
    http.call(
        "POST",
        query_route,
        query_route,
        &other.secret,
        request.clone(),
        404,
    )
    .await;
    let other_page = http
        .call(
            "GET",
            &format!("{route}?contract_version=1"),
            route,
            &other.secret,
            Value::Null,
            200,
        )
        .await;
    assert_eq!(
        other_page["page"]["workspaces"].as_array().unwrap().len(),
        1
    );
    assert_ne!(
        other_page["page"]["workspaces"][0]["workspace_id"],
        first_workspace
    );
    let admin_only = identity
        .issue(
            &admin.secret,
            CredentialSpec {
                scope_policy: None,
                subject_id: "admin-only".into(),
                capabilities: [Capability::CredentialAdmin].into(),
                grants: vec![],
                expires_at_millis: None,
            },
        )
        .unwrap();
    http.call(
        "POST",
        query_route,
        query_route,
        &admin_only.secret,
        request.clone(),
        200,
    )
    .await;
    for extra in ["company_id", "private_subject_id", "scope"] {
        let mut forged = request.clone();
        forged[extra] = "outside".into();
        http.call("POST", query_route, query_route, &admin.secret, forged, 400)
            .await;
    }
    for filter in [
        json!({"limit":0}),
        json!({"limit":101}),
        json!({"scan_limit":0}),
        json!({"scan_limit":10001}),
        json!({"view":"unknown"}),
    ] {
        let mut invalid = request.clone();
        invalid["filter"] = filter;
        http.call(
            "POST",
            query_route,
            query_route,
            &admin.secret,
            invalid,
            400,
        )
        .await;
    }
    for parameters in [
        "contract_version=2",
        "contract_version=1&limit=0",
        "contract_version=1&after_workspace_id=invalid",
        "contract_version=1&company_id=other-company",
    ] {
        http.call(
            "GET",
            &format!("{route}?{parameters}"),
            route,
            &admin.secret,
            Value::Null,
            400,
        )
        .await;
    }
    let one_page = http
        .call(
            "GET",
            &format!("{route}?contract_version=1&limit=1"),
            route,
            &admin.secret,
            Value::Null,
            200,
        )
        .await;
    let cursor = one_page["page"]["next_after_workspace_id"]
        .as_str()
        .unwrap();
    let next_page = http
        .call(
            "GET",
            &format!("{route}?contract_version=1&limit=1&after_workspace_id={cursor}"),
            route,
            &admin.secret,
            Value::Null,
            200,
        )
        .await;
    assert!(next_page["page"]["next_after_workspace_id"].is_null());
    assert_ne!(
        one_page["page"]["workspaces"][0]["workspace_id"],
        next_page["page"]["workspaces"][0]["workspace_id"]
    );
    let writer = memory_key(&identity, &admin.secret, "first-subject", true);
    http.call("POST", commands, commands, &writer, json!({"contract_version":1,"scope":memory_scope("private"),"idempotency_key":"delete","operation":{"type":"delete","record_id":first_receipt["receipt"]["record_id"],"expected_revision":1}}), 200).await;
    let retained = http
        .call(
            "POST",
            query_route,
            query_route,
            &admin.secret,
            request.clone(),
            200,
        )
        .await;
    assert!(retained["page"]["entries"][0]["record"]["payload"].is_null());
    assert_eq!(
        retained["page"]["entries"][0]["eligibility"]["first_failure"]["reason"],
        "deleted"
    );
    let mut current = request.clone();
    current["filter"]["view"] = "current".into();
    assert_eq!(
        http.call(
            "POST",
            query_route,
            query_route,
            &admin.secret,
            current,
            200
        )
        .await["page"]["entries"],
        json!([])
    );
    http.call("POST", read_route, read_route, &admin.secret, json!({"contract_version":1,"workspace_id":first_workspace,"record_id":first_receipt["receipt"]["record_id"]}), 200).await;
    identity
        .revoke(&admin.secret, admin_only.credential.id, 1)
        .unwrap();
    http.call(
        "POST",
        query_route,
        query_route,
        &admin_only.secret,
        request,
        401,
    )
    .await;
    server.abort();
}

#[test]
fn company_memory_catalog_and_private_read_survive_process_kill() {
    use crate::http_server_tests::{crash_server_for, wire_request};
    let dir = TempDir::new().unwrap();
    let (admin, key) = {
        let (_, identity) = app(dir.path());
        let admin = identity.bootstrap_tenant("company", "owner").unwrap();
        let key = memory_key(&identity, &admin.secret, "private-owner", true);
        (admin, key)
    };
    let fixture = "platform_http_tests::platform_http_memory_child";
    let (mut process, port) = crash_server_for(dir.path(), fixture);
    let (status, receipt) = wire_request(
        port,
        "POST",
        "/api/v1/memory/commands",
        &key,
        memory_create("crash", "Retained private memory", "private"),
    );
    assert_eq!(status, 200);
    let route = "/api/v1/company/memory/workspaces?contract_version=1";
    let (status, workspaces) = wire_request(port, "GET", route, &admin.secret, Value::Null);
    assert_eq!(status, 200);
    let workspace = workspaces["page"]["workspaces"][0]["workspace_id"].clone();
    process.0.kill().unwrap();
    process.0.wait().unwrap();
    let (_restarted, port) = crash_server_for(dir.path(), fixture);
    assert_eq!(
        wire_request(port, "GET", route, &admin.secret, Value::Null),
        (200, workspaces)
    );
    let (status, record) = wire_request(
        port,
        "POST",
        "/api/v1/company/memory/read",
        &admin.secret,
        json!({"contract_version":1,"workspace_id":workspace,"record_id":receipt["receipt"]["record_id"]}),
    );
    assert_eq!(status, 200);
    assert_eq!(
        record["entry"]["record"]["record_id"],
        receipt["receipt"]["record_id"]
    );
    assert_eq!(
        record["entry"]["record"]["payload"]["content"]["primary"],
        "Retained private memory"
    );
}
