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

pub use auth::{AuthService, Credentials, AuthConfig, Session};
pub use rbac::{Permission, Role, RbacService};
pub use user::{User, UserService, UserId};
pub use middleware::{AuthMiddleware, require_auth, require_permission, optional_auth, get_user, rate_limit, global_rate_limit};
pub use audit::{AuditLog, AuditService, AuditEvent, AuditEventType, AuditResult, AuditFilter, AuditConfig};
pub use token::{TokenService, ApiKey, AuthToken, Claims};
pub use bootstrap::{BootstrapService, BootstrapState};
pub use rate_limit::{RateLimitService, RateLimitPolicy, RateLimitKey, RateLimitInfo, EndpointType, PolicyId};
pub use token_blacklist::{TokenBlacklist, BlacklistConfig, BlacklistedToken, RevocationReason};
