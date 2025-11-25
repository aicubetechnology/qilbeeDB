//! QilbeeDB - Agent-first graph database
//!
//! This is the main library crate that re-exports all QilbeeDB components.

pub use qilbee_core as core;
pub use qilbee_graph as graph;
pub use qilbee_memory as memory;
pub use qilbee_protocol as protocol;
pub use qilbee_query as query;
pub use qilbee_server as server;
pub use qilbee_storage as storage;

// Re-export commonly used types
pub use qilbee_core::{
    BiTemporal, EntityId, Error, EventTime, GraphId, IdGenerator, Node, NodeId, Property,
    PropertyValue, Relationship, RelationshipId, Result, TransactionTime,
};

pub use qilbee_graph::{Database, Graph};
pub use qilbee_memory::{AgentMemory, Episode, EpisodeContent, EpisodeType, MemoryConfig};
pub use qilbee_storage::{StorageEngine, StorageOptions};
