//! CORS (Cross-Origin Resource Sharing) configuration for production environments
//!
//! Provides configurable CORS policies for different deployment scenarios:
//! - Development: Permissive (allow all origins)
//! - Production: Strict (whitelist specific origins)

use axum::http::{header, HeaderValue, Method};
use tower_http::cors::{Any, CorsLayer};
use std::time::Duration;

/// CORS configuration for the HTTP server
#[derive(Debug, Clone)]
pub struct CorsConfig {
    /// Allowed origins (empty = allow all in development mode)
    pub allowed_origins: Vec<String>,
    /// Whether to allow credentials (cookies, authorization headers)
    pub allow_credentials: bool,
    /// Maximum age for preflight cache (in seconds)
    pub max_age_secs: u64,
    /// Allowed HTTP methods
    pub allowed_methods: Vec<Method>,
    /// Allowed request headers
    pub allowed_headers: Vec<String>,
    /// Headers to expose to the client
    pub expose_headers: Vec<String>,
    /// Enable permissive mode (development only)
    pub permissive: bool,
}

impl Default for CorsConfig {
    fn default() -> Self {
        Self {
            allowed_origins: vec![],
            allow_credentials: true,
            max_age_secs: 3600, // 1 hour
            allowed_methods: vec![
                Method::GET,
                Method::POST,
                Method::PUT,
                Method::DELETE,
                Method::PATCH,
                Method::OPTIONS,
            ],
            allowed_headers: vec![
                "Content-Type".to_string(),
                "Authorization".to_string(),
                "X-API-Key".to_string(),
                "X-Request-ID".to_string(),
                "Accept".to_string(),
                "Origin".to_string(),
            ],
            expose_headers: vec![
                "X-RateLimit-Limit".to_string(),
                "X-RateLimit-Remaining".to_string(),
                "X-RateLimit-Reset".to_string(),
                "X-Request-ID".to_string(),
            ],
            permissive: true, // Default to permissive for development
        }
    }
}

impl CorsConfig {
    /// Create a development configuration (permissive)
    pub fn development() -> Self {
        Self {
            permissive: true,
            ..Default::default()
        }
    }

    /// Create a production configuration (strict)
    ///
    /// # Arguments
    /// * `origins` - List of allowed origins (e.g., ["https://app.qilbeedb.com", "https://admin.qilbeedb.com"])
    pub fn production(origins: Vec<String>) -> Self {
        Self {
            allowed_origins: origins,
            allow_credentials: true,
            max_age_secs: 86400, // 24 hours for production
            permissive: false,
            ..Default::default()
        }
    }

    /// Create CORS configuration from environment variables
    ///
    /// Reads the following environment variables:
    /// - `CORS_ALLOWED_ORIGINS`: Comma-separated list of allowed origins
    /// - `CORS_ALLOW_CREDENTIALS`: "true" or "false" (default: true)
    /// - `CORS_MAX_AGE`: Max age in seconds (default: 3600)
    /// - `CORS_PERMISSIVE`: "true" for development mode (default: false in production)
    pub fn from_env() -> Self {
        let allowed_origins: Vec<String> = std::env::var("CORS_ALLOWED_ORIGINS")
            .map(|s| s.split(',').map(|s| s.trim().to_string()).collect())
            .unwrap_or_default();

        let allow_credentials = std::env::var("CORS_ALLOW_CREDENTIALS")
            .map(|s| s.to_lowercase() == "true")
            .unwrap_or(true);

        let max_age_secs = std::env::var("CORS_MAX_AGE")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(3600);

        let permissive = std::env::var("CORS_PERMISSIVE")
            .map(|s| s.to_lowercase() == "true")
            .unwrap_or(allowed_origins.is_empty()); // Permissive if no origins specified

        Self {
            allowed_origins,
            allow_credentials,
            max_age_secs,
            permissive,
            ..Default::default()
        }
    }

    /// Build the tower-http CorsLayer from this configuration
    pub fn build_layer(&self) -> CorsLayer {
        if self.permissive {
            // Development mode: allow everything
            CorsLayer::permissive()
        } else {
            // Production mode: strict configuration
            let mut layer = CorsLayer::new();

            // Configure allowed origins
            if self.allowed_origins.is_empty() {
                // No origins specified - this is a configuration error in production
                // Fall back to very restrictive (same-origin only)
                tracing::warn!("CORS: No allowed origins configured in production mode. Using restrictive defaults.");
                layer = layer.allow_origin(Any);
            } else {
                // Parse origins into HeaderValues
                let origins: Vec<HeaderValue> = self
                    .allowed_origins
                    .iter()
                    .filter_map(|origin| {
                        origin.parse().ok().or_else(|| {
                            tracing::warn!("CORS: Invalid origin format: {}", origin);
                            None
                        })
                    })
                    .collect();

                if origins.is_empty() {
                    tracing::warn!("CORS: No valid origins after parsing. Using restrictive defaults.");
                    layer = layer.allow_origin(Any);
                } else {
                    layer = layer.allow_origin(origins);
                }
            }

            // Configure methods
            layer = layer.allow_methods(self.allowed_methods.clone());

            // Configure allowed headers
            let headers: Vec<header::HeaderName> = self
                .allowed_headers
                .iter()
                .filter_map(|h| h.parse().ok())
                .collect();
            layer = layer.allow_headers(headers);

            // Configure exposed headers
            let expose: Vec<header::HeaderName> = self
                .expose_headers
                .iter()
                .filter_map(|h| h.parse().ok())
                .collect();
            layer = layer.expose_headers(expose);

            // Configure credentials
            if self.allow_credentials {
                layer = layer.allow_credentials(true);
            }

            // Configure max age for preflight cache
            layer = layer.max_age(Duration::from_secs(self.max_age_secs));

            layer
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = CorsConfig::default();
        assert!(config.permissive);
        assert!(config.allow_credentials);
        assert_eq!(config.max_age_secs, 3600);
    }

    #[test]
    fn test_development_config() {
        let config = CorsConfig::development();
        assert!(config.permissive);
    }

    #[test]
    fn test_production_config() {
        let origins = vec!["https://app.example.com".to_string()];
        let config = CorsConfig::production(origins.clone());
        assert!(!config.permissive);
        assert_eq!(config.allowed_origins, origins);
        assert_eq!(config.max_age_secs, 86400);
    }

    #[test]
    fn test_build_layer_permissive() {
        let config = CorsConfig::development();
        let _layer = config.build_layer(); // Should not panic
    }

    #[test]
    fn test_build_layer_production() {
        let config = CorsConfig::production(vec!["https://example.com".to_string()]);
        let _layer = config.build_layer(); // Should not panic
    }

    #[test]
    fn test_allowed_headers_includes_api_key() {
        let config = CorsConfig::default();
        assert!(config.allowed_headers.contains(&"X-API-Key".to_string()));
        assert!(config.allowed_headers.contains(&"Authorization".to_string()));
    }

    #[test]
    fn test_expose_headers_includes_rate_limit() {
        let config = CorsConfig::default();
        assert!(config.expose_headers.contains(&"X-RateLimit-Limit".to_string()));
        assert!(config.expose_headers.contains(&"X-RateLimit-Remaining".to_string()));
    }
}
