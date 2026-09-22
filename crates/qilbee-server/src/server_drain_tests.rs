use super::*;
use axum::{Router, routing::post};
use std::{sync::mpsc, time::Duration};
use tokio::{net::TcpStream, sync::Notify, time::timeout};

async fn blocked_writer() -> (
    Arc<Server>,
    tempfile::TempDir,
    std::net::SocketAddr,
    Arc<Notify>,
    mpsc::Sender<()>,
) {
    let dir = tempfile::TempDir::new().unwrap();
    let server =
        Arc::new(Server::new(ServerConfig::for_development(dir.path()).http_port(0)).unwrap());
    let database = server.database().clone();
    let started = Arc::new(Notify::new());
    let notified = started.clone();
    let (release, wait) = mpsc::channel();
    let wait = Arc::new(std::sync::Mutex::new(wait));
    let router = Router::new().route(
        "/write",
        post(move || {
            let database = database.clone();
            let notified = notified.clone();
            let wait = wait.clone();
            async move {
                crate::http_work::spawn_blocking(move || {
                    notified.notify_one();
                    wait.lock().unwrap().recv().unwrap();
                    database.create_graph("committed-during-drain").unwrap();
                })
                .await
                .unwrap();
                "committed"
            }
        }),
    );
    server.start_inner(Some(router)).await.unwrap();
    let address = server
        .lifecycle
        .lock()
        .await
        .as_ref()
        .unwrap()
        .address
        .unwrap();
    (server, dir, address, started, release)
}

async fn wait_listener_closed(address: std::net::SocketAddr) {
    timeout(Duration::from_secs(5), async {
        loop {
            if TcpStream::connect(address).await.is_err() {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("Listener did not close admission");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_closes_admission_and_preserves_inflight_write_before_reopen() {
    let (server, dir, address, started, release) = blocked_writer().await;
    let request = tokio::spawn(async move {
        reqwest::Client::new()
            .post(format!("http://{address}/write"))
            .send()
            .await
            .unwrap()
    });
    timeout(Duration::from_secs(5), started.notified())
        .await
        .unwrap();
    let stopping = {
        let server = server.clone();
        tokio::spawn(async move { server.stop().await })
    };
    wait_listener_closed(address).await;
    assert!(
        !stopping.is_finished(),
        "Stop returned while a writer was blocked"
    );
    release.send(()).unwrap();
    assert_eq!(request.await.unwrap().text().await.unwrap(), "committed");
    timeout(Duration::from_secs(5), stopping)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(!server.is_running());
    drop(server);
    let reopened = Database::open(dir.path()).unwrap();
    assert!(
        reopened
            .list_graphs()
            .unwrap()
            .contains(&"committed-during-drain".to_string())
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn disconnected_writer_and_cancelled_stop_do_not_allow_early_restart() {
    let (server, _dir, address, started, release) = blocked_writer().await;
    let request = tokio::spawn(async move {
        reqwest::Client::new()
            .post(format!("http://{address}/write"))
            .send()
            .await
    });
    timeout(Duration::from_secs(5), started.notified())
        .await
        .unwrap();
    request.abort();
    assert!(request.await.unwrap_err().is_cancelled());
    let stopping = {
        let server = server.clone();
        tokio::spawn(async move { server.stop().await })
    };
    wait_listener_closed(address).await;
    assert!(!stopping.is_finished());
    stopping.abort();
    assert!(stopping.await.unwrap_err().is_cancelled());
    assert!(
        server.start().await.is_err(),
        "Cancelled stop must retain the draining lifecycle"
    );
    release.send(()).unwrap();
    timeout(Duration::from_secs(5), server.stop())
        .await
        .unwrap()
        .unwrap();
    assert!(
        server
            .database()
            .list_graphs()
            .unwrap()
            .contains(&"committed-during-drain".to_string())
    );
    server.start().await.unwrap();
    server.stop().await.unwrap();
}

#[tokio::test]
async fn flush_failure_prevents_success_and_restart_until_stop_retry() {
    let dir = tempfile::TempDir::new().unwrap();
    let server = Server::new(ServerConfig::for_development(dir.path()).http_port(0)).unwrap();
    server.start().await.unwrap();
    let result = server
        .stop_with_flush(|_| Err(Error::Storage("injected flush failure".into())))
        .await;
    assert!(matches!(result, Err(Error::Storage(_))));
    assert!(server.start().await.is_err());
    server.stop().await.unwrap();
    server.start().await.unwrap();
    server.stop().await.unwrap();
}

#[tokio::test]
async fn failed_http_task_cannot_report_a_clean_stop_or_restart() {
    let dir = tempfile::TempDir::new().unwrap();
    let server = Server::new(ServerConfig::for_development(dir.path()).http_port(0)).unwrap();
    server.start().await.unwrap();
    server
        .lifecycle
        .lock()
        .await
        .as_ref()
        .unwrap()
        .http
        .as_ref()
        .unwrap()
        .abort();
    assert!(server.stop().await.is_err());
    assert!(server.stop().await.is_err());
    assert!(server.start().await.is_err());
}

#[tokio::test]
async fn worker_panic_prevents_clean_stop_but_an_error_response_does_not() {
    for panic_worker in [false, true] {
        let dir = tempfile::TempDir::new().unwrap();
        let server = Server::new(ServerConfig::for_development(dir.path()).http_port(0)).unwrap();
        let router = Router::new().route(
            "/failure",
            post(move || async move {
                let _ = crate::http_work::spawn_blocking(move || {
                    assert!(!panic_worker, "injected worker failure");
                })
                .await;
                axum::http::StatusCode::INTERNAL_SERVER_ERROR
            }),
        );
        server.start_inner(Some(router)).await.unwrap();
        let address = server
            .lifecycle
            .lock()
            .await
            .as_ref()
            .unwrap()
            .address
            .unwrap();
        let response = reqwest::Client::new()
            .post(format!("http://{address}/failure"))
            .send()
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        );
        assert_eq!(server.stop().await.is_err(), panic_worker);
    }
}

#[cfg(unix)]
#[tokio::test]
async fn sigint_and_sigterm_complete_real_server_shutdown() {
    const CHILD: &str = "QILBEEDB_SIGNAL_DRAIN_FIXTURE";
    if let Some(directory) = std::env::var_os(CHILD) {
        let directory = std::path::PathBuf::from(directory);
        let server =
            Server::new(ServerConfig::for_development(directory.join("database")).http_port(0))
                .unwrap();
        server.start().await.unwrap();
        server.database().create_graph("signal-fixture").unwrap();
        let signal = shutdown_signal();
        tokio::pin!(signal);
        // Poll the signal future before announcing readiness so the parent cannot
        // race OS handler registration and accidentally exercise default SIGTERM.
        tokio::select! {
            biased;
            _ = &mut signal => panic!("Signal arrived before fixture readiness"),
            _ = tokio::task::yield_now() => {},
        }
        std::fs::write(directory.join("ready"), b"ready").unwrap();
        signal.await.unwrap();
        server.stop().await.unwrap();
        std::fs::write(directory.join("stopped"), b"drained and flushed").unwrap();
        return;
    }
    for signal in ["-INT", "-TERM"] {
        let directory = tempfile::TempDir::new().unwrap();
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "server::drain_tests::sigint_and_sigterm_complete_real_server_shutdown",
                "--nocapture",
            ])
            .env(CHILD, directory.path())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while !directory.path().join("ready").exists() && std::time::Instant::now() < deadline {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        if directory.path().join("ready").exists() {
            assert!(
                std::process::Command::new("kill")
                    .args([signal, &child.id().to_string()])
                    .status()
                    .unwrap()
                    .success()
            );
            while child.try_wait().unwrap().is_none() && std::time::Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        if child.try_wait().unwrap().is_none() {
            child.kill().unwrap();
        }
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(directory.path().join("stopped").exists());
        let reopened = Database::open(directory.path().join("database")).unwrap();
        assert!(
            reopened
                .list_graphs()
                .unwrap()
                .contains(&"signal-fixture".to_string())
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_does_not_accept_another_http1_request_on_a_busy_connection() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let (server, _dir, address, started, release) = blocked_writer().await;
    let mut socket = TcpStream::connect(address).await.unwrap();
    let request = b"POST /write HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\nConnection: keep-alive\r\n\r\n";
    socket.write_all(request).await.unwrap();
    timeout(Duration::from_secs(5), started.notified())
        .await
        .unwrap();
    let stopping = {
        let server = server.clone();
        tokio::spawn(async move { server.stop().await })
    };
    wait_listener_closed(address).await;
    // A transport error here is also a valid refusal; no second operation may run.
    let _ = socket.write_all(request).await;
    release.send(()).unwrap();
    drop(release);
    let mut response = Vec::new();
    timeout(Duration::from_secs(5), socket.read_to_end(&mut response))
        .await
        .unwrap()
        .unwrap();
    timeout(Duration::from_secs(5), stopping)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(
        response
            .windows(b"HTTP/1.1".len())
            .filter(|part| *part == b"HTTP/1.1")
            .count(),
        1
    );
    assert!(String::from_utf8(response).unwrap().contains("committed"));
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn forced_drain_interruption_reconciles_lost_http_write_with_original_identity() {
    use crate::security::identity::{
        Capability, CredentialSpec, IdentityStore, ResourceScope, Visibility,
    };
    use serde_json::{Value, json};
    const CHILD: &str = "QILBEEDB_LOST_RESPONSE_DRAIN_FIXTURE";
    if let Some(directory) = std::env::var_os(CHILD) {
        let directory = std::path::PathBuf::from(directory);
        let server =
            Server::new(ServerConfig::for_development(directory.join("database")).http_port(0))
                .unwrap();
        let identity = IdentityStore::new(Arc::new(server.database().storage().clone()));
        let admin = identity
            .bootstrap_tenant("fixture-company", "operator")
            .unwrap();
        let token = identity
            .issue(
                &admin.secret,
                CredentialSpec {
                    scope_policy: None,
                    subject_id: "writer".into(),
                    capabilities: [Capability::MemoryRead, Capability::MemoryWrite].into(),
                    grants: vec![ResourceScope {
                        project_id: "project".into(),
                        mission_id: None,
                        agent_id: "agent".into(),
                        visibility: Visibility::Shared,
                    }],
                    expires_at_millis: None,
                },
            )
            .unwrap()
            .secret;
        let router = http_server::create_router(server.database().clone())
            .unwrap()
            .layer(axum::middleware::from_fn_with_state(
                directory.clone(),
                |axum::extract::State(directory): axum::extract::State<std::path::PathBuf>,
                 request: axum::extract::Request,
                 next: axum::middleware::Next| async move {
                    let hold = request.uri().path() == "/api/v1/memory/commands";
                    let response = next.run(request).await;
                    if hold && response.status().is_success() {
                        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
                            .await
                            .unwrap();
                        std::fs::write(directory.join("committed.tmp"), &bytes).unwrap();
                        std::fs::rename(
                            directory.join("committed.tmp"),
                            directory.join("committed"),
                        )
                        .unwrap();
                        // Simulate an undelivered response after the real authorized
                        // endpoint commits. No replacement persistence or receipt.
                        std::future::pending::<axum::response::Response>().await
                    } else {
                        response
                    }
                },
            ));
        server.start_inner(Some(router)).await.unwrap();
        let address = server
            .lifecycle
            .lock()
            .await
            .as_ref()
            .unwrap()
            .address
            .unwrap();
        let signal = shutdown_signal();
        tokio::pin!(signal);
        tokio::select! { biased;
            _ = &mut signal => panic!("Premature signal"),
            _ = tokio::task::yield_now() => {},
        }
        std::fs::write(
            directory.join("ready.tmp"),
            json!({"address":address.to_string(),"token":token}).to_string(),
        )
        .unwrap();
        std::fs::rename(directory.join("ready.tmp"), directory.join("ready")).unwrap();
        signal.await.unwrap();
        std::fs::write(directory.join("draining"), b"requested").unwrap();
        server.stop().await.unwrap();
        panic!("A held response must not finish drainage before force-kill");
    }
    struct OwnedChild(std::process::Child);
    impl Drop for OwnedChild {
        fn drop(&mut self) {
            if self.0.try_wait().ok().flatten().is_none() {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
    }
    async fn marker(path: &std::path::Path) {
        timeout(Duration::from_secs(15), async {
            while !path.exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("Child fixture did not reach its checkpoint");
    }
    let dir = tempfile::TempDir::new().unwrap();
    let mut child = OwnedChild(std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "server::drain_tests::forced_drain_interruption_reconciles_lost_http_write_with_original_identity", "--nocapture"])
        .env(CHILD, dir.path()).stdout(std::process::Stdio::null()).spawn().unwrap());
    marker(&dir.path().join("ready")).await;
    let ready: Value =
        serde_json::from_slice(&std::fs::read(dir.path().join("ready")).unwrap()).unwrap();
    let token = ready["token"].as_str().unwrap().to_owned();
    let body = json!({"contract_version":1,"idempotency_key":"original-intent",
        "scope":{"project_id":"project","mission_id":null,"agent_id":"agent","visibility":"shared"},
        "operation":{"type":"create","record":{"episode_type":"Observation","event_time_millis":1700000000000_i64,
            "content":{"primary":"Preserved through unknown outcome"},"tags":[],"metadata":{}}}});
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap();
    let request = {
        let client = client.clone();
        let token = token.clone();
        let body = body.clone();
        let url = format!(
            "http://{}/api/v1/memory/commands",
            ready["address"].as_str().unwrap()
        );
        tokio::spawn(async move { client.post(url).bearer_auth(token).json(&body).send().await })
    };
    marker(&dir.path().join("committed")).await;
    let original: Value =
        serde_json::from_slice(&std::fs::read(dir.path().join("committed")).unwrap()).unwrap();
    assert!(
        std::process::Command::new("kill")
            .args(["-TERM", &child.0.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    marker(&dir.path().join("draining")).await;
    wait_listener_closed(ready["address"].as_str().unwrap().parse().unwrap()).await;
    assert!(child.0.try_wait().unwrap().is_none());
    child.0.kill().unwrap();
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(child.0.wait().unwrap().signal(), Some(9));
    assert!(
        request.await.unwrap().is_err(),
        "The committed response must remain undelivered"
    );
    let server =
        Server::new(ServerConfig::for_development(dir.path().join("database")).http_port(0))
            .unwrap();
    server.start().await.unwrap();
    let address = server
        .lifecycle
        .lock()
        .await
        .as_ref()
        .unwrap()
        .address
        .unwrap();
    let url = format!("http://{address}/api/v1/memory/commands");
    let replay = client
        .post(&url)
        .bearer_auth(&token)
        .json(&body)
        .send()
        .await
        .unwrap();
    assert!(replay.status().is_success());
    assert_eq!(replay.json::<Value>().await.unwrap(), original);
    let mut conflicting = body;
    conflicting["operation"]["record"]["content"]["primary"] = json!("Different intent");
    assert_eq!(
        client
            .post(&url)
            .bearer_auth(&token)
            .json(&conflicting)
            .send()
            .await
            .unwrap()
            .status(),
        axum::http::StatusCode::CONFLICT
    );
    server.stop().await.unwrap();
}
