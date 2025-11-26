//! QilbeeDB Server
//!
//! The main server component that ties together all QilbeeDB subsystems.
//!
//! # Features
//!
//! - Multi-protocol support (Bolt, HTTP, gRPC)
//! - Connection management
//! - Query execution
//! - Agent memory management
//! - Enterprise-grade security

pub mod config;
pub mod server;
pub mod http_server;
pub mod security;

pub use config::ServerConfig;
pub use server::Server;
pub use security::{
    AuthService, User, UserService, RbacService, TokenService,
    Permission, Role, AuditService,
};
