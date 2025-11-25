//! Server configuration

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Data directory
    pub data_dir: PathBuf,

    /// Bolt protocol port
    pub bolt_port: u16,

    /// HTTP API port
    pub http_port: u16,

    /// Enable Bolt protocol
    pub enable_bolt: bool,

    /// Enable HTTP API
    pub enable_http: bool,

    /// Maximum concurrent connections
    pub max_connections: usize,

    /// Query timeout in seconds
    pub query_timeout_secs: u64,

    /// Enable authentication
    pub auth_enabled: bool,

    /// Log level
    pub log_level: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            data_dir: PathBuf::from("./data"),
            bolt_port: 7687,
            http_port: 7474,
            enable_bolt: true,
            enable_http: true,
            max_connections: 1000,
            query_timeout_secs: 300,
            auth_enabled: false,
            log_level: "info".to_string(),
        }
    }
}

impl ServerConfig {
    /// Create a new configuration
    pub fn new<P: Into<PathBuf>>(data_dir: P) -> Self {
        Self {
            data_dir: data_dir.into(),
            ..Default::default()
        }
    }

    /// Create configuration for development
    pub fn for_development<P: Into<PathBuf>>(data_dir: P) -> Self {
        Self {
            data_dir: data_dir.into(),
            log_level: "debug".to_string(),
            auth_enabled: false,
            ..Default::default()
        }
    }

    /// Create configuration for production
    pub fn for_production<P: Into<PathBuf>>(data_dir: P) -> Self {
        Self {
            data_dir: data_dir.into(),
            log_level: "info".to_string(),
            auth_enabled: true,
            max_connections: 10000,
            ..Default::default()
        }
    }

    /// Builder: set Bolt port
    pub fn bolt_port(mut self, port: u16) -> Self {
        self.bolt_port = port;
        self
    }

    /// Builder: set HTTP port
    pub fn http_port(mut self, port: u16) -> Self {
        self.http_port = port;
        self
    }

    /// Builder: disable Bolt
    pub fn disable_bolt(mut self) -> Self {
        self.enable_bolt = false;
        self
    }

    /// Builder: disable HTTP
    pub fn disable_http(mut self) -> Self {
        self.enable_http = false;
        self
    }

    /// Builder: enable auth
    pub fn with_auth(mut self) -> Self {
        self.auth_enabled = true;
        self
    }

    /// Builder: set log level
    pub fn log_level(mut self, level: &str) -> Self {
        self.log_level = level.to_string();
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ServerConfig::default();
        assert_eq!(config.bolt_port, 7687);
        assert_eq!(config.http_port, 7474);
        assert!(config.enable_bolt);
        assert!(config.enable_http);
    }

    #[test]
    fn test_builder() {
        let config = ServerConfig::new("/data")
            .bolt_port(7688)
            .http_port(7475)
            .with_auth()
            .log_level("debug");

        assert_eq!(config.bolt_port, 7688);
        assert_eq!(config.http_port, 7475);
        assert!(config.auth_enabled);
        assert_eq!(config.log_level, "debug");
    }
}
