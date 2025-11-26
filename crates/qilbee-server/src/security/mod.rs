//! Enterprise-grade security module for QilbeeDB
//!
//! Provides comprehensive authentication, authorization, and audit capabilities
//! for all protocols (HTTP, Bolt).

pub mod auth;
pub mod rbac;
pub mod user;
pub mod middleware;
pub mod audit;
pub mod token;
pub mod bootstrap;

pub use auth::{AuthService, Credentials, AuthConfig, Session};
pub use rbac::{Permission, Role, RbacService};
pub use user::{User, UserService, UserId};
pub use middleware::{AuthMiddleware, require_auth, require_permission, optional_auth, get_user};
pub use audit::{AuditLog, AuditService, AuditEvent, AuditResult, AuditFilter, AuditConfig};
pub use token::{TokenService, ApiKey, AuthToken, Claims};
pub use bootstrap::{BootstrapService, BootstrapState};
