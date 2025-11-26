//! Server implementation

use crate::config::ServerConfig;
use crate::http_server;
use crate::security::{UserService, BootstrapService};
use qilbee_core::{Error, Result};
use qilbee_graph::Database;
use std::sync::Arc;
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

    /// HTTP server handle
    http_handle: std::sync::Mutex<Option<JoinHandle<()>>>,
}

impl Server {
    /// Create a new server instance
    pub fn new(config: ServerConfig) -> Result<Self> {
        let database = Database::open(&config.data_dir)?;

        // Run bootstrap if authentication is enabled
        if config.auth_enabled {
            info!("Authentication is enabled, checking bootstrap status...");
            let user_service = Arc::new(UserService::new());
            let bootstrap = BootstrapService::new(
                config.data_dir.clone(),
                user_service.clone(),
            );

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
            http_handle: std::sync::Mutex::new(None),
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

    /// Start the server
    pub async fn start(&self) -> Result<()> {
        if self.is_running() {
            return Err(Error::Configuration("Server already running".to_string()));
        }

        info!("Starting QilbeeDB server...");
        info!("Data directory: {:?}", self.config.data_dir);

        if self.config.enable_bolt {
            info!("Bolt protocol enabled on port {}", self.config.bolt_port);
            // TODO: Start Bolt listener
        }

        if self.config.enable_http {
            info!("HTTP API enabled on port {}", self.config.http_port);

            // Start HTTP server
            let router = http_server::create_router(Arc::clone(&self.database));
            let addr = format!("0.0.0.0:{}", self.config.http_port);
            let listener = tokio::net::TcpListener::bind(&addr)
                .await
                .map_err(|e| Error::Io(e))?;

            info!("HTTP server listening on {}", addr);

            // Spawn HTTP server task
            let handle = tokio::spawn(async move {
                if let Err(e) = axum::serve(listener, router).await {
                    warn!("HTTP server error: {}", e);
                }
            });

            *self.http_handle.lock().unwrap() = Some(handle);
        }

        self.running
            .store(true, std::sync::atomic::Ordering::SeqCst);

        info!("QilbeeDB server started successfully");
        Ok(())
    }

    /// Stop the server
    pub async fn stop(&self) -> Result<()> {
        if !self.is_running() {
            return Err(Error::Configuration("Server not running".to_string()));
        }

        info!("Stopping QilbeeDB server...");

        // Flush data
        self.database.flush()?;

        self.running
            .store(false, std::sync::atomic::Ordering::SeqCst);

        info!("QilbeeDB server stopped");
        Ok(())
    }

    /// Get server version
    pub fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_server() -> (Server, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let config = ServerConfig::for_development(temp_dir.path());
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
