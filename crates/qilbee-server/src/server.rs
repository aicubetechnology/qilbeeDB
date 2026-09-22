//! Server implementation

use crate::config::ServerConfig;
use crate::http_server;
use crate::http_work::HttpWork;
use crate::security::{BootstrapService, UserService};
use qilbee_core::{Error, Result};
use qilbee_graph::Database;
use std::sync::Arc;
use tokio::sync::{Mutex, oneshot};
use tokio::task::JoinHandle;
use tracing::{info, warn};

/// QilbeeDB Server
pub struct Server {
    /// Server configuration
    config: ServerConfig,

    /// Database instance
    database: Arc<Database>,

    /// Running state
    running: std::sync::atomic::AtomicBool,

    /// Retained across cancellation of a caller waiting for stop.
    lifecycle: Mutex<Option<Runtime>>,
}

struct Runtime {
    shutdown: Option<oneshot::Sender<()>>,
    http: Option<JoinHandle<std::io::Result<()>>>,
    work: Arc<HttpWork>,
    failure: Option<String>,
    #[cfg(test)]
    address: Option<std::net::SocketAddr>,
}

impl Server {
    /// Create a new server instance
    pub fn new(config: ServerConfig) -> Result<Self> {
        let database = Database::open(&config.data_dir)?;

        // Run bootstrap if authentication is enabled
        if config.enable_legacy_http && config.auth_enabled {
            info!("Authentication is enabled, checking bootstrap status...");
            let user_service = Arc::new(UserService::new());
            let bootstrap = BootstrapService::new(config.data_dir.clone(), user_service.clone());

            // Run bootstrap if needed
            if bootstrap.is_bootstrap_required()? {
                info!("Initial bootstrap required");
                bootstrap.run_auto()?;
            } else {
                info!("System already bootstrapped");
            }
        }

        Ok(Self {
            config,
            database: Arc::new(database),
            running: std::sync::atomic::AtomicBool::new(false),
            lifecycle: Mutex::new(None),
        })
    }

    /// Get the configuration
    pub fn config(&self) -> &ServerConfig {
        &self.config
    }

    /// Get the database
    pub fn database(&self) -> &Arc<Database> {
        &self.database
    }

    /// Check if server is running
    pub fn is_running(&self) -> bool {
        self.running.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Start the server. A draining or failed lifecycle cannot be restarted.
    pub async fn start(&self) -> Result<()> {
        self.start_inner(None).await
    }

    async fn start_inner(&self, router_override: Option<axum::Router>) -> Result<()> {
        let mut lifecycle = self.lifecycle.lock().await;
        if lifecycle.is_some() {
            return Err(Error::Configuration(
                "Server is running or shutdown is incomplete".into(),
            ));
        }
        let work = Arc::new(HttpWork::default());
        let mut runtime = Runtime {
            shutdown: None,
            http: None,
            work: work.clone(),
            failure: None,
            #[cfg(test)]
            address: None,
        };
        if self.config.enable_http {
            let router = if let Some(router) = router_override {
                router
            } else if self.config.enable_legacy_http {
                warn!("Legacy HTTP mode enabled; platform credential and scope guarantees do not apply");
                http_server::create_legacy_router(self.database.clone())?
            } else {
                http_server::create_router(self.database.clone())?
            }.layer(axum::middleware::from_fn_with_state(work, crate::http_work::track));
            let listener = tokio::net::TcpListener::bind(("0.0.0.0", self.config.http_port))
                .await
                .map_err(Error::Io)?;
            let address = listener.local_addr().map_err(Error::Io)?;
            info!("HTTP server listening on {}", address);
            let (shutdown, signal) = oneshot::channel();
            runtime.shutdown = Some(shutdown);
            runtime.http = Some(tokio::spawn(async move {
                axum::serve(listener, router)
                    .with_graceful_shutdown(async {
                        let _ = signal.await;
                    })
                    .await
            }));
            #[cfg(test)]
            {
                runtime.address = Some(address);
            }
        }
        *lifecycle = Some(runtime);
        self.running
            .store(true, std::sync::atomic::Ordering::SeqCst);
        info!("QilbeeDB server started successfully");
        Ok(())
    }

    /// Close HTTP admission, await accepted requests and blocking work, then flush.
    /// There is no internal forced-abort deadline. Cancelling this waiter retains
    /// the draining lifecycle so another stop call can finish it safely.
    pub async fn stop(&self) -> Result<()> {
        self.stop_with_flush(Database::flush).await
    }

    async fn stop_with_flush(&self, flush: impl FnOnce(&Database) -> Result<()>) -> Result<()> {
        let mut lifecycle = self.lifecycle.lock().await;
        let runtime = lifecycle
            .as_mut()
            .ok_or_else(|| Error::Configuration("Server not running".into()))?;
        self.running
            .store(false, std::sync::atomic::Ordering::SeqCst);
        if let Some(shutdown) = runtime.shutdown.take() {
            info!("Closing HTTP admission and waiting for accepted work");
            let _ = shutdown.send(());
        }
        if let Some(http) = runtime.http.as_mut() {
            let result = http.await;
            // Retain the handle until it finishes. Dropping a stop waiter must
            // neither detach drainage nor poll a completed handle a second time.
            runtime.http = None;
            runtime.failure = match result {
                Ok(Ok(())) => None,
                Ok(Err(error)) => Some(format!("HTTP shutdown failed: {error}")),
                Err(error) => Some(format!("HTTP task failed: {error}")),
            };
        }
        if let Some(failure) = &runtime.failure {
            return Err(Error::Internal(failure.clone()));
        }
        runtime.work.wait_idle().await;
        if runtime.work.panicked() {
            let failure =
                "An HTTP request or blocking worker panicked during this lifecycle".to_string();
            runtime.failure = Some(failure.clone());
            return Err(Error::Internal(failure));
        }
        // A flush failure retains the stopped-admission lifecycle for a retry;
        // restart is permitted only after the complete stop succeeds.
        flush(&self.database)?;
        *lifecycle = None;
        info!("QilbeeDB server stopped after HTTP drainage and flush");
        Ok(())
    }

    /// Get server version
    pub fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }
}

/// Wait for the operating system's normal termination signals.
pub async fn shutdown_signal() -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result,
            signal = terminate.recv() => signal.ok_or_else(||
                std::io::Error::other("Termination signal stream closed")),
        }
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await
    }
}

#[cfg(test)]
#[path = "server_drain_tests.rs"]
mod drain_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_server() -> (Server, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let config = ServerConfig::for_development(temp_dir.path()).http_port(0);
        let server = Server::new(config).unwrap();
        (server, temp_dir)
    }

    #[test]
    fn test_server_creation() {
        let (server, _dir) = create_test_server();
        assert!(!server.is_running());
    }

    #[test]
    fn test_server_config() {
        let (server, _dir) = create_test_server();
        assert!(server.config().enable_bolt);
        assert!(server.config().enable_http);
    }

    #[test]
    fn test_server_database() {
        let (server, _dir) = create_test_server();
        let _db = server.database();
    }

    #[tokio::test]
    async fn test_server_start_stop() {
        let (server, _dir) = create_test_server();

        server.start().await.unwrap();
        assert!(server.is_running());

        server.stop().await.unwrap();
        assert!(!server.is_running());
    }
}
