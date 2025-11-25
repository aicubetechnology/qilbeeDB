//! QilbeeDB Server Entry Point

use qilbee_server::{Server, ServerConfig};
use std::path::PathBuf;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    info!("QilbeeDB v{}", Server::version());
    info!("Agent-first Graph Database by AICUBE TECHNOLOGY LLC");

    // Parse configuration (simple args for now)
    let data_dir = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("./data"));

    let config = ServerConfig::new(&data_dir);

    // Create and start server
    let server = match Server::new(config) {
        Ok(s) => s,
        Err(e) => {
            error!("Failed to create server: {}", e);
            std::process::exit(1);
        }
    };

    if let Err(e) = server.start().await {
        error!("Failed to start server: {}", e);
        std::process::exit(1);
    }

    // Wait for shutdown signal
    info!("Press Ctrl+C to stop the server");

    match tokio::signal::ctrl_c().await {
        Ok(()) => {
            info!("Received shutdown signal");
        }
        Err(e) => {
            error!("Failed to listen for shutdown signal: {}", e);
        }
    }

    // Stop server
    if let Err(e) = server.stop().await {
        error!("Error during shutdown: {}", e);
    }

    info!("Goodbye!");
}
