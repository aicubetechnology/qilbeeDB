//! Entity identification types for QilbeeDB
//!
//! Provides strongly-typed identifiers for all graph entities.

use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

/// Internal numeric ID for efficient storage and lookup
pub type InternalId = u64;

/// Trait for all entity identifiers
pub trait EntityId: Clone + Copy + Eq + std::hash::Hash + fmt::Debug + fmt::Display {
    /// Create a new random ID
    fn new() -> Self;

    /// Create from internal numeric ID
    fn from_internal(id: InternalId) -> Self;

    /// Get the internal numeric representation
    fn as_internal(&self) -> InternalId;

    /// Create from UUID
    fn from_uuid(uuid: Uuid) -> Self;

    /// Get as UUID
    fn as_uuid(&self) -> Uuid;
}

/// Identifier for a graph instance
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GraphId(InternalId);

impl GraphId {
    /// Create a new graph ID from a name
    pub fn from_name(name: &str) -> Self {
        let hash = xxhash_rust::xxh3::xxh3_64(name.as_bytes());
        Self(hash)
    }
}

impl EntityId for GraphId {
    fn new() -> Self {
        Self(Uuid::new_v4().as_u128() as u64)
    }

    fn from_internal(id: InternalId) -> Self {
        Self(id)
    }

    fn as_internal(&self) -> InternalId {
        self.0
    }

    fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid.as_u128() as u64)
    }

    fn as_uuid(&self) -> Uuid {
        Uuid::from_u128(self.0 as u128)
    }
}

impl fmt::Debug for GraphId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "GraphId({})", self.0)
    }
}

impl fmt::Display for GraphId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Identifier for a node in the graph
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeId(InternalId);

impl EntityId for NodeId {
    fn new() -> Self {
        Self(Uuid::new_v4().as_u128() as u64)
    }

    fn from_internal(id: InternalId) -> Self {
        Self(id)
    }

    fn as_internal(&self) -> InternalId {
        self.0
    }

    fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid.as_u128() as u64)
    }

    fn as_uuid(&self) -> Uuid {
        Uuid::from_u128(self.0 as u128)
    }
}

impl fmt::Debug for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NodeId({})", self.0)
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Identifier for a relationship in the graph
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RelationshipId(InternalId);

impl EntityId for RelationshipId {
    fn new() -> Self {
        Self(Uuid::new_v4().as_u128() as u64)
    }

    fn from_internal(id: InternalId) -> Self {
        Self(id)
    }

    fn as_internal(&self) -> InternalId {
        self.0
    }

    fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid.as_u128() as u64)
    }

    fn as_uuid(&self) -> Uuid {
        Uuid::from_u128(self.0 as u128)
    }
}

impl fmt::Debug for RelationshipId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RelationshipId({})", self.0)
    }
}

impl fmt::Display for RelationshipId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Identifier generator for sequential IDs within a graph
#[derive(Debug)]
pub struct IdGenerator {
    next_node_id: std::sync::atomic::AtomicU64,
    next_rel_id: std::sync::atomic::AtomicU64,
}

impl IdGenerator {
    /// Create a new ID generator
    pub fn new() -> Self {
        Self {
            next_node_id: std::sync::atomic::AtomicU64::new(1),
            next_rel_id: std::sync::atomic::AtomicU64::new(1),
        }
    }

    /// Create with starting values (for recovery)
    pub fn with_start(node_start: u64, rel_start: u64) -> Self {
        Self {
            next_node_id: std::sync::atomic::AtomicU64::new(node_start),
            next_rel_id: std::sync::atomic::AtomicU64::new(rel_start),
        }
    }

    /// Generate the next node ID
    pub fn next_node_id(&self) -> NodeId {
        let id = self
            .next_node_id
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        NodeId::from_internal(id)
    }

    /// Generate the next relationship ID
    pub fn next_relationship_id(&self) -> RelationshipId {
        let id = self
            .next_rel_id
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        RelationshipId::from_internal(id)
    }

    /// Get current node ID counter value
    pub fn current_node_id(&self) -> u64 {
        self.next_node_id
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Get current relationship ID counter value
    pub fn current_relationship_id(&self) -> u64 {
        self.next_rel_id.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl Default for IdGenerator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_id_creation() {
        let id1 = NodeId::new();
        let id2 = NodeId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_node_id_from_internal() {
        let id = NodeId::from_internal(42);
        assert_eq!(id.as_internal(), 42);
    }

    #[test]
    fn test_graph_id_from_name() {
        let id1 = GraphId::from_name("test_graph");
        let id2 = GraphId::from_name("test_graph");
        let id3 = GraphId::from_name("other_graph");

        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_id_generator() {
        let id_gen = IdGenerator::new();

        let n1 = id_gen.next_node_id();
        let n2 = id_gen.next_node_id();
        assert_ne!(n1, n2);
        assert_eq!(n1.as_internal() + 1, n2.as_internal());

        let r1 = id_gen.next_relationship_id();
        let r2 = id_gen.next_relationship_id();
        assert_ne!(r1, r2);
    }

    #[test]
    fn test_id_generator_with_start() {
        let id_gen = IdGenerator::with_start(100, 200);
        assert_eq!(id_gen.next_node_id().as_internal(), 100);
        assert_eq!(id_gen.next_relationship_id().as_internal(), 200);
    }
}
