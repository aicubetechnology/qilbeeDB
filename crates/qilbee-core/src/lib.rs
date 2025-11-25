//! QilbeeDB Core Library
//!
//! This crate provides the fundamental types, traits, and error handling
//! for the QilbeeDB graph database system.
//!
//! # Overview
//!
//! QilbeeDB is an agent-first graph database designed for AI applications,
//! implementing bi-temporal data models and native support for agent memory.
//!
//! # Modules
//!
//! - `types` - Core data types (Node, Relationship, Property, etc.)
//! - `error` - Error types and result aliases
//! - `id` - Entity identification and generation
//! - `temporal` - Bi-temporal data handling

pub mod error;
pub mod id;
pub mod property;
pub mod temporal;
pub mod types;

pub use error::{Error, Result};
pub use id::{EntityId, GraphId, IdGenerator, NodeId, RelationshipId};
pub use property::{Property, PropertyValue};
pub use temporal::{BiTemporal, EventTime, TransactionTime};
pub use types::{Direction, Label, Node, Relationship};
