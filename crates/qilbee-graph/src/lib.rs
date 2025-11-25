//! QilbeeDB Graph Engine
//!
//! Provides high-level graph operations built on top of the storage engine.
//!
//! # Overview
//!
//! The graph engine provides:
//! - Graph instance management (create, open, delete graphs)
//! - High-level node and relationship operations
//! - Schema management (constraints, indices)
//! - Graph algorithms

pub mod database;
pub mod graph;
pub mod schema;

pub use database::Database;
pub use graph::Graph;
pub use schema::{Constraint, ConstraintType, Index, IndexType, Schema};
