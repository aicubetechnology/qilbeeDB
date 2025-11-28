//! HTTPS enforcement middleware for production environments
//!
//! Provides HTTP to HTTPS redirect functionality and HTTPS enforcement
//! for secure production deployments.

use axum::{
    body::Body,
    http::{header, Request, Response, StatusCode},
    middleware::Next,
};

/// HTTPS enforcement configuration
#[derive(Debug, Clone)]
pub struct HttpsConfig {
    /// Enable HTTPS enforcement (redirect HTTP to HTTPS)
    pub enabled: bool,
    /// The HTTPS port to redirect to (default: 443)
    pub https_port: u16,
    /// Allow localhost/127.0.0.1 connections over HTTP (for development)
    pub allow_localhost: bool,
    /// Trusted proxy headers for detecting HTTPS behind load balancers
    /// Common headers: X-Forwarded-Proto, X-Forwarded-Ssl, Front-End-Https
    pub trust_proxy_headers: bool,
    /// Exempt paths from HTTPS enforcement (e.g., health checks)
    pub exempt_paths: Vec<String>,
}

impl Default for HttpsConfig {
    fn default() -> Self {
        Self {
            enabled: false, // Disabled by default for development
            https_port: 443,
            allow_localhost: true,
            trust_proxy_headers: true,
            exempt_paths: vec![
                "/health".to_string(),
                "/ready".to_string(),
                "/metrics".to_string(),
            ],
        }
    }
}

impl HttpsConfig {
    /// Create a development configuration (HTTPS disabled)
    pub fn development() -> Self {
        Self {
            enabled: false,
            allow_localhost: true,
            ..Default::default()
        }
    }

    /// Create a production configuration (HTTPS enforced)
    pub fn production() -> Self {
        Self {
            enabled: true,
            allow_localhost: false,
            trust_proxy_headers: true,
            ..Default::default()
        }
    }

    /// Create a production configuration for behind a load balancer
    pub fn behind_proxy() -> Self {
        Self {
            enabled: true,
            allow_localhost: false,
            trust_proxy_headers: true,
            // When behind a proxy, the proxy handles TLS termination
            // and forwards X-Forwarded-Proto header
            ..Default::default()
        }
    }

    /// Create HTTPS configuration from environment variables
    ///
    /// Reads the following environment variables:
    /// - `HTTPS_ENFORCE`: "true" to enable HTTPS enforcement (default: false)
    /// - `HTTPS_PORT`: HTTPS port to redirect to (default: 443)
    /// - `HTTPS_ALLOW_LOCALHOST`: "true" to allow HTTP for localhost (default: true)
    /// - `HTTPS_TRUST_PROXY`: "true" to trust X-Forwarded-Proto header (default: true)
    /// - `HTTPS_EXEMPT_PATHS`: Comma-separated list of paths exempt from HTTPS
    pub fn from_env() -> Self {
        let enabled = std::env::var("HTTPS_ENFORCE")
            .map(|s| s.to_lowercase() == "true")
            .unwrap_or(false);

        let https_port = std::env::var("HTTPS_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(443);

        let allow_localhost = std::env::var("HTTPS_ALLOW_LOCALHOST")
            .map(|s| s.to_lowercase() == "true")
            .unwrap_or(true);

        let trust_proxy_headers = std::env::var("HTTPS_TRUST_PROXY")
            .map(|s| s.to_lowercase() == "true")
            .unwrap_or(true);

        let exempt_paths: Vec<String> = std::env::var("HTTPS_EXEMPT_PATHS")
            .map(|s| s.split(',').map(|s| s.trim().to_string()).collect())
            .unwrap_or_else(|_| vec![
                "/health".to_string(),
                "/ready".to_string(),
                "/metrics".to_string(),
            ]);

        Self {
            enabled,
            https_port,
            allow_localhost,
            trust_proxy_headers,
            exempt_paths,
        }
    }

    /// Check if a request should be redirected to HTTPS
    pub fn should_redirect(&self, request: &Request<Body>) -> bool {
        if !self.enabled {
            return false;
        }

        // Check exempt paths
        let path = request.uri().path();
        for exempt_path in &self.exempt_paths {
            if path.starts_with(exempt_path) {
                return false;
            }
        }

        // Check if already HTTPS via proxy header
        if self.trust_proxy_headers {
            // X-Forwarded-Proto is the standard header
            if let Some(proto) = request.headers().get("x-forwarded-proto") {
                if proto.to_str().map(|s| s == "https").unwrap_or(false) {
                    return false;
                }
            }

            // X-Forwarded-Ssl is used by some proxies
            if let Some(ssl) = request.headers().get("x-forwarded-ssl") {
                if ssl.to_str().map(|s| s == "on").unwrap_or(false) {
                    return false;
                }
            }

            // Front-End-Https is used by some Microsoft proxies
            if let Some(https) = request.headers().get("front-end-https") {
                if https.to_str().map(|s| s == "on").unwrap_or(false) {
                    return false;
                }
            }
        }

        // Check localhost exemption
        if self.allow_localhost {
            if let Some(host) = request.headers().get(header::HOST) {
                if let Ok(host_str) = host.to_str() {
                    let host_part = host_str.split(':').next().unwrap_or(host_str);
                    if host_part == "localhost" || host_part == "127.0.0.1" || host_part == "::1" {
                        return false;
                    }
                }
            }
        }

        // If we reach here and none of the exemptions apply, redirect
        true
    }

    /// Build the HTTPS redirect URL
    pub fn build_redirect_url(&self, request: &Request<Body>) -> Option<String> {
        let host = request.headers().get(header::HOST)?.to_str().ok()?;

        // Remove port from host if present
        let host_without_port = host.split(':').next().unwrap_or(host);

        // Build the HTTPS URL
        let path_and_query = request.uri().path_and_query()
            .map(|pq| pq.as_str())
            .unwrap_or("/");

        let redirect_url = if self.https_port == 443 {
            format!("https://{}{}", host_without_port, path_and_query)
        } else {
            format!("https://{}:{}{}", host_without_port, self.https_port, path_and_query)
        };

        Some(redirect_url)
    }
}

/// Middleware that enforces HTTPS connections
///
/// When HTTPS enforcement is enabled, HTTP requests are redirected to HTTPS
/// with a 301 Permanent Redirect status code.
pub async fn https_redirect_middleware(
    request: Request<Body>,
    next: Next,
) -> Response<Body> {
    let config = HttpsConfig::from_env();

    if config.should_redirect(&request) {
        if let Some(redirect_url) = config.build_redirect_url(&request) {
            tracing::info!(
                url = %redirect_url,
                "Redirecting HTTP request to HTTPS"
            );

            return Response::builder()
                .status(StatusCode::MOVED_PERMANENTLY)
                .header(header::LOCATION, redirect_url)
                .header(header::CONTENT_TYPE, "text/plain")
                .body(Body::from("Redirecting to HTTPS..."))
                .unwrap_or_else(|_| {
                    Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .body(Body::from("Redirect failed"))
                        .unwrap()
                });
        }
    }

    next.run(request).await
}

/// Helper function to check if TLS is properly configured
pub fn check_tls_config() -> Result<(), String> {
    let cert_path = std::env::var("TLS_CERT_PATH");
    let key_path = std::env::var("TLS_KEY_PATH");

    match (cert_path, key_path) {
        (Ok(cert), Ok(key)) => {
            // Check if files exist
            if !std::path::Path::new(&cert).exists() {
                return Err(format!("TLS certificate not found at: {}", cert));
            }
            if !std::path::Path::new(&key).exists() {
                return Err(format!("TLS private key not found at: {}", key));
            }
            Ok(())
        }
        (Ok(_), Err(_)) => Err("TLS_KEY_PATH not set".to_string()),
        (Err(_), Ok(_)) => Err("TLS_CERT_PATH not set".to_string()),
        (Err(_), Err(_)) => Err("TLS not configured (TLS_CERT_PATH and TLS_KEY_PATH not set)".to_string()),
    }
}

/// TLS configuration for the server
#[derive(Debug, Clone)]
pub struct TlsConfig {
    /// Path to the TLS certificate file (PEM format)
    pub cert_path: String,
    /// Path to the TLS private key file (PEM format)
    pub key_path: String,
    /// Minimum TLS version (default: TLS 1.2)
    pub min_tls_version: TlsVersion,
    /// Enable/disable specific cipher suites
    pub cipher_suites: Vec<String>,
}

/// Supported TLS versions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TlsVersion {
    Tls12,
    Tls13,
}

impl Default for TlsVersion {
    fn default() -> Self {
        TlsVersion::Tls12
    }
}

impl TlsConfig {
    /// Create TLS configuration from environment variables
    ///
    /// Reads the following environment variables:
    /// - `TLS_CERT_PATH`: Path to the TLS certificate file
    /// - `TLS_KEY_PATH`: Path to the TLS private key file
    /// - `TLS_MIN_VERSION`: Minimum TLS version ("1.2" or "1.3", default: "1.2")
    pub fn from_env() -> Option<Self> {
        let cert_path = std::env::var("TLS_CERT_PATH").ok()?;
        let key_path = std::env::var("TLS_KEY_PATH").ok()?;

        let min_tls_version = std::env::var("TLS_MIN_VERSION")
            .map(|v| match v.as_str() {
                "1.3" => TlsVersion::Tls13,
                _ => TlsVersion::Tls12,
            })
            .unwrap_or(TlsVersion::Tls12);

        Some(Self {
            cert_path,
            key_path,
            min_tls_version,
            cipher_suites: vec![],
        })
    }

    /// Check if TLS is configured via environment variables
    pub fn is_configured() -> bool {
        std::env::var("TLS_CERT_PATH").is_ok() && std::env::var("TLS_KEY_PATH").is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;

    #[test]
    fn test_default_config() {
        let config = HttpsConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.https_port, 443);
        assert!(config.allow_localhost);
        assert!(config.trust_proxy_headers);
    }

    #[test]
    fn test_development_config() {
        let config = HttpsConfig::development();
        assert!(!config.enabled);
        assert!(config.allow_localhost);
    }

    #[test]
    fn test_production_config() {
        let config = HttpsConfig::production();
        assert!(config.enabled);
        assert!(!config.allow_localhost);
    }

    #[test]
    fn test_behind_proxy_config() {
        let config = HttpsConfig::behind_proxy();
        assert!(config.enabled);
        assert!(config.trust_proxy_headers);
    }

    #[test]
    fn test_exempt_paths() {
        let config = HttpsConfig {
            enabled: true,
            exempt_paths: vec!["/health".to_string()],
            ..Default::default()
        };

        let request = Request::builder()
            .uri("/health")
            .header("host", "example.com")
            .body(Body::empty())
            .unwrap();

        assert!(!config.should_redirect(&request));
    }

    #[test]
    fn test_https_via_proxy_header() {
        let config = HttpsConfig {
            enabled: true,
            trust_proxy_headers: true,
            ..Default::default()
        };

        let request = Request::builder()
            .uri("/api/test")
            .header("host", "example.com")
            .header("x-forwarded-proto", "https")
            .body(Body::empty())
            .unwrap();

        assert!(!config.should_redirect(&request));
    }

    #[test]
    fn test_localhost_exemption() {
        let config = HttpsConfig {
            enabled: true,
            allow_localhost: true,
            ..Default::default()
        };

        let request = Request::builder()
            .uri("/api/test")
            .header("host", "localhost:7474")
            .body(Body::empty())
            .unwrap();

        assert!(!config.should_redirect(&request));
    }

    #[test]
    fn test_should_redirect_http() {
        let config = HttpsConfig {
            enabled: true,
            allow_localhost: false,
            trust_proxy_headers: false,
            exempt_paths: vec![],
            ..Default::default()
        };

        let request = Request::builder()
            .uri("/api/test")
            .header("host", "example.com")
            .body(Body::empty())
            .unwrap();

        assert!(config.should_redirect(&request));
    }

    #[test]
    fn test_build_redirect_url() {
        let config = HttpsConfig {
            https_port: 443,
            ..Default::default()
        };

        let request = Request::builder()
            .uri("/api/test?param=value")
            .header("host", "example.com")
            .body(Body::empty())
            .unwrap();

        let url = config.build_redirect_url(&request);
        assert_eq!(url, Some("https://example.com/api/test?param=value".to_string()));
    }

    #[test]
    fn test_build_redirect_url_custom_port() {
        let config = HttpsConfig {
            https_port: 8443,
            ..Default::default()
        };

        let request = Request::builder()
            .uri("/api/test")
            .header("host", "example.com:8080")
            .body(Body::empty())
            .unwrap();

        let url = config.build_redirect_url(&request);
        assert_eq!(url, Some("https://example.com:8443/api/test".to_string()));
    }

    #[test]
    fn test_tls_version_default() {
        assert_eq!(TlsVersion::default(), TlsVersion::Tls12);
    }
}
