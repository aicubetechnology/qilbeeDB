//! QilbeeDB Storage Engine
//!
//! Provides persistent storage for the graph database using RocksDB.
//!
//! # Architecture
//!
//! The storage engine is organized into:
//! - Key-Value store abstraction
//! - Column families for different data types
//! - WAL for durability
//! - Transaction support
//!
//! # Column Families
//!
//! - `nodes` - Node data storage
//! - `relationships` - Relationship data storage
//! - `node_labels` - Label to node ID mapping
//! - `adjacency_out` - Outgoing adjacency lists
//! - `adjacency_in` - Incoming adjacency lists
//! - `properties` - Property indices
//! - `meta` - Database metadata

pub mod engine;
pub mod keys;
pub mod options;
pub mod transaction;

pub use engine::StorageEngine;
pub use options::StorageOptions;
pub use transaction::Transaction;
