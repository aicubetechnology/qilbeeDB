use super::administration::Client;
use super::*;

#[tokio::test]
async fn administrative_directory_http_pages_preserve_scope_and_published_schemas() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let master = identity.bootstrap_master("master").unwrap();
    let a = identity.bootstrap_tenant("a", "owner").unwrap();
    let b = identity.bootstrap_tenant("b", "owner").unwrap();
    identity
        .issue(
            &a.secret,
            CredentialSpec {
                subject_id: "worker".into(),
                capabilities: [Capability::MemoryRead].into(),
                grants: vec![],
                expires_at_millis: None,
            },
        )
        .unwrap();
    identity
        .create_login_account(&a.secret, false, "owner", "test-password", a.credential.id)
        .unwrap();
    identity
        .create_login_account(&b.secret, false, "owner", "test-password", b.credential.id)
        .unwrap();
    identity
        .create_login_account(
            &master.secret,
            true,
            "master",
            "test-password",
            master.credential.id,
        )
        .unwrap();
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
    let path = "/api/v1/credentials";
    http.call(
        "GET",
        &format!("{path}?contract_version=1"),
        path,
        "",
        Value::Null,
        401,
    )
    .await;
    let first = http
        .call(
            "GET",
            &format!("{path}?contract_version=1&limit=1"),
            path,
            &a.secret,
            Value::Null,
            200,
        )
        .await;
    assert_eq!(first["page"]["items"].as_array().unwrap().len(), 1);
    let cursor = first["page"]["next_cursor"].as_str().unwrap();
    let continuation = format!("{path}?contract_version=1&limit=1&cursor={cursor}");
    let second = http
        .call("GET", &continuation, path, &a.secret, Value::Null, 200)
        .await;
    assert!(second["page"]["next_cursor"].is_null());
    assert_ne!(
        first["page"]["items"][0]["id"],
        second["page"]["items"][0]["id"]
    );
    http.call("GET", &continuation, path, &b.secret, Value::Null, 400)
        .await;
    for suffix in [
        "contract_version=1&limit=0",
        "contract_version=1&limit=101",
        "contract_version=1&cursor=invalid",
        "contract_version=2",
        "contract_version=1&tenant_id=b",
    ] {
        http.call(
            "GET",
            &format!("{path}?{suffix}"),
            path,
            &a.secret,
            Value::Null,
            400,
        )
        .await;
    }
    for (route, key, expected_count) in [
        ("/api/v1/admin/tenants", &master.secret, 2),
        ("/api/v1/admin/credentials", &master.secret, 1),
        ("/api/v1/admin/login-accounts", &master.secret, 1),
        ("/api/v1/login-accounts", &a.secret, 1),
    ] {
        let value = http
            .call(
                "GET",
                &format!("{route}?contract_version=1"),
                route,
                key,
                Value::Null,
                200,
            )
            .await;
        assert_eq!(
            value["page"]["items"].as_array().unwrap().len(),
            expected_count
        );
        if route.contains("/admin/") {
            http.call(
                "GET",
                &format!("{route}?contract_version=1"),
                route,
                &a.secret,
                Value::Null,
                403,
            )
            .await;
        }
    }
    let route = "/api/v1/admin/tenants";
    let created = http.call("POST", route, route, &master.secret, json!({"contract_version":1,"tenant_id":"named","display_name":"Named Company LLC","subject_id":"owner"}), 201).await;
    assert_eq!(created["tenant"]["display_name"], "Named Company LLC");
    identity.revoke(&a.secret, a.credential.id, 1).unwrap();
    http.call(
        "GET",
        &format!("{path}?contract_version=1"),
        path,
        &a.secret,
        Value::Null,
        401,
    )
    .await;
    server.abort();
}
