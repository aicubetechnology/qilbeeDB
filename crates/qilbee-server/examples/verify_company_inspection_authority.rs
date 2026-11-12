//! Real HTTP/credential/persistence qualification of company inspection authority.
//! The child serves the actual platform router; no simulated services are used.
use qilbee_graph::Database;
use qilbee_server::{platform_http, security::identity::IdentityStore};
use reqwest::{Client, Method, StatusCode};
use serde_json::{Value, json};
use std::{
    io::{BufRead, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::Arc,
    time::Duration,
};

struct Service {
    child: Child,
    base: String,
}
impl Service {
    fn start(path: &Path) -> Self {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .arg("--serve")
            .arg(path)
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut base = String::new();
        std::io::BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut base)
            .unwrap();
        let service = Self {
            child,
            base: base.trim().to_owned(),
        };
        assert!(
            service.base.starts_with("http://127.0.0.1:"),
            "real server failed to start"
        );
        service
    }
    fn kill(&mut self) {
        self.child.kill().unwrap();
        self.child.wait().unwrap();
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
struct Key {
    secret: String,
    id: String,
}
async fn request(
    client: &Client,
    service: &Service,
    method: Method,
    path: &str,
    key: Option<&str>,
    body: Option<Value>,
    expected: StatusCode,
) -> Value {
    let mut call = client.request(method, format!("{}{path}", service.base));
    if let Some(key) = key {
        call = call.bearer_auth(key);
    }
    if let Some(body) = body {
        call = call.json(&body);
    }
    let response = call.send().await.unwrap();
    assert_eq!(response.status(), expected, "unexpected status for {path}");
    assert_eq!(response.headers().get("cache-control").unwrap(), "no-store");
    response.json().await.unwrap()
}
fn scope() -> Value {
    json!({"project_id":"inspection-project","mission_id":"inspection-mission",
        "agent_id":"inspection-agent","visibility":"private"})
}
async fn issue(
    client: &Client,
    service: &Service,
    admin: &str,
    subject: &str,
    capabilities: &[&str],
    granted: bool,
) -> Key {
    let result = request(
        client,
        service,
        Method::POST,
        "/api/v1/credentials",
        Some(admin),
        Some(json!({"contract_version":1,"spec":{"subject_id":subject,
            "capabilities":capabilities,"grants":if granted { vec![scope()] } else { vec![] },
            "expires_at_millis":null}})),
        StatusCode::CREATED,
    )
    .await;
    Key {
        secret: result["secret"].as_str().unwrap().to_owned(),
        id: result["credential"]["id"].as_str().unwrap().to_owned(),
    }
}
async fn create(
    client: &Client,
    service: &Service,
    writer: &str,
    operation: &str,
    expired: bool,
) -> String {
    let result = request(
        client,
        service,
        Method::POST,
        "/api/v1/memory/commands",
        Some(writer),
        Some(
            json!({"contract_version":1,"scope":scope(),"idempotency_key":operation,
            "operation":{"type":"create","record":{"episode_type":"Observation",
                "content":{"primary":"disposable company authority fixture","secondary":null,
                    "context":null,"data":null,"embedding":null},
                "event_time_millis":0,"valid_until_millis":if expired { Some(1) } else { None },
                "tags":[],"metadata":{}}}}),
        ),
        StatusCode::OK,
    )
    .await;
    result["receipt"]["record_id"].as_str().unwrap().to_owned()
}
fn scoped_path(id: &str) -> String {
    format!(
        "/api/v1/memory/records/{id}?contract_version=1&project_id=inspection-project&mission_id=inspection-mission&agent_id=inspection-agent&visibility=private"
    )
}
async fn workspaces(client: &Client, service: &Service, key: &str, expected: StatusCode) -> Value {
    request(
        client,
        service,
        Method::GET,
        "/api/v1/company/memory/workspaces?contract_version=1&limit=25",
        Some(key),
        None,
        expected,
    )
    .await
}
async fn inspect(
    client: &Client,
    service: &Service,
    key: &str,
    workspace: &str,
    record: &str,
    expected: StatusCode,
) -> Value {
    request(
        client,
        service,
        Method::POST,
        "/api/v1/company/memory/read",
        Some(key),
        Some(json!({"contract_version":1,"workspace_id":workspace,"record_id":record})),
        expected,
    )
    .await
}
async fn revoke(client: &Client, service: &Service, admin: &str, key: &Key) {
    request(
        client,
        service,
        Method::POST,
        &format!("/api/v1/credentials/{}/revoke", key.id),
        Some(admin),
        Some(json!({"contract_version":1,"expected_revision":1})),
        StatusCode::OK,
    )
    .await;
}
async fn login_session(
    client: &Client,
    service: &Service,
    admin: &str,
    credential: &Key,
) -> (String, String) {
    let password = format!("inspection-{}", uuid::Uuid::new_v4());
    let account = request(
        client,
        service,
        Method::POST,
        "/api/v1/login-accounts",
        Some(admin),
        Some(
            json!({"contract_version":1,"username":"inspection-console","password":password,
            "credential_id":credential.id}),
        ),
        StatusCode::CREATED,
    )
    .await;
    let login = request(
        client,
        service,
        Method::POST,
        "/api/v1/login",
        None,
        Some(
            json!({"contract_version":1,"tenant_id":"inspection-company-a",
            "username":"inspection-console","password":password}),
        ),
        StatusCode::OK,
    )
    .await;
    (
        login["session"]["token"].as_str().unwrap().to_owned(),
        account["account"]["id"].as_str().unwrap().to_owned(),
    )
}

async fn serve(path: &Path) {
    let database = Arc::new(Database::open(path).unwrap());
    let router = platform_http::create_router(database).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    println!("http://{}", listener.local_addr().unwrap());
    std::io::stdout().flush().unwrap();
    axum::serve(listener, router).await.unwrap();
}
#[tokio::main]
async fn main() {
    let arguments: Vec<_> = std::env::args().collect();
    if arguments.get(1).map(String::as_str) == Some("--serve") {
        serve(Path::new(&arguments[2])).await;
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("real-platform");
    let database = Arc::new(Database::open(&path).unwrap());
    let identity = IdentityStore::new(Arc::new(database.storage().clone()));
    let admin_a = identity
        .bootstrap_tenant("inspection-company-a", "operator-a")
        .unwrap();
    let admin_b = identity
        .bootstrap_tenant("inspection-company-b", "operator-b")
        .unwrap();
    drop((identity, database));
    let client = Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(15))
        .build()
        .unwrap();
    let mut service = Service::start(&path);
    let health = request(
        &client,
        &service,
        Method::GET,
        "/health",
        None,
        None,
        StatusCode::OK,
    )
    .await;
    assert_eq!(health["version"], env!("CARGO_PKG_VERSION"));
    let inspector_a = issue(
        &client,
        &service,
        &admin_a.secret,
        "inspector-a",
        &["credential_admin"],
        false,
    )
    .await;
    let inspector_b = issue(
        &client,
        &service,
        &admin_b.secret,
        "inspector-b",
        &["credential_admin"],
        false,
    )
    .await;
    let writer_a = issue(
        &client,
        &service,
        &admin_a.secret,
        "fixture-owner",
        &["memory_write"],
        true,
    )
    .await;
    let writer_b = issue(
        &client,
        &service,
        &admin_b.secret,
        "fixture-owner",
        &["memory_write"],
        true,
    )
    .await;
    let owner_reader = issue(
        &client,
        &service,
        &admin_a.secret,
        "fixture-owner",
        &["memory_read"],
        true,
    )
    .await;
    let other_reader = issue(
        &client,
        &service,
        &admin_a.secret,
        "other-subject",
        &["memory_read"],
        true,
    )
    .await;
    let (session, account_id) =
        login_session(&client, &service, &admin_a.secret, &inspector_a).await;
    let current = create(&client, &service, &writer_a.secret, "current-a", false).await;
    let expired = create(&client, &service, &writer_a.secret, "expired-a", true).await;
    let foreign = create(&client, &service, &writer_b.secret, "current-b", false).await;
    request(
        &client,
        &service,
        Method::GET,
        &scoped_path(&current),
        Some(&owner_reader.secret),
        None,
        StatusCode::OK,
    )
    .await;
    request(
        &client,
        &service,
        Method::GET,
        &scoped_path(&current),
        Some(&other_reader.secret),
        None,
        StatusCode::NOT_FOUND,
    )
    .await;
    request(
        &client,
        &service,
        Method::GET,
        &scoped_path(&current),
        Some(&inspector_a.secret),
        None,
        StatusCode::FORBIDDEN,
    )
    .await;
    request(
        &client,
        &service,
        Method::GET,
        &scoped_path(&expired),
        Some(&owner_reader.secret),
        None,
        StatusCode::NOT_FOUND,
    )
    .await;
    request(
        &client,
        &service,
        Method::GET,
        "/api/v1/company/memory/workspaces?contract_version=1",
        None,
        None,
        StatusCode::UNAUTHORIZED,
    )
    .await;
    workspaces(
        &client,
        &service,
        &owner_reader.secret,
        StatusCode::FORBIDDEN,
    )
    .await;
    let selection_a = workspaces(&client, &service, &inspector_a.secret, StatusCode::OK).await;
    let selection_b = workspaces(&client, &service, &inspector_b.secret, StatusCode::OK).await;
    let rows_a = selection_a["page"]["workspaces"].as_array().unwrap();
    let rows_b = selection_b["page"]["workspaces"].as_array().unwrap();
    assert_eq!(rows_a.len(), 1);
    assert_eq!(rows_b.len(), 1);
    assert_eq!(rows_a[0]["private_subject_id"], "fixture-owner");
    let workspace_a = rows_a[0]["workspace_id"].as_str().unwrap().to_owned();
    let workspace_b = rows_b[0]["workspace_id"].as_str().unwrap().to_owned();
    assert_ne!(workspace_a, workspace_b);
    let record = inspect(
        &client,
        &service,
        &inspector_a.secret,
        &workspace_a,
        &current,
        StatusCode::OK,
    )
    .await;
    assert!(
        record["entry"]["record"]["payload"]["content"]["primary"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    );
    inspect(
        &client,
        &service,
        &inspector_a.secret,
        &workspace_a,
        &expired,
        StatusCode::OK,
    )
    .await;
    inspect(
        &client,
        &service,
        &inspector_b.secret,
        &workspace_a,
        &current,
        StatusCode::NOT_FOUND,
    )
    .await;
    inspect(
        &client,
        &service,
        &inspector_a.secret,
        &workspace_b,
        &foreign,
        StatusCode::NOT_FOUND,
    )
    .await;
    inspect(
        &client,
        &service,
        &owner_reader.secret,
        &workspace_a,
        &current,
        StatusCode::FORBIDDEN,
    )
    .await;
    inspect(
        &client,
        &service,
        &session,
        &workspace_a,
        &current,
        StatusCode::OK,
    )
    .await;
    request(
        &client,
        &service,
        Method::GET,
        &scoped_path(&current),
        Some(&session),
        None,
        StatusCode::FORBIDDEN,
    )
    .await;
    request(
        &client,
        &service,
        Method::POST,
        "/api/v1/company/memory/read",
        Some(&inspector_a.secret),
        Some(
            json!({"contract_version":1,"company_id":"inspection-company-b",
            "workspace_id":workspace_b,"record_id":foreign}),
        ),
        StatusCode::BAD_REQUEST,
    )
    .await;
    for (key, status) in [
        (&inspector_a.secret, StatusCode::OK),
        (&session, StatusCode::OK),
        (&owner_reader.secret, StatusCode::FORBIDDEN),
    ] {
        request(
            &client,
            &service,
            Method::POST,
            "/api/v1/company/learning/query",
            Some(key),
            Some(
                json!({"contract_version":1,"query":{"kind":"policy","limit":25,
                "max_scanned_records":100}}),
            ),
            status,
        )
        .await;
    }
    for (view, count) in [("retained", 2), ("current", 1)] {
        let result = request(&client, &service, Method::POST, "/api/v1/company/memory/query", Some(&inspector_a.secret),
            Some(json!({"contract_version":1,"workspace_id":workspace_a,"filter":{"view":view,"limit":10,"scan_limit":100}})), StatusCode::OK).await;
        assert_eq!(result["page"]["entries"].as_array().unwrap().len(), count);
    }
    revoke(&client, &service, &admin_a.secret, &writer_a).await;
    request(
        &client,
        &service,
        Method::GET,
        "/api/v1/identity",
        Some(&writer_a.secret),
        None,
        StatusCode::UNAUTHORIZED,
    )
    .await;
    inspect(
        &client,
        &service,
        &inspector_a.secret,
        &workspace_a,
        &current,
        StatusCode::OK,
    )
    .await;
    service.kill();
    service = Service::start(&path);
    request(
        &client,
        &service,
        Method::GET,
        "/api/v1/identity",
        Some(&writer_a.secret),
        None,
        StatusCode::UNAUTHORIZED,
    )
    .await;
    inspect(
        &client,
        &service,
        &session,
        &workspace_a,
        &current,
        StatusCode::OK,
    )
    .await;
    let reopened = workspaces(&client, &service, &inspector_a.secret, StatusCode::OK).await;
    assert_eq!(reopened["page"]["workspaces"].as_array().unwrap().len(), 1);
    inspect(
        &client,
        &service,
        &inspector_a.secret,
        &workspace_a,
        &current,
        StatusCode::OK,
    )
    .await;
    inspect(
        &client,
        &service,
        &inspector_a.secret,
        &workspace_a,
        &expired,
        StatusCode::OK,
    )
    .await;
    request(
        &client,
        &service,
        Method::GET,
        &scoped_path(&current),
        Some(&owner_reader.secret),
        None,
        StatusCode::OK,
    )
    .await;
    request(
        &client,
        &service,
        Method::POST,
        &format!("/api/v1/login-accounts/{account_id}/disable"),
        Some(&admin_a.secret),
        Some(json!({"contract_version":1,"expected_revision":1})),
        StatusCode::OK,
    )
    .await;
    workspaces(&client, &service, &session, StatusCode::UNAUTHORIZED).await;
    for key in [&inspector_a, &owner_reader, &other_reader] {
        revoke(&client, &service, &admin_a.secret, key).await;
    }
    for key in [&inspector_b, &writer_b] {
        revoke(&client, &service, &admin_b.secret, key).await;
    }
    workspaces(
        &client,
        &service,
        &inspector_a.secret,
        StatusCode::UNAUTHORIZED,
    )
    .await;
    revoke(
        &client,
        &service,
        &admin_a.secret,
        &Key {
            secret: String::new(),
            id: admin_a.credential.id.to_string(),
        },
    )
    .await;
    revoke(
        &client,
        &service,
        &admin_b.secret,
        &Key {
            secret: String::new(),
            id: admin_b.credential.id.to_string(),
        },
    )
    .await;
    service.kill();
    let recovered = Arc::new(Database::open(&path).unwrap());
    let authority = IdentityStore::new(Arc::new(recovered.storage().clone()));
    for token in [
        &admin_a.secret,
        &admin_b.secret,
        &inspector_a.secret,
        &inspector_b.secret,
        &writer_a.secret,
        &writer_b.secret,
        &owner_reader.secret,
        &other_reader.secret,
        &session,
    ] {
        assert!(
            matches!(
                authority.authenticate(token),
                Err(qilbee_core::Error::Unauthorized(_))
            ),
            "a temporary credential or session remained authenticated after cleanup"
        );
    }
    drop((authority, recovered));
    println!(
        "{}",
        json!({"version":env!("CARGO_PKG_VERSION"),"actual_http_authority":"passed",
        "two_company_private_subject_isolation":"passed","retained_expired_inspection":"passed",
        "writer_revocation_and_process_kill_recovery":"passed","administrator_login_session":"passed",
        "temporary_credentials_revoked":"passed","temporary_login_disabled":"passed"})
    );
}
