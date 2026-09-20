use crate::http_server::create_legacy_router as create_router;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use qilbee_graph::Database;
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};
use tempfile::TempDir;
use tower::Service;

fn router(path: &Path) -> Router {
    create_router(Arc::new(Database::open_for_testing(path).unwrap())).unwrap()
}

async fn request(
    router: &Router,
    method: &str,
    path: &str,
    token: &str,
    body: Value,
) -> (StatusCode, Value) {
    let response = router
        .clone()
        .call(
            Request::builder()
                .method(method)
                .uri(path)
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

async fn login(router: &Router) -> String {
    let (status, body) = request(
        router,
        "POST",
        "/api/v1/auth/login",
        "",
        json!({
            "username": "admin", "password": "SecureAdmin@123!"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["access_token"].as_str().unwrap().to_owned()
}

async fn store(router: &Router, token: &str, agent: &str, text: &str) -> String {
    let (status, body) = request(
        router,
        "POST",
        &format!("/memory/{agent}/episodes"),
        token,
        json!({"agentId": agent, "episodeType": "observation", "content": {"primary": text}}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["episodeId"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn http_durable_memory_survives_router_and_database_reopen() {
    let dir = TempDir::new().unwrap();
    let id = {
        let app = router(dir.path());
        let token = login(&app).await;
        store(&app, &token, "agent-a", "remember across restarts").await
    };
    let app = router(dir.path());
    let token = login(&app).await;
    let (status, body) = request(
        &app,
        "GET",
        &format!("/memory/agent-a/episodes/{id}"),
        &token,
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["content"]["observation"], "remember across restarts");
    let (status, _) = request(
        &app,
        "GET",
        &format!("/memory/agent-b/episodes/{id}"),
        &token,
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, stats) = request(
        &app,
        "GET",
        "/memory/agent-a/statistics",
        &token,
        Value::Null,
    )
    .await;
    assert_eq!(stats["totalEpisodes"], 1);
}

#[tokio::test]
async fn http_durable_lookup_reaches_episodes_older_than_recent_page() {
    let dir = TempDir::new().unwrap();
    let app = router(dir.path());
    let token = login(&app).await;
    let first = store(&app, &token, "agent-a", "first record").await;
    for i in 0..105 {
        store(&app, &token, "agent-a", &format!("later {i}")).await;
    }
    let (status, body) = request(
        &app,
        "GET",
        &format!("/memory/agent-a/episodes/{first}"),
        &token,
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

#[tokio::test]
async fn http_durable_invalid_id_is_bad_request_and_clear_survives_reopen() {
    let dir = TempDir::new().unwrap();
    {
        let app = router(dir.path());
        let token = login(&app).await;
        store(&app, &token, "agent-a", "remove me").await;
        let (status, _) = request(
            &app,
            "GET",
            "/memory/agent-a/episodes/not-a-uuid",
            &token,
            Value::Null,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, _) = request(&app, "DELETE", "/memory/agent-a", &token, Value::Null).await;
        assert_eq!(status, StatusCode::OK);
    }
    let app = router(dir.path());
    let token = login(&app).await;
    let (_, body) = request(
        &app,
        "GET",
        "/memory/agent-a/episodes/recent",
        &token,
        Value::Null,
    )
    .await;
    assert_eq!(body["episodes"], json!([]));
}

#[tokio::test]
async fn http_durable_payload_keeps_event_time_metadata_and_structured_content() {
    let dir = TempDir::new().unwrap();
    let id = {
        let app = router(dir.path());
        let token = login(&app).await;
        let (status, body) = request(&app, "POST", "/memory/agent-a/episodes", &token, json!({
            "agentId": "agent-a", "episodeType": "observation", "eventTime": 1700000000000_i64,
            "content": {"primary": "sensor reading", "context": "laboratory", "data": {"values": [1, 2], "verified": false}},
            "metadata": {"source": "sensor-a", "sequence": 42}
        })).await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        body["episodeId"].as_str().unwrap().to_owned()
    };
    let app = router(dir.path());
    let token = login(&app).await;
    let (status, body) = request(
        &app,
        "GET",
        &format!("/memory/agent-a/episodes/{id}"),
        &token,
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["eventTime"], 1700000000000_i64);
    assert_eq!(body["content"]["context"], "laboratory");
    assert_eq!(
        body["content"]["data"],
        json!({"values": [1, 2], "verified": false})
    );
    assert_eq!(
        body["metadata"],
        json!({"source": "sensor-a", "sequence": 42})
    );
}

#[tokio::test]
async fn http_durable_mismatched_payload_agent_is_rejected() {
    let dir = TempDir::new().unwrap();
    let app = router(dir.path());
    let token = login(&app).await;
    let (status, _) = request(
        &app,
        "POST",
        "/memory/agent-a/episodes",
        &token,
        json!({
            "agentId": "agent-b", "episodeType": "observation", "content": {"primary": "foreign"}
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[test]
fn http_durable_storage_open_failure_is_not_replaced_by_volatile_memory() {
    let dir = TempDir::new().unwrap();
    let database = Arc::new(Database::open_for_testing(dir.path()).unwrap());
    std::fs::write(dir.path().join("agent-memory"), b"not a directory").unwrap();
    assert!(create_router(database).is_err());
}

#[test]
#[ignore = "Subprocess fixture, invoked only by the abrupt-restart contract test"]
fn http_durable_server_child() {
    use std::io::Write;
    let path = std::env::var("QILBEE_TEST_CRASH_DATA").expect("Test data path is required");
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let app = router(Path::new(&path));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        println!("HTTP_CRASH_READY {}", listener.local_addr().unwrap().port());
        std::io::stdout().flush().unwrap();
        axum::serve(listener, app).await.unwrap();
    });
}

pub(crate) struct TestServerProcess(pub(crate) std::process::Child);

impl Drop for TestServerProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn crash_server(path: &Path) -> (TestServerProcess, u16) {
    crash_server_for(path, "http_server_tests::http_durable_server_child")
}

pub(crate) fn crash_server_for(path: &Path, fixture: &str) -> (TestServerProcess, u16) {
    use std::io::{BufRead, BufReader};
    let mut process = TestServerProcess(
        std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", fixture, "--nocapture"])
            .env("QILBEE_TEST_CRASH_DATA", path)
            .env("OPENAI_API_KEY", "")
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let stdout = process.0.stdout.take().unwrap();
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let line = line.unwrap();
            if let Some(port) = line.strip_prefix("HTTP_CRASH_READY ") {
                let _ = sender.send(port.parse::<u16>().unwrap());
                return;
            }
        }
    });
    let port = receiver
        .recv_timeout(std::time::Duration::from_secs(30))
        .expect("HTTP child did not start");
    (process, port)
}

pub(crate) fn wire_request(
    port: u16,
    method: &str,
    path: &str,
    token: &str,
    body: Value,
) -> (u16, Value) {
    use std::io::{Read, Write};
    let mut socket = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    socket
        .set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .unwrap();
    socket
        .set_write_timeout(Some(std::time::Duration::from_secs(10)))
        .unwrap();
    let body = serde_json::to_vec(&body).unwrap();
    write!(socket, "{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nAuthorization: Bearer {token}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).unwrap();
    socket.write_all(&body).unwrap();
    let mut response = Vec::new();
    socket.read_to_end(&mut response).unwrap();
    let boundary = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .unwrap();
    let header = std::str::from_utf8(&response[..boundary]).unwrap();
    let status = header.split_whitespace().nth(1).unwrap().parse().unwrap();
    (
        status,
        serde_json::from_slice(&response[boundary + 4..]).unwrap(),
    )
}

#[test]
fn http_durable_acknowledged_writes_survive_abrupt_server_termination() {
    let dir = TempDir::new().unwrap();
    let (mut server, port) = crash_server(dir.path());
    let credentials = json!({"username": "admin", "password": "SecureAdmin@123!"});
    let (status, login) = wire_request(port, "POST", "/api/v1/auth/login", "", credentials.clone());
    assert_eq!(status, 200);
    let token = login["access_token"].as_str().unwrap();
    let mut acknowledged = Vec::new();
    for sequence in 0..20 {
        let (status, receipt) = wire_request(
            port,
            "POST",
            "/memory/crash-agent/episodes",
            token,
            json!({
                "agentId": "crash-agent", "episodeType": "observation",
                "content": {"primary": format!("acknowledged-{sequence}"), "data": {"sequence": sequence}}
            }),
        );
        assert_eq!(status, 201, "{receipt}");
        acknowledged.push(receipt["episodeId"].as_str().unwrap().to_owned());
    }
    server.0.kill().unwrap();
    server.0.wait().unwrap();
    let (_restarted, port) = crash_server(dir.path());
    let (status, login) = wire_request(port, "POST", "/api/v1/auth/login", "", credentials);
    assert_eq!(status, 200);
    let token = login["access_token"].as_str().unwrap();
    for (sequence, id) in acknowledged.iter().enumerate() {
        let (status, episode) = wire_request(
            port,
            "GET",
            &format!("/memory/crash-agent/episodes/{id}"),
            token,
            Value::Null,
        );
        assert_eq!(status, 200, "{episode}");
        assert_eq!(episode["content"]["data"]["sequence"], sequence);
    }
    let (status, stats) = wire_request(
        port,
        "GET",
        "/memory/crash-agent/statistics",
        token,
        Value::Null,
    );
    assert_eq!(status, 200);
    assert_eq!(stats["totalEpisodes"], acknowledged.len());
}
