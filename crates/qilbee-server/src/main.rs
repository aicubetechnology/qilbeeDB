//! QilbeeDB Server Entry Point

use qilbee_server::{Server, ServerConfig};
use std::path::PathBuf;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match qilbee_storage::verification::writer_exclusion_helper_command(&args) {
        Ok(true) => return,
        Ok(false) => {}
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
    if args.first().map(String::as_str) == Some("health-check") {
        if args.len() != 1 || qilbee_server::health_probe::check().is_err() {
            eprintln!("QilbeeDB health check failed");
            std::process::exit(1);
        }
        return;
    }
    match qilbee_server::operator::bootstrap_command(&args) {
        Ok(Some(result)) => {
            // This is the explicit operator result, not an application log.
            println!("{}", result);
            return;
        }
        Ok(None) => {}
        Err(error) => {
            eprintln!("Bootstrap failed: {}", error);
            std::process::exit(1);
        }
    }
    match qilbee_server::store_verification::verify_store_command(&args) {
        Ok(Some(report)) => {
            // Complete verification report; a failure never prints a partial one.
            println!("{}", report);
            return;
        }
        Ok(None) => {}
        Err(error) => {
            eprintln!("Store verification failed: {}", error);
            std::process::exit(1);
        }
    }

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

    let signal_failed = match qilbee_server::server::shutdown_signal().await {
        Ok(()) => {
            info!("Received shutdown signal");
            false
        }
        Err(e) => {
            error!("Failed to listen for shutdown signal: {}", e);
            true
        }
    };

    // Stop server
    if let Err(e) = server.stop().await {
        error!("Error during shutdown: {}", e);
        std::process::exit(1);
    }

    if signal_failed {
        std::process::exit(1);
    }
    info!("Goodbye!");
}
