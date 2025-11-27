//! HTTP security middleware for Axum

use axum::{
    extract::{Request, State},
    http::{StatusCode, HeaderMap},
    middleware::Next,
    response::{IntoResponse, Response, Json},
};
use serde_json::json;
use std::sync::Arc;
use super::{AuthService, RbacService, Permission, User, AuditService, AuditResult, RateLimitService, EndpointType, RateLimitKey};

/// Shared authentication middleware state
#[derive(Clone)]
pub struct AuthMiddleware {
    pub auth_service: Arc<AuthService>,
    pub rbac_service: Arc<RbacService>,
    pub audit_service: Arc<AuditService>,
    pub rate_limit_service: Arc<RateLimitService>,
}

impl AuthMiddleware {
    pub fn new(
        auth_service: Arc<AuthService>,
        rbac_service: Arc<RbacService>,
        audit_service: Arc<AuditService>,
        rate_limit_service: Arc<RateLimitService>,
    ) -> Self {
        Self {
            auth_service,
            rbac_service,
            audit_service,
            rate_limit_service,
        }
    }
}

/// Extract IP address from request headers
fn extract_ip(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(',').next().unwrap_or("").trim().to_string())
        .or_else(|| {
            headers
                .get("x-real-ip")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string())
        })
}

/// Extract user agent from request headers
fn extract_user_agent(headers: &HeaderMap) -> Option<String> {
    headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

/// Authentication middleware that validates JWT or API keys
pub async fn require_auth(
    State(middleware): State<AuthMiddleware>,
    mut req: Request,
    next: Next,
) -> Result<Response, impl IntoResponse> {
    let headers = req.headers();
    let ip_address = extract_ip(headers);
    let _user_agent = extract_user_agent(headers);

    // Try Bearer token first
    if let Some(auth_header) = headers.get("authorization").and_then(|h| h.to_str().ok()) {
        if auth_header.starts_with("Bearer ") {
            let token = &auth_header[7..];
            match middleware.auth_service.validate_token(token) {
                Ok(user) => {
                    // Log successful authentication
                    middleware.audit_service.log_auth_event(
                        &user.username,
                        "validate_token",
                        AuditResult::Success,
                        ip_address,
                    );

                    // Insert user into request extensions
                    req.extensions_mut().insert(user);
                    return Ok(next.run(req).await);
                }
                Err(e) => {
                    middleware.audit_service.log_auth_event(
                        "unknown",
                        "validate_token",
                        AuditResult::Unauthorized,
                        ip_address,
                    );

                    return Err((
                        StatusCode::UNAUTHORIZED,
                        Json(json!({
                            "error": "Invalid or expired token",
                            "message": e.to_string()
                        }))
                    ));
                }
            }
        }
    }

    // Try API key
    if let Some(api_key) = headers.get("x-api-key").and_then(|h| h.to_str().ok()) {
        match middleware.auth_service.validate_api_key(api_key) {
            Ok(user) => {
                // Log successful API key authentication
                middleware.audit_service.log_auth_event(
                    &user.username,
                    "validate_api_key",
                    AuditResult::Success,
                    ip_address,
                );

                req.extensions_mut().insert(user);
                return Ok(next.run(req).await);
            }
            Err(e) => {
                middleware.audit_service.log_auth_event(
                    "unknown",
                    "validate_api_key",
                    AuditResult::Unauthorized,
                    ip_address,
                );

                return Err((
                    StatusCode::UNAUTHORIZED,
                    Json(json!({
                        "error": "Invalid API key",
                        "message": e.to_string()
                    }))
                ));
            }
        }
    }

    // No authentication provided
    middleware.audit_service.log_auth_event(
        "unknown",
        "authentication_missing",
        AuditResult::Unauthorized,
        ip_address,
    );

    Err((
        StatusCode::UNAUTHORIZED,
        Json(json!({
            "error": "Authentication required",
            "message": "Provide either Bearer token or X-API-Key header"
        }))
    ))
}

/// Optional authentication middleware (doesn't fail if no auth provided)
pub async fn optional_auth(
    State(middleware): State<AuthMiddleware>,
    mut req: Request,
    next: Next,
) -> Response {
    // Extract headers first to avoid borrow checker issues
    let auth_header = req
        .headers()
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());

    let api_key_header = req
        .headers()
        .get("x-api-key")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());

    // Try Bearer token
    if let Some(auth_header) = auth_header {
        if auth_header.starts_with("Bearer ") {
            let token = &auth_header[7..];
            if let Ok(user) = middleware.auth_service.validate_token(token) {
                req.extensions_mut().insert(user);
            }
        }
    }

    // Try API key if no bearer token
    if req.extensions().get::<User>().is_none() {
        if let Some(api_key) = api_key_header {
            if let Ok(user) = middleware.auth_service.validate_api_key(&api_key) {
                req.extensions_mut().insert(user);
            }
        }
    }

    next.run(req).await
}

/// Permission checking middleware - MUST be used after require_auth
pub async fn require_permission(
    permission: Permission,
    State(middleware): State<AuthMiddleware>,
    req: Request,
    next: Next,
) -> Result<Response, impl IntoResponse> {
    // Get user from request extensions (inserted by require_auth)
    let user = req
        .extensions()
        .get::<User>()
        .ok_or((
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": "Authentication required",
                "message": "User not found in request. Ensure require_auth middleware is applied first."
            }))
        ))?;

    // Check permission
    if !middleware.rbac_service.has_permission(&user.roles, &permission) {
        // Log unauthorized access attempt
        middleware.audit_service.log_access(
            &user.id.0.to_string(),
            &user.username,
            &format!("require_permission:{:?}", permission),
            "unknown",
            AuditResult::Forbidden,
        );

        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "Insufficient permissions",
                "message": format!("Required permission: {:?}", permission)
            }))
        ));
    }

    // Log successful access
    middleware.audit_service.log_access(
        &user.id.0.to_string(),
        &user.username,
        &format!("require_permission:{:?}", permission),
        "unknown",
        AuditResult::Success,
    );

    Ok(next.run(req).await)
}

/// Extract authenticated user from request
pub fn get_user(req: &Request) -> Option<&User> {
    req.extensions().get::<User>()
}

/// Rate limiting middleware
///
/// Checks rate limits for the specified endpoint type using IP address or user ID
/// Returns 429 Too Many Requests if limit exceeded with rate limit headers
pub async fn rate_limit(
    endpoint_type: EndpointType,
    State(middleware): State<AuthMiddleware>,
    req: Request,
    next: Next,
) -> Response {
    let headers = req.headers();

    // Determine rate limit key (prefer user ID over IP)
    let rate_limit_key = if let Some(user) = req.extensions().get::<User>() {
        RateLimitKey::from_user_id(user.id.0.to_string())
    } else {
        // Fall back to IP address
        let ip = extract_ip(headers).unwrap_or_else(|| "unknown".to_string());
        RateLimitKey::from_ip(ip)
    };

    // Check rate limit
    let rate_limit_info = middleware.rate_limit_service.check(endpoint_type.clone(), rate_limit_key);

    if !rate_limit_info.allowed {
        // Rate limit exceeded - return 429 with headers
        let mut response = Json(json!({
            "error": "Too Many Requests",
            "message": format!("Rate limit exceeded for {:?}", endpoint_type),
            "limit": rate_limit_info.limit,
            "remaining": rate_limit_info.remaining,
            "reset_in_seconds": rate_limit_info.reset
        })).into_response();

        // Add rate limit headers
        let headers = response.headers_mut();
        headers.insert("X-RateLimit-Limit", rate_limit_info.limit.to_string().parse().unwrap());
        headers.insert("X-RateLimit-Remaining", rate_limit_info.remaining.to_string().parse().unwrap());
        headers.insert("X-RateLimit-Reset", rate_limit_info.reset.to_string().parse().unwrap());

        *response.status_mut() = StatusCode::TOO_MANY_REQUESTS;

        return response;
    }

    // Rate limit OK - add headers and continue
    let mut response = next.run(req).await;
    let headers = response.headers_mut();
    headers.insert("X-RateLimit-Limit", rate_limit_info.limit.to_string().parse().unwrap());
    headers.insert("X-RateLimit-Remaining", rate_limit_info.remaining.to_string().parse().unwrap());
    headers.insert("X-RateLimit-Reset", rate_limit_info.reset.to_string().parse().unwrap());

    response
}

/// Determine endpoint type from request path
fn determine_endpoint_type(path: &str) -> EndpointType {
    // Login endpoint - most restrictive
    if path == "/api/v1/auth/login" {
        return EndpointType::Login;
    }

    // API key management
    if path.starts_with("/api/v1/api-keys") {
        return EndpointType::ApiKeyCreation;
    }

    // User management
    if path.starts_with("/api/v1/users") {
        return EndpointType::UserManagement;
    }

    // Everything else is general API
    EndpointType::GeneralApi
}

/// Global rate limiting middleware that determines endpoint type from the request path
///
/// This middleware should be applied globally and will automatically route to the
/// correct rate limit policy based on the request path.
/// Uses from_fn_with_state pattern for proper middleware integration.
pub async fn global_rate_limit(
    State(middleware): State<AuthMiddleware>,
    req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path().to_string();

    tracing::debug!("Global rate limit middleware called for path: {}", path);

    // Skip rate limiting for health check
    if path == "/health" {
        return next.run(req).await;
    }

    let headers = req.headers();

    // Determine endpoint type from path
    let endpoint_type = determine_endpoint_type(&path);

    // Determine rate limit key (prefer user ID over IP)
    let rate_limit_key = if let Some(user) = req.extensions().get::<User>() {
        RateLimitKey::from_user_id(user.id.0.to_string())
    } else {
        // Fall back to IP address
        let ip = extract_ip(headers).unwrap_or_else(|| "unknown".to_string());
        RateLimitKey::from_ip(ip)
    };

    // Check rate limit
    let rate_limit_info = middleware.rate_limit_service.check(endpoint_type.clone(), rate_limit_key);

    if !rate_limit_info.allowed {
        // Rate limit exceeded - return 429 with headers
        let mut response = Json(json!({
            "error": "Too Many Requests",
            "message": format!("Rate limit exceeded for {:?}", endpoint_type),
            "limit": rate_limit_info.limit,
            "remaining": rate_limit_info.remaining,
            "reset_in_seconds": rate_limit_info.reset
        })).into_response();

        // Add rate limit headers
        let headers = response.headers_mut();
        headers.insert("X-RateLimit-Limit", rate_limit_info.limit.to_string().parse().unwrap());
        headers.insert("X-RateLimit-Remaining", rate_limit_info.remaining.to_string().parse().unwrap());
        headers.insert("X-RateLimit-Reset", rate_limit_info.reset.to_string().parse().unwrap());

        *response.status_mut() = StatusCode::TOO_MANY_REQUESTS;

        return response;
    }

    // Rate limit OK - add headers and continue
    let mut response = next.run(req).await;
    let headers = response.headers_mut();
    headers.insert("X-RateLimit-Limit", rate_limit_info.limit.to_string().parse().unwrap());
    headers.insert("X-RateLimit-Remaining", rate_limit_info.remaining.to_string().parse().unwrap());
    headers.insert("X-RateLimit-Reset", rate_limit_info.reset.to_string().parse().unwrap());

    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::{UserService, TokenService, AuthConfig, Role, AuditConfig};
    use axum::{
        body::Body,
        http::{Request, header},
    };

    fn create_test_middleware() -> AuthMiddleware {
        let user_service = Arc::new(UserService::new());
        let token_service = Arc::new(TokenService::new("test_secret".to_string()));
        let auth_service = Arc::new(AuthService::new(
            user_service.clone(),
            token_service.clone(),
            AuthConfig::default(),
        ));
        let rbac_service = Arc::new(RbacService::new());
        let audit_service = Arc::new(AuditService::new(AuditConfig::default()));
        let rate_limit_service = Arc::new(RateLimitService::new());

        // Create test user
        user_service
            .create_user(
                "testuser".to_string(),
                "test@example.com".to_string(),
                "password123",
            )
            .unwrap();

        AuthMiddleware::new(auth_service, rbac_service, audit_service, rate_limit_service)
    }

    #[tokio::test]
    async fn test_extract_ip() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", "192.168.1.1, 10.0.0.1".parse().unwrap());

        let ip = extract_ip(&headers);
        assert_eq!(ip, Some("192.168.1.1".to_string()));
    }

    #[tokio::test]
    async fn test_extract_user_agent() {
        let mut headers = HeaderMap::new();
        headers.insert("user-agent", "Mozilla/5.0".parse().unwrap());

        let ua = extract_user_agent(&headers);
        assert_eq!(ua, Some("Mozilla/5.0".to_string()));
    }
}
