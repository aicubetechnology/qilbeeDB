//! Real TCP qualification of dynamic external scopes and administrative changes.
use super::administration::Client;
use super::*;

fn policy() -> Value {
    json!({
        "version":"company_scopes_v1",
        "projects":{"mode":"all"}, "agents":{"mode":"all"}, "missions":{"mode":"all"},
        "allow_unassigned_mission":true, "visibilities":["shared","private"]
    })
}

#[test]
fn company_scope_policy_acknowledgements_and_revocation_survive_process_kill() {
    use crate::http_server_tests::{crash_server_for, wire_request};
    let dir = TempDir::new().unwrap();
    let admin = {
        let (_, identity) = app(dir.path());
        identity
            .bootstrap_tenant("company", "owner")
            .unwrap()
            .secret
    };
    let fixture = "platform_http_tests::platform_http_memory_child";
    let (mut process, port) = crash_server_for(dir.path(), fixture);
    let (status, issued) = wire_request(
        port,
        "POST",
        "/api/v1/credentials",
        &admin,
        integration("integration"),
    );
    assert_eq!(status, 201);
    let token = issued["secret"].as_str().unwrap();
    let id = issued["credential"]["id"].as_str().unwrap();
    let command = memory_create(
        "durable-external-scope",
        "Acknowledged before policy change",
        "shared",
    );
    let (status, receipt) = wire_request(
        port,
        "POST",
        "/api/v1/memory/commands",
        token,
        command.clone(),
    );
    assert_eq!(status, 200);
    let mut policy = policy();
    policy["agents"] = json!({"mode":"only","ids":["agent"]});
    let path = format!("/api/v1/credentials/{id}/scope-authority");
    let update = json!({"contract_version":1,"expected_revision":1,"authority":{"grants":[],"scope_policy":policy}});
    let (status, changed) = wire_request(port, "POST", &path, &admin, update.clone());
    assert_eq!(status, 200);
    process.0.kill().unwrap();
    process.0.wait().unwrap();
    let (mut restarted, port) = crash_server_for(dir.path(), fixture);
    let (status, current) = wire_request(port, "GET", "/api/v1/identity", token, Value::Null);
    assert_eq!(status, 200);
    assert_eq!(current, changed);
    assert_eq!(wire_request(port, "POST", &path, &admin, update).0, 409);
    assert_eq!(
        wire_request(port, "POST", "/api/v1/memory/commands", token, command).1,
        receipt
    );
    let mut outside = memory_scope("shared");
    outside["agent_id"] = "outside".into();
    assert_eq!(
        wire_request(port, "POST", "/api/v1/memory/query", token, query(outside)).0,
        403
    );
    let revoke = format!("/api/v1/credentials/{id}/revoke");
    assert_eq!(
        wire_request(
            port,
            "POST",
            &revoke,
            &admin,
            json!({"contract_version":1,"expected_revision":2})
        )
        .0,
        200
    );
    restarted.0.kill().unwrap();
    restarted.0.wait().unwrap();
    let (_recovered, port) = crash_server_for(dir.path(), fixture);
    assert_eq!(
        wire_request(port, "GET", "/api/v1/identity", token, Value::Null).0,
        401
    );
    let (status, current) = wire_request(
        port,
        "GET",
        &format!("/api/v1/credentials/{id}"),
        &admin,
        Value::Null,
    );
    assert_eq!(status, 200);
    assert_eq!(current["credential"]["revision"], 3);
    assert_eq!(
        current["credential"]["history"][1],
        changed["credential"]["history"][1]
    );
}
fn integration(subject: &str) -> Value {
    json!({"contract_version":1,"spec":{
        "subject_id":subject,"capabilities":["memory_read","memory_write"],
        "grants":[],"scope_policy":policy(),"expires_at_millis":null
    }})
}
fn query(scope: Value) -> Value {
    json!({"contract_version":1,"scope":scope,"filter":{"limit":10,"scan_limit":100}})
}

#[tokio::test]
async fn company_scope_policy_real_http_authorizes_new_ids_preserves_subjects_and_changes_without_new_keys()
 {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let a = identity.bootstrap_tenant("company-a", "owner").unwrap();
    let b = identity.bootstrap_tenant("company-b", "owner").unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
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
    let issue = "/api/v1/credentials";
    let a_key = http
        .call(
            "POST",
            issue,
            issue,
            &a.secret,
            integration("integration"),
            201,
        )
        .await;
    let b_key = http
        .call(
            "POST",
            issue,
            issue,
            &b.secret,
            integration("integration"),
            201,
        )
        .await;
    let other_subject = http
        .call(
            "POST",
            issue,
            issue,
            &a.secret,
            integration("different-subject"),
            201,
        )
        .await;
    let a_token = a_key["secret"].as_str().unwrap();
    let b_token = b_key["secret"].as_str().unwrap();
    let other_token = other_subject["secret"].as_str().unwrap();
    let mut legacy = integration("legacy");
    legacy["spec"]
        .as_object_mut()
        .unwrap()
        .remove("scope_policy");
    let old = http
        .call("POST", issue, issue, &a.secret, legacy, 201)
        .await;
    assert!(old["credential"]["spec"].get("scope_policy").is_none());
    let scope = json!({"project_id":"consumer/project","agent_id":"agent/new:α","mission_id":null,"visibility":"shared"});
    let query_path = "/api/v1/memory/query";
    http.call(
        "POST",
        query_path,
        query_path,
        old["secret"].as_str().unwrap(),
        query(scope.clone()),
        403,
    )
    .await;
    http.call(
        "POST",
        issue,
        issue,
        a_token,
        integration("cannot-issue"),
        403,
    )
    .await;

    // New project and agent IDs require no provisioning request or per-agent key.
    for visibility in ["shared", "private"] {
        let mut create = memory_create(visibility, "A scoped memory", visibility);
        create["scope"] = scope.clone();
        create["scope"]["visibility"] = visibility.into();
        let path = "/api/v1/memory/commands";
        let first = http
            .call("POST", path, path, a_token, create.clone(), 200)
            .await;
        assert_eq!(
            first,
            http.call("POST", path, path, a_token, create.clone(), 200)
                .await
        );
        let id = first["receipt"]["record_id"].as_str().unwrap();
        let record_path = format!(
            "/api/v1/memory/records/{id}?contract_version=1&project_id=consumer%2Fproject&agent_id=agent%2Fnew%3A%CE%B1&visibility={visibility}"
        );
        let template = "/api/v1/memory/records/{id}";
        http.call("GET", &record_path, template, a_token, Value::Null, 200)
            .await;
        http.call("GET", &record_path, template, b_token, Value::Null, 404)
            .await;
        http.call(
            "GET",
            &record_path,
            template,
            other_token,
            Value::Null,
            if visibility == "shared" { 200 } else { 404 },
        )
        .await;
        let private_query = query(create["scope"].clone());
        let found = http
            .call(
                "POST",
                query_path,
                query_path,
                a_token,
                private_query.clone(),
                200,
            )
            .await;
        assert_eq!(found["page"]["records"].as_array().unwrap().len(), 1);
        let other = http
            .call("POST", query_path, query_path, b_token, private_query, 200)
            .await;
        assert!(other["page"]["records"].as_array().unwrap().is_empty());
    }
    let login_accounts = "/api/v1/login-accounts";
    http.call("POST", login_accounts, login_accounts, &a.secret, json!({"contract_version":1,"username":"integration-operator","password":"synthetic-policy-password","credential_id":a_key["credential"]["id"]}), 201).await;
    let login = "/api/v1/login";
    let session = http.call("POST", login, login, "", json!({"contract_version":1,"tenant_id":"company-a","username":"integration-operator","password":"synthetic-policy-password"}), 200).await;
    let session_token = session["session"]["token"].as_str().unwrap();
    http.call(
        "POST",
        query_path,
        query_path,
        session_token,
        query(scope.clone()),
        200,
    )
    .await;
    let id = a_key["credential"]["id"].as_str().unwrap();
    let path = format!("/api/v1/credentials/{id}/scope-authority");
    let template = "/api/v1/credentials/{id}/scope-authority";
    let mut narrow = policy();
    narrow["projects"] = json!({"mode":"only","ids":[scope["project_id"]]});
    narrow["agents"] = json!({"mode":"only","ids":[scope["agent_id"]]});
    narrow["missions"] = json!({"mode":"only","ids":[]});
    narrow["visibilities"] = json!(["shared"]);
    let update = json!({"contract_version":1,"expected_revision":1,"authority":{"grants":[],"scope_policy":narrow}});
    http.call("POST", &path, template, "", update.clone(), 401)
        .await;
    http.call("POST", &path, template, &b.secret, update.clone(), 403)
        .await;
    http.call("POST", &path, template, a_token, update.clone(), 403)
        .await;
    let changed = http
        .call("POST", &path, template, &a.secret, update.clone(), 200)
        .await;
    assert_eq!(changed["credential"]["revision"], 2);
    assert_eq!(
        changed["credential"]["history"][1]["scope_authority_change"]["previous"]["scope_policy"],
        policy()
    );
    assert_eq!(
        changed["credential"]["history"][1]["scope_authority_change"]["current"]["scope_policy"],
        narrow
    );
    assert!(changed.get("secret").is_none());
    http.call("POST", &path, template, &a.secret, update, 409)
        .await;
    http.call(
        "GET",
        "/api/v1/identity",
        "/api/v1/identity",
        session_token,
        Value::Null,
        401,
    )
    .await;
    http.call(
        "GET",
        "/api/v1/identity",
        "/api/v1/identity",
        a_token,
        Value::Null,
        200,
    )
    .await;
    http.call(
        "POST",
        query_path,
        query_path,
        a_token,
        query(scope.clone()),
        200,
    )
    .await;
    for (field, value) in [
        ("project_id", json!("other-project")),
        ("agent_id", json!("another-agent")),
        ("mission_id", json!("new-mission")),
        ("visibility", json!("private")),
    ] {
        let mut outside = scope.clone();
        outside[field] = value;
        http.call("POST", query_path, query_path, a_token, query(outside), 403)
            .await;
    }
    for field in ["tenant_id", "subject_id"] {
        let mut forged = scope.clone();
        forged[field] = "company-b".into();
        http.call("POST", query_path, query_path, a_token, query(forged), 400)
            .await;
    }
    let clear = json!({"contract_version":1,"expected_revision":2,"authority":{"grants":[]}});
    http.call("POST", &path, template, &a.secret, clear, 200)
        .await;
    http.call(
        "POST",
        query_path,
        query_path,
        a_token,
        query(scope.clone()),
        403,
    )
    .await;
    // Recover the known subject and namespace with the same secret and exact grants.
    let exact = json!({"contract_version":1,"expected_revision":3,"authority":{"grants":[scope]}});
    http.call("POST", &path, template, &a.secret, exact, 200)
        .await;
    let found = http
        .call("POST", query_path, query_path, a_token, query(scope), 200)
        .await;
    assert_eq!(found["page"]["records"].as_array().unwrap().len(), 1);
    let revoke = format!("/api/v1/credentials/{id}/revoke");
    http.call(
        "POST",
        &revoke,
        "/api/v1/credentials/{id}/revoke",
        &a.secret,
        json!({"contract_version":1,"expected_revision":4}),
        200,
    )
    .await;
    http.call("POST", &path, template, &a.secret, json!({"contract_version":1,"expected_revision":5,"authority":{"grants":[],"scope_policy":policy()}}), 403).await;
    http.call(
        "GET",
        "/api/v1/identity",
        "/api/v1/identity",
        a_token,
        Value::Null,
        401,
    )
    .await;
    server.abort();
}

#[tokio::test]
async fn company_scope_policy_http_and_openapi_reject_ambiguous_or_incomplete_issuance() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("company", "owner").unwrap();
    let mut invalid = Vec::new();
    for field in [
        "projects",
        "agents",
        "missions",
        "allow_unassigned_mission",
        "visibilities",
        "version",
    ] {
        let mut body = integration("worker");
        body["spec"]["scope_policy"]
            .as_object_mut()
            .unwrap()
            .remove(field);
        invalid.push(body);
    }
    for change in [
        json!({"mode":"all","ids":["ignored"]}),
        json!({"mode":"only","ids":["duplicate","duplicate"]}),
        json!({"mode":"unknown"}),
    ] {
        let mut body = integration("worker");
        body["spec"]["scope_policy"]["agents"] = change;
        invalid.push(body);
    }
    let mut future = integration("worker");
    future["spec"]["scope_policy"]["version"] = "company_scopes_v2".into();
    invalid.push(future);
    let mut escalated = integration("worker");
    escalated["spec"]["capabilities"] = json!(["credential_admin"]);
    invalid.push(escalated);
    let mut both = integration("worker");
    both["spec"]["grants"] = json!([memory_scope("shared")]);
    invalid.push(both);
    let mut duplicate = integration("worker");
    duplicate["spec"]["scope_policy"]["visibilities"] = json!(["shared", "shared"]);
    invalid.push(duplicate);
    let api: Value =
        serde_json::from_str(include_str!("../../../../docs/api/openapi.json")).unwrap();
    let schema = json!({"$ref":"#/components/schemas/IssueRequest","components":api["components"]});
    let validator = jsonschema::draft202012::options().build(&schema).unwrap();
    assert!(validator.is_valid(&integration("worker")));
    for body in invalid {
        assert!(!validator.is_valid(&body), "Schema accepted {body}");
        assert_eq!(
            request(&router, "POST", "/api/v1/credentials", &admin.secret, body)
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
    }
}
