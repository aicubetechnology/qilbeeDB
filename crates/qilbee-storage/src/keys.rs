//! Key encoding for storage operations
//!
//! Provides efficient binary key encoding for all storage operations.

use qilbee_core::{EntityId, GraphId, NodeId, RelationshipId};

/// Prefix bytes for different key types
pub mod prefix {
    pub const NODE: u8 = 0x01;
    pub const RELATIONSHIP: u8 = 0x02;
    pub const LABEL_INDEX: u8 = 0x03;
    pub const ADJACENCY_OUT: u8 = 0x04;
    pub const ADJACENCY_IN: u8 = 0x05;
    pub const PROPERTY_INDEX: u8 = 0x06;
    pub const SCHEMA: u8 = 0x07;
    pub const META: u8 = 0x08;
    pub const CONSTRAINT: u8 = 0x09;
    pub const MEMORY_EPISODE: u8 = 0x10;
    pub const MEMORY_SEMANTIC: u8 = 0x11;
    pub const MEMORY_TEMPORAL: u8 = 0x12;
}

/// Key builder for storage operations
#[derive(Debug)]
pub struct KeyBuilder {
    buffer: Vec<u8>,
}

impl KeyBuilder {
    /// Create a new key builder with estimated capacity
    pub fn new(capacity: usize) -> Self {
        Self {
            buffer: Vec::with_capacity(capacity),
        }
    }

    /// Create a key builder for node keys
    pub fn node(graph_id: GraphId, node_id: NodeId) -> Vec<u8> {
        let mut builder = Self::new(17);
        builder.push_u8(prefix::NODE);
        builder.push_u64(graph_id.as_internal());
        builder.push_u64(node_id.as_internal());
        builder.finish()
    }

    /// Create a node prefix for scanning all nodes in a graph
    pub fn node_prefix(graph_id: GraphId) -> Vec<u8> {
        let mut builder = Self::new(9);
        builder.push_u8(prefix::NODE);
        builder.push_u64(graph_id.as_internal());
        builder.finish()
    }

    /// Create a key builder for relationship keys
    pub fn relationship(graph_id: GraphId, rel_id: RelationshipId) -> Vec<u8> {
        let mut builder = Self::new(17);
        builder.push_u8(prefix::RELATIONSHIP);
        builder.push_u64(graph_id.as_internal());
        builder.push_u64(rel_id.as_internal());
        builder.finish()
    }

    /// Create a label index key (label -> node_id)
    pub fn label_index(graph_id: GraphId, label: &str, node_id: NodeId) -> Vec<u8> {
        let mut builder = Self::new(17 + label.len());
        builder.push_u8(prefix::LABEL_INDEX);
        builder.push_u64(graph_id.as_internal());
        builder.push_string(label);
        builder.push_u64(node_id.as_internal());
        builder.finish()
    }

    /// Create a label index prefix for scanning all nodes with a label
    pub fn label_index_prefix(graph_id: GraphId, label: &str) -> Vec<u8> {
        let mut builder = Self::new(9 + label.len());
        builder.push_u8(prefix::LABEL_INDEX);
        builder.push_u64(graph_id.as_internal());
        builder.push_string(label);
        builder.finish()
    }

    /// Create an outgoing adjacency key
    pub fn adjacency_out(
        graph_id: GraphId,
        source: NodeId,
        rel_type: &str,
        rel_id: RelationshipId,
    ) -> Vec<u8> {
        let mut builder = Self::new(25 + rel_type.len());
        builder.push_u8(prefix::ADJACENCY_OUT);
        builder.push_u64(graph_id.as_internal());
        builder.push_u64(source.as_internal());
        builder.push_string(rel_type);
        builder.push_u64(rel_id.as_internal());
        builder.finish()
    }

    /// Create an outgoing adjacency prefix for scanning
    pub fn adjacency_out_prefix(graph_id: GraphId, source: NodeId) -> Vec<u8> {
        let mut builder = Self::new(17);
        builder.push_u8(prefix::ADJACENCY_OUT);
        builder.push_u64(graph_id.as_internal());
        builder.push_u64(source.as_internal());
        builder.finish()
    }

    /// Create an outgoing adjacency prefix with relationship type
    pub fn adjacency_out_type_prefix(
        graph_id: GraphId,
        source: NodeId,
        rel_type: &str,
    ) -> Vec<u8> {
        let mut builder = Self::new(17 + rel_type.len());
        builder.push_u8(prefix::ADJACENCY_OUT);
        builder.push_u64(graph_id.as_internal());
        builder.push_u64(source.as_internal());
        builder.push_string(rel_type);
        builder.finish()
    }

    /// Create an incoming adjacency key
    pub fn adjacency_in(
        graph_id: GraphId,
        target: NodeId,
        rel_type: &str,
        rel_id: RelationshipId,
    ) -> Vec<u8> {
        let mut builder = Self::new(25 + rel_type.len());
        builder.push_u8(prefix::ADJACENCY_IN);
        builder.push_u64(graph_id.as_internal());
        builder.push_u64(target.as_internal());
        builder.push_string(rel_type);
        builder.push_u64(rel_id.as_internal());
        builder.finish()
    }

    /// Create an incoming adjacency prefix for scanning
    pub fn adjacency_in_prefix(graph_id: GraphId, target: NodeId) -> Vec<u8> {
        let mut builder = Self::new(17);
        builder.push_u8(prefix::ADJACENCY_IN);
        builder.push_u64(graph_id.as_internal());
        builder.push_u64(target.as_internal());
        builder.finish()
    }

    /// Create a property index key
    pub fn property_index(
        graph_id: GraphId,
        label: &str,
        property: &str,
        value_hash: u64,
        entity_id: u64,
    ) -> Vec<u8> {
        let mut builder = Self::new(33 + label.len() + property.len());
        builder.push_u8(prefix::PROPERTY_INDEX);
        builder.push_u64(graph_id.as_internal());
        builder.push_string(label);
        builder.push_string(property);
        builder.push_u64(value_hash);
        builder.push_u64(entity_id);
        builder.finish()
    }

    /// Create a property index prefix for scanning all nodes with a label+property combination
    pub fn property_index_prefix(
        graph_id: GraphId,
        label: &str,
        property: &str,
    ) -> Vec<u8> {
        let mut builder = Self::new(17 + label.len() + property.len());
        builder.push_u8(prefix::PROPERTY_INDEX);
        builder.push_u64(graph_id.as_internal());
        builder.push_string(label);
        builder.push_string(property);
        builder.finish()
    }

    /// Create a property index prefix for scanning all nodes with a label+property+value
    pub fn property_index_value_prefix(
        graph_id: GraphId,
        label: &str,
        property: &str,
        value_hash: u64,
    ) -> Vec<u8> {
        let mut builder = Self::new(25 + label.len() + property.len());
        builder.push_u8(prefix::PROPERTY_INDEX);
        builder.push_u64(graph_id.as_internal());
        builder.push_string(label);
        builder.push_string(property);
        builder.push_u64(value_hash);
        builder.finish()
    }

    /// Create a schema key
    pub fn schema(graph_id: GraphId, schema_type: &str, name: &str) -> Vec<u8> {
        let mut builder = Self::new(9 + schema_type.len() + name.len());
        builder.push_u8(prefix::SCHEMA);
        builder.push_u64(graph_id.as_internal());
        builder.push_string(schema_type);
        builder.push_string(name);
        builder.finish()
    }

    /// Create a metadata key
    pub fn meta(key: &str) -> Vec<u8> {
        let mut builder = Self::new(1 + key.len());
        builder.push_u8(prefix::META);
        builder.push_string(key);
        builder.finish()
    }

    /// Create a graph metadata key
    pub fn graph_meta(graph_id: GraphId, key: &str) -> Vec<u8> {
        let mut builder = Self::new(9 + key.len());
        builder.push_u8(prefix::META);
        builder.push_u64(graph_id.as_internal());
        builder.push_string(key);
        builder.finish()
    }

    /// Create an episodic memory key
    pub fn memory_episode(
        graph_id: GraphId,
        agent_id: &str,
        event_time_millis: i64,
        episode_id: u64,
    ) -> Vec<u8> {
        let mut builder = Self::new(25 + agent_id.len());
        builder.push_u8(prefix::MEMORY_EPISODE);
        builder.push_u64(graph_id.as_internal());
        builder.push_string(agent_id);
        builder.push_i64(event_time_millis);
        builder.push_u64(episode_id);
        builder.finish()
    }

    /// Create an episodic memory prefix for time-range queries
    pub fn memory_episode_prefix(graph_id: GraphId, agent_id: &str) -> Vec<u8> {
        let mut builder = Self::new(9 + agent_id.len());
        builder.push_u8(prefix::MEMORY_EPISODE);
        builder.push_u64(graph_id.as_internal());
        builder.push_string(agent_id);
        builder.finish()
    }

    // Builder methods

    fn push_u8(&mut self, val: u8) {
        self.buffer.push(val);
    }

    fn push_u64(&mut self, val: u64) {
        self.buffer.extend_from_slice(&val.to_be_bytes());
    }

    fn push_i64(&mut self, val: i64) {
        self.buffer.extend_from_slice(&val.to_be_bytes());
    }

    fn push_string(&mut self, s: &str) {
        // Length-prefixed string
        let bytes = s.as_bytes();
        self.buffer
            .extend_from_slice(&(bytes.len() as u16).to_be_bytes());
        self.buffer.extend_from_slice(bytes);
    }

    fn finish(self) -> Vec<u8> {
        self.buffer
    }
}

/// Key decoder for parsing stored keys
pub struct KeyDecoder<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> KeyDecoder<'a> {
    /// Create a new decoder
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    /// Read a u8
    pub fn read_u8(&mut self) -> Option<u8> {
        if self.pos < self.data.len() {
            let val = self.data[self.pos];
            self.pos += 1;
            Some(val)
        } else {
            None
        }
    }

    /// Read a u64
    pub fn read_u64(&mut self) -> Option<u64> {
        if self.pos + 8 <= self.data.len() {
            let bytes: [u8; 8] = self.data[self.pos..self.pos + 8].try_into().ok()?;
            self.pos += 8;
            Some(u64::from_be_bytes(bytes))
        } else {
            None
        }
    }

    /// Read an i64
    pub fn read_i64(&mut self) -> Option<i64> {
        if self.pos + 8 <= self.data.len() {
            let bytes: [u8; 8] = self.data[self.pos..self.pos + 8].try_into().ok()?;
            self.pos += 8;
            Some(i64::from_be_bytes(bytes))
        } else {
            None
        }
    }

    /// Read a length-prefixed string
    pub fn read_string(&mut self) -> Option<&'a str> {
        if self.pos + 2 > self.data.len() {
            return None;
        }

        let len_bytes: [u8; 2] = self.data[self.pos..self.pos + 2].try_into().ok()?;
        let len = u16::from_be_bytes(len_bytes) as usize;
        self.pos += 2;

        if self.pos + len > self.data.len() {
            return None;
        }

        let s = std::str::from_utf8(&self.data[self.pos..self.pos + len]).ok()?;
        self.pos += len;
        Some(s)
    }

    /// Get remaining bytes
    pub fn remaining(&self) -> &'a [u8] {
        &self.data[self.pos..]
    }

    /// Get current position
    pub fn position(&self) -> usize {
        self.pos
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qilbee_core::EntityId;

    #[test]
    fn test_node_key() {
        let graph_id = GraphId::from_name("test");
        let node_id = NodeId::from_internal(42);

        let key = KeyBuilder::node(graph_id, node_id);

        assert_eq!(key[0], prefix::NODE);
        assert_eq!(key.len(), 17);

        let mut decoder = KeyDecoder::new(&key);
        assert_eq!(decoder.read_u8(), Some(prefix::NODE));
        assert_eq!(decoder.read_u64(), Some(graph_id.as_internal()));
        assert_eq!(decoder.read_u64(), Some(42));
    }

    #[test]
    fn test_label_index_key() {
        let graph_id = GraphId::from_name("test");
        let node_id = NodeId::from_internal(123);

        let key = KeyBuilder::label_index(graph_id, "Person", node_id);

        let mut decoder = KeyDecoder::new(&key);
        assert_eq!(decoder.read_u8(), Some(prefix::LABEL_INDEX));
        assert_eq!(decoder.read_u64(), Some(graph_id.as_internal()));
        assert_eq!(decoder.read_string(), Some("Person"));
        assert_eq!(decoder.read_u64(), Some(123));
    }

    #[test]
    fn test_adjacency_key() {
        let graph_id = GraphId::from_name("test");
        let source = NodeId::from_internal(1);
        let rel_id = RelationshipId::from_internal(100);

        let key = KeyBuilder::adjacency_out(graph_id, source, "KNOWS", rel_id);

        let mut decoder = KeyDecoder::new(&key);
        assert_eq!(decoder.read_u8(), Some(prefix::ADJACENCY_OUT));
        assert_eq!(decoder.read_u64(), Some(graph_id.as_internal()));
        assert_eq!(decoder.read_u64(), Some(1));
        assert_eq!(decoder.read_string(), Some("KNOWS"));
        assert_eq!(decoder.read_u64(), Some(100));
    }

    #[test]
    fn test_key_prefix_scanning() {
        let graph_id = GraphId::from_name("test");
        let source = NodeId::from_internal(1);

        let prefix = KeyBuilder::adjacency_out_prefix(graph_id, source);
        let full_key = KeyBuilder::adjacency_out(
            graph_id,
            source,
            "KNOWS",
            RelationshipId::from_internal(100),
        );

        // Full key should start with prefix
        assert!(full_key.starts_with(&prefix));
    }
}
