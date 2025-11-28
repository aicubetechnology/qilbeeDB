//! Enterprise-grade security module for QilbeeDB
//!
//! Provides comprehensive authentication, authorization, audit, and rate limiting
//! capabilities for all protocols (HTTP, Bolt).

pub mod auth;
pub mod rbac;
pub mod user;
pub mod middleware;
pub mod audit;
pub mod token;
pub mod bootstrap;
pub mod rate_limit;
pub mod token_blacklist;
pub mod account_lockout;
pub mod password;
pub mod security_headers;
pub mod cors;
pub mod https;

pub use auth::{AuthService, Credentials, AuthConfig, Session};
pub use rbac::{Permission, Role, RbacService};
pub use user::{User, UserService, UserId};
pub use middleware::{AuthMiddleware, require_auth, require_permission, optional_auth, get_user, rate_limit, global_rate_limit};
pub use audit::{AuditLog, AuditService, AuditEvent, AuditEventType, AuditResult, AuditFilter, AuditConfig};
pub use token::{TokenService, ApiKey, AuthToken, Claims};
pub use bootstrap::{BootstrapService, BootstrapState};
pub use rate_limit::{RateLimitService, RateLimitPolicy, RateLimitKey, RateLimitInfo, EndpointType, PolicyId};
pub use token_blacklist::{TokenBlacklist, BlacklistConfig, BlacklistedToken, RevocationReason};
pub use account_lockout::{AccountLockoutService, LockoutConfig, LockoutStatus};
pub use password::{validate_password, PasswordPolicy, PasswordValidationResult, PASSWORD_REQUIREMENTS};
pub use security_headers::{security_headers_middleware, SecurityHeadersConfig};
pub use cors::CorsConfig;
pub use https::{HttpsConfig, TlsConfig, TlsVersion, https_redirect_middleware, check_tls_config};
