//! Real TCP login and administrative responses must satisfy the published contract.
use super::administration::Client;
use super::*;

#[tokio::test]
async fn login_real_http_preserves_tenant_master_and_session_boundaries() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let master = identity.bootstrap_master("master").unwrap();
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
    let accounts = "/api/v1/login-accounts";
    let global_accounts = "/api/v1/admin/login-accounts";
    let create = json!({"contract_version":1,"username":"owner@example.test","password":"test-only-password","credential_id":a.credential.id});
    http.call("POST", accounts, accounts, "", create.clone(), 401)
        .await;
    let mut cross = create.clone();
    cross["credential_id"] = json!(b.credential.id);
    http.call("POST", accounts, accounts, &a.secret, cross, 403)
        .await;
    http.call(
        "POST",
        global_accounts,
        global_accounts,
        &a.secret,
        create.clone(),
        403,
    )
    .await;
    let account = http
        .call("POST", accounts, accounts, &a.secret, create.clone(), 201)
        .await;
    http.call("POST", accounts, accounts, &a.secret, create, 409)
        .await;
    let id = account["account"]["id"].as_str().unwrap();
    let login = "/api/v1/login";
    let payload = json!({"contract_version":1,"tenant_id":"company-a","username":"owner@example.test","password":"test-only-password"});
    let session = http
        .call("POST", login, login, "", payload.clone(), 200)
        .await;
    let token = session["session"]["token"].as_str().unwrap();
    let me = http
        .call(
            "GET",
            "/api/v1/identity",
            "/api/v1/identity",
            token,
            Value::Null,
            200,
        )
        .await;
    assert_eq!(me["credential"]["tenant_id"], "company-a");
    http.call(
        "POST",
        "/api/v1/admin/tenants",
        "/api/v1/admin/tenants",
        token,
        json!({"contract_version":1,"tenant_id":"evil","subject_id":"evil"}),
        403,
    )
    .await;
    http.call(
        "GET",
        &format!("{accounts}/{id}"),
        "/api/v1/login-accounts/{id}",
        &b.secret,
        Value::Null,
        403,
    )
    .await;
    http.call(
        "GET",
        &format!("{accounts}/{id}"),
        "/api/v1/login-accounts/{id}",
        token,
        Value::Null,
        200,
    )
    .await;
    let mut wrong = payload.clone();
    wrong["password"] = "wrong-password".into();
    let denied = http.call("POST", login, login, "", wrong, 401).await;
    let mut unknown = payload.clone();
    unknown["username"] = "absent".into();
    assert_eq!(
        denied,
        http.call("POST", login, login, "", unknown, 401).await
    );
    let mut other = payload.clone();
    other["tenant_id"] = "company-b".into();
    assert_eq!(
        denied,
        http.call("POST", login, login, "", other, 401).await
    );
    let change = format!("{accounts}/{id}/password");
    http.call(
        "POST",
        &change,
        "/api/v1/login-accounts/{id}/password",
        token,
        json!({"contract_version":1,"expected_revision":1,"password":"replacement-password"}),
        200,
    )
    .await;
    http.call(
        "GET",
        "/api/v1/identity",
        "/api/v1/identity",
        token,
        Value::Null,
        401,
    )
    .await;
    http.call(
        "GET",
        "/api/v1/identity",
        "/api/v1/identity",
        &a.secret,
        Value::Null,
        200,
    )
    .await;
    let mut replacement = payload.clone();
    replacement["password"] = "replacement-password".into();
    let session = http.call("POST", login, login, "", replacement, 200).await;
    let token = session["session"]["token"].as_str().unwrap();
    http.call(
        "POST",
        "/api/v1/logout",
        "/api/v1/logout",
        token,
        Value::Null,
        200,
    )
    .await;
    http.call(
        "POST",
        "/api/v1/logout",
        "/api/v1/logout",
        token,
        Value::Null,
        401,
    )
    .await;
    http.call(
        "POST",
        "/api/v1/logout",
        "/api/v1/logout",
        &a.secret,
        Value::Null,
        401,
    )
    .await;
    http.call(
        "POST",
        &format!("{accounts}/{id}/disable"),
        "/api/v1/login-accounts/{id}/disable",
        &a.secret,
        json!({"contract_version":1,"expected_revision":2}),
        200,
    )
    .await;
    let mut disabled = payload;
    disabled["password"] = "replacement-password".into();
    assert_eq!(
        denied,
        http.call("POST", login, login, "", disabled, 401).await
    );
    let master_account = http.call("POST", global_accounts, global_accounts, &master.secret, json!({"contract_version":1,"username":"master","password":"test-master-password","credential_id":master.credential.id}), 201).await;
    let master_id = master_account["account"]["id"].as_str().unwrap();
    let global_login = "/api/v1/admin/login";
    let session = http
        .call(
            "POST",
            global_login,
            global_login,
            "",
            json!({"contract_version":1,"username":"master","password":"test-master-password"}),
            200,
        )
        .await;
    let token = session["session"]["token"].as_str().unwrap();
    let me = http
        .call(
            "GET",
            "/api/v1/admin/identity",
            "/api/v1/admin/identity",
            token,
            Value::Null,
            200,
        )
        .await;
    assert_eq!(me["credential"]["is_master"], true);
    http.call(
        "GET",
        "/api/v1/identity",
        "/api/v1/identity",
        token,
        Value::Null,
        401,
    )
    .await;
    http.call(
        "POST",
        "/api/v1/admin/tenants",
        "/api/v1/admin/tenants",
        token,
        json!({"contract_version":1,"tenant_id":"company-c","subject_id":"owner"}),
        201,
    )
    .await;
    http.call(
        "GET",
        &format!("{global_accounts}/{master_id}"),
        "/api/v1/admin/login-accounts/{id}",
        token,
        Value::Null,
        200,
    )
    .await;
    http.call(
        "POST",
        &format!("{global_accounts}/{master_id}/password"),
        "/api/v1/admin/login-accounts/{id}/password",
        token,
        json!({"contract_version":1,"expected_revision":1,"password":"new-master-password"}),
        200,
    )
    .await;
    http.call(
        "GET",
        "/api/v1/admin/identity",
        "/api/v1/admin/identity",
        token,
        Value::Null,
        401,
    )
    .await;
    http.call(
        "POST",
        &format!("{global_accounts}/{master_id}/disable"),
        "/api/v1/admin/login-accounts/{id}/disable",
        &master.secret,
        json!({"contract_version":1,"expected_revision":2}),
        200,
    )
    .await;
    // The transport limit applies before expensive password work.
    http.call("POST", login, login, "", json!({"contract_version":1,"tenant_id":"company-a","username":"oversized","password":"x".repeat(9000)}), 413).await;
    server.abort();
}

#[tokio::test]
async fn anonymous_login_rate_limit_is_identical_for_unknown_names_and_validated_by_openapi() {
    let dir = TempDir::new().unwrap();
    let (router, _) = app(dir.path());
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
    let path = "/api/v1/admin/login";
    for _ in 0..8 {
        http.call(
            "POST",
            path,
            path,
            "",
            json!({"contract_version":1,"username":"unknown","password":"invalid-password"}),
            401,
        )
        .await;
    }
    http.call(
        "POST",
        path,
        path,
        "",
        json!({"contract_version":1,"username":"unknown","password":"invalid-password"}),
        429,
    )
    .await;
    http.call(
        "POST",
        path,
        path,
        "",
        json!({"contract_version":1,"username":"different","password":"invalid-password"}),
        401,
    )
    .await;
    server.abort();
}
