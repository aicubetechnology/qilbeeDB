//! Exercise real HTTP responses, authorization boundaries and the published schema.
use super::*;

struct Client {
    client: reqwest::Client,
    base: String,
    api: Value,
}

impl Client {
    async fn call(
        &self,
        method: &str,
        path: &str,
        template: &str,
        token: &str,
        body: Value,
        expected: u16,
    ) -> Value {
        let mut request = self
            .client
            .request(method.parse().unwrap(), format!("{}{path}", self.base))
            .bearer_auth(token);
        if method == "POST" {
            request = request.json(&body);
        }
        let response = request.send().await.unwrap();
        assert_eq!(response.status().as_u16(), expected, "{method} {path}");
        assert_eq!(response.headers()["cache-control"], "no-store");
        let value: Value = response.json().await.unwrap();
        let schema = self.api["paths"][template][method.to_lowercase()]["responses"]
            [expected.to_string()]["content"]["application/json"]["schema"]
            .clone();
        assert!(
            !schema.is_null(),
            "Missing response schema: {template} {expected}"
        );
        let schema = json!({"$schema":"https://json-schema.org/draft/2020-12/schema","allOf":[schema],"components":self.api["components"]});
        let validator = jsonschema::draft202012::options().build(&schema).unwrap();
        if let Err(error) = validator.validate(&value) {
            panic!("Response schema failed: {error}");
        }
        value
    }
}

#[tokio::test]
async fn global_administration_real_http_enforces_saas_boundaries_and_openapi() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let master = identity.bootstrap_master("master").unwrap();
    let existing = identity
        .bootstrap_tenant("existing-company", "existing-owner")
        .unwrap();
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
    let create = "/api/v1/admin/tenants";
    let registration =
        json!({"contract_version":1,"tenant_id":"company-a","subject_id":"company-owner"});
    http.call("POST", create, create, "", registration.clone(), 401)
        .await;
    http.call(
        "POST",
        create,
        create,
        &existing.secret,
        registration.clone(),
        403,
    )
    .await;
    // There is no HTTP master bootstrap or caller-controlled tenant-to-master promotion.
    for path in ["/api/v1/admin/bootstrap-master", "/api/v1/bootstrap-master"] {
        let response = http
            .client
            .post(format!("{}{path}", http.base))
            .json(&registration)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), 404);
    }
    let issue = "/api/v1/admin/credentials";
    let spec = json!({"contract_version":1,"spec":{"subject_id":"signup-backend","capabilities":["tenant_create"],"expires_at_millis":null}});
    let provisioner = http
        .call("POST", issue, issue, &master.secret, spec.clone(), 201)
        .await;
    let signup = provisioner["secret"].as_str().unwrap();
    assert_eq!(provisioner["credential"]["is_master"], false);
    http.call("POST", issue, issue, signup, spec, 403).await;
    let mut forged = registration.clone();
    forged["is_master"] = true.into();
    http.call("POST", create, create, signup, forged, 400).await;
    let registered = http
        .call("POST", create, create, signup, registration.clone(), 201)
        .await;
    let company = registered["secret"].as_str().unwrap();
    http.call("POST", create, create, signup, registration, 409)
        .await;
    let lookup = "/api/v1/admin/tenants/company-a";
    let template = "/api/v1/admin/tenants/{tenant}";
    http.call("GET", lookup, template, signup, Value::Null, 403)
        .await;
    http.call("GET", lookup, template, company, Value::Null, 403)
        .await;
    let inspected = http
        .call("GET", lookup, template, &master.secret, Value::Null, 200)
        .await;
    assert_eq!(
        inspected["registration"]["initial_admin_id"],
        registered["credential"]["id"]
    );
    http.call(
        "GET",
        "/api/v1/admin/tenants/absent",
        template,
        &master.secret,
        Value::Null,
        404,
    )
    .await;
    let admin_path = "/api/v1/admin/tenants/company-a/admin-credentials";
    let admin_template = "/api/v1/admin/tenants/{tenant}/admin-credentials";
    let appointment = json!({"contract_version":1,"subject_id":"recovery-operator"});
    http.call(
        "POST",
        admin_path,
        admin_template,
        signup,
        appointment.clone(),
        403,
    )
    .await;
    let recovered = http
        .call(
            "POST",
            admin_path,
            admin_template,
            &master.secret,
            appointment,
            201,
        )
        .await;
    assert_eq!(recovered["credential"]["spec"]["grants"], json!([]));
    let tenant_identity = "/api/v1/identity";
    http.call(
        "GET",
        tenant_identity,
        tenant_identity,
        &master.secret,
        Value::Null,
        401,
    )
    .await;
    http.call(
        "GET",
        tenant_identity,
        tenant_identity,
        signup,
        Value::Null,
        401,
    )
    .await;
    http.call(
        "GET",
        tenant_identity,
        tenant_identity,
        company,
        Value::Null,
        200,
    )
    .await;
    let who = "/api/v1/admin/identity";
    let sanitized = http.call("GET", who, who, signup, Value::Null, 200).await;
    assert!(sanitized.get("secret").is_none());
    let target = format!(
        "/api/v1/admin/credentials/{}",
        provisioner["credential"]["id"].as_str().unwrap()
    );
    http.call(
        "GET",
        &target,
        "/api/v1/admin/credentials/{id}",
        &master.secret,
        Value::Null,
        200,
    )
    .await;
    let rotate = format!("{target}/rotate");
    let change = json!({"contract_version":1,"expected_revision":1});
    let rotated = http
        .call(
            "POST",
            &rotate,
            "/api/v1/admin/credentials/{id}/rotate",
            &master.secret,
            change.clone(),
            200,
        )
        .await;
    http.call("GET", who, who, signup, Value::Null, 401).await;
    http.call(
        "POST",
        &rotate,
        "/api/v1/admin/credentials/{id}/rotate",
        &master.secret,
        change,
        409,
    )
    .await;
    let revoke = format!("{target}/revoke");
    http.call(
        "POST",
        &revoke,
        "/api/v1/admin/credentials/{id}/revoke",
        &master.secret,
        json!({"contract_version":1,"expected_revision":2}),
        200,
    )
    .await;
    http.call(
        "GET",
        who,
        who,
        rotated["secret"].as_str().unwrap(),
        Value::Null,
        401,
    )
    .await;
    // Newly registered tenant administrators still inherit exact tenant isolation.
    let key_a = memory_key(&identity, company, "same-subject", true);
    let key_b = memory_key(&identity, &existing.secret, "same-subject", true);
    let command = "/api/v1/memory/commands";
    let created = http
        .call(
            "POST",
            command,
            command,
            &key_a,
            memory_create("private-record", "company-a secret", "shared"),
            200,
        )
        .await;
    let id = created["receipt"]["record_id"].as_str().unwrap();
    let path = format!(
        "/api/v1/memory/records/{id}?contract_version=1&project_id=project&agent_id=agent&visibility=shared"
    );
    http.call(
        "GET",
        &path,
        "/api/v1/memory/records/{id}",
        &key_b,
        Value::Null,
        404,
    )
    .await;
    // Tenant capability payloads cannot introduce a global capability.
    let escalation = json!({"contract_version":1,"spec":{"subject_id":"attacker","capabilities":["tenant_create"],"grants":[]}});
    http.call(
        "POST",
        "/api/v1/credentials",
        "/api/v1/credentials",
        company,
        escalation,
        400,
    )
    .await;
    server.abort();
    let _ = server.await;
}
