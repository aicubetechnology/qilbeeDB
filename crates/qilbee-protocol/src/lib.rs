//! QilbeeDB Protocol Implementations
//!
//! Provides wire protocol implementations for client connections.
//!
//! # Protocols
//!
//! - **Bolt**: Neo4j-compatible binary protocol
//! - **HTTP**: REST API for web clients
//! - **gRPC**: High-performance binary protocol (future)

pub mod bolt;
pub mod http;
pub mod message;

pub use bolt::{BoltMessage, BoltVersion};
pub use http::{HttpMethod, HttpRequest, HttpResponse, StatusCode};
pub use message::{Request, Response};
