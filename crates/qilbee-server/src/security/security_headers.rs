//! Security headers middleware for HTTP responses
//!
//! Adds security headers to all HTTP responses to protect against common
//! web vulnerabilities such as XSS, clickjacking, MIME sniffing, etc.

use axum::{
    body::Body,
    http::{header, Request, Response, HeaderValue},
    middleware::Next,
};

/// Security headers configuration
#[derive(Debug, Clone)]
pub struct SecurityHeadersConfig {
    /// Enable Strict-Transport-Security header (HSTS)
    pub enable_hsts: bool,
    /// HSTS max-age in seconds (default: 1 year)
    pub hsts_max_age: u64,
    /// Include subdomains in HSTS
    pub hsts_include_subdomains: bool,
    /// Enable HSTS preload
    pub hsts_preload: bool,
    /// X-Content-Type-Options value
    pub content_type_options: String,
    /// X-Frame-Options value (DENY, SAMEORIGIN, or empty to disable)
    pub frame_options: String,
    /// X-XSS-Protection value
    pub xss_protection: String,
    /// Referrer-Policy value
    pub referrer_policy: String,
    /// Content-Security-Policy value (empty to disable)
    pub content_security_policy: String,
    /// Permissions-Policy value (empty to disable)
    pub permissions_policy: String,
    /// Cache-Control for sensitive endpoints
    pub cache_control: String,
}

impl Default for SecurityHeadersConfig {
    fn default() -> Self {
        Self {
            enable_hsts: true,
            hsts_max_age: 31536000, // 1 year
            hsts_include_subdomains: true,
            hsts_preload: false, // Don't preload by default (requires domain owner action)
            content_type_options: "nosniff".to_string(),
            frame_options: "DENY".to_string(),
            xss_protection: "1; mode=block".to_string(),
            referrer_policy: "strict-origin-when-cross-origin".to_string(),
            content_security_policy: "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; object-src 'none'; frame-ancestors 'none'; base-uri 'self'; form-action 'self'".to_string(),
            permissions_policy: "geolocation=(), microphone=(), camera=(), payment=()".to_string(),
            cache_control: "no-store, no-cache, must-revalidate, proxy-revalidate".to_string(),
        }
    }
}

impl SecurityHeadersConfig {
    /// Create a development configuration (less strict)
    pub fn development() -> Self {
        Self {
            enable_hsts: false, // Don't enable HSTS in development
            content_security_policy: String::new(), // Disable CSP in development
            ..Default::default()
        }
    }

    /// Create a production configuration (strict)
    pub fn production() -> Self {
        Self {
            enable_hsts: true,
            hsts_max_age: 31536000,
            hsts_include_subdomains: true,
            hsts_preload: true,
            ..Default::default()
        }
    }
}

/// Middleware that adds security headers to all responses
pub async fn security_headers_middleware(
    request: Request<Body>,
    next: Next,
) -> Response<Body> {
    // Process the request first
    let mut response = next.run(request).await;
    let headers = response.headers_mut();

    // Use default config (can be made configurable via state later)
    let config = SecurityHeadersConfig::default();

    // X-Content-Type-Options - Prevents MIME type sniffing
    if !config.content_type_options.is_empty() {
        if let Ok(value) = HeaderValue::from_str(&config.content_type_options) {
            headers.insert(header::X_CONTENT_TYPE_OPTIONS, value);
        }
    }

    // X-Frame-Options - Prevents clickjacking
    if !config.frame_options.is_empty() {
        if let Ok(value) = HeaderValue::from_str(&config.frame_options) {
            headers.insert(header::X_FRAME_OPTIONS, value);
        }
    }

    // X-XSS-Protection - XSS filter (legacy, but still useful for older browsers)
    if !config.xss_protection.is_empty() {
        if let Ok(value) = HeaderValue::from_str(&config.xss_protection) {
            headers.insert(header::X_XSS_PROTECTION, value);
        }
    }

    // Strict-Transport-Security (HSTS) - Forces HTTPS
    if config.enable_hsts {
        let mut hsts_value = format!("max-age={}", config.hsts_max_age);
        if config.hsts_include_subdomains {
            hsts_value.push_str("; includeSubDomains");
        }
        if config.hsts_preload {
            hsts_value.push_str("; preload");
        }
        if let Ok(value) = HeaderValue::from_str(&hsts_value) {
            headers.insert(header::STRICT_TRANSPORT_SECURITY, value);
        }
    }

    // Referrer-Policy - Controls referrer information
    if !config.referrer_policy.is_empty() {
        if let Ok(value) = HeaderValue::from_str(&config.referrer_policy) {
            headers.insert(header::REFERRER_POLICY, value);
        }
    }

    // Content-Security-Policy - Prevents XSS and data injection
    if !config.content_security_policy.is_empty() {
        if let Ok(value) = HeaderValue::from_str(&config.content_security_policy) {
            headers.insert(header::CONTENT_SECURITY_POLICY, value);
        }
    }

    // Permissions-Policy (formerly Feature-Policy) - Controls browser features
    if !config.permissions_policy.is_empty() {
        if let Ok(value) = HeaderValue::from_str(&config.permissions_policy) {
            headers.insert(
                header::HeaderName::from_static("permissions-policy"),
                value,
            );
        }
    }

    // Cache-Control for security-sensitive responses
    if !config.cache_control.is_empty() {
        if let Ok(value) = HeaderValue::from_str(&config.cache_control) {
            // Only set if not already set by the handler
            if !headers.contains_key(header::CACHE_CONTROL) {
                headers.insert(header::CACHE_CONTROL, value);
            }
        }
    }

    // Pragma: no-cache (for HTTP/1.0 compatibility)
    if let Ok(value) = HeaderValue::from_str("no-cache") {
        if !headers.contains_key(header::PRAGMA) {
            headers.insert(header::PRAGMA, value);
        }
    }

    // X-Permitted-Cross-Domain-Policies - Controls Adobe cross-domain policy
    if let Ok(value) = HeaderValue::from_str("none") {
        headers.insert(
            header::HeaderName::from_static("x-permitted-cross-domain-policies"),
            value,
        );
    }

    // X-Download-Options - Prevents IE from executing downloads in site context
    if let Ok(value) = HeaderValue::from_str("noopen") {
        headers.insert(
            header::HeaderName::from_static("x-download-options"),
            value,
        );
    }

    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = SecurityHeadersConfig::default();
        assert!(config.enable_hsts);
        assert_eq!(config.hsts_max_age, 31536000);
        assert_eq!(config.content_type_options, "nosniff");
        assert_eq!(config.frame_options, "DENY");
    }

    #[test]
    fn test_development_config() {
        let config = SecurityHeadersConfig::development();
        assert!(!config.enable_hsts);
        assert!(config.content_security_policy.is_empty());
    }

    #[test]
    fn test_production_config() {
        let config = SecurityHeadersConfig::production();
        assert!(config.enable_hsts);
        assert!(config.hsts_preload);
    }
}
