//! Storage engine implementation using RocksDB

use crate::keys::KeyBuilder;
use crate::options::StorageOptions;
use qilbee_core::{EntityId, Error, GraphId, Node, NodeId, PropertyValue, Relationship, RelationshipId, Result};
use rocksdb::{ColumnFamily, ColumnFamilyDescriptor, Options, WriteBatch, DB};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use tracing::{debug, info, warn};

/// Compute a hash for a property value for indexing
fn hash_property_value(value: &PropertyValue) -> u64 {
    let mut hasher = DefaultHasher::new();
    match value {
        PropertyValue::Null => 0_u64.hash(&mut hasher),
        PropertyValue::Boolean(b) => b.hash(&mut hasher),
        PropertyValue::Integer(i) => i.hash(&mut hasher),
        PropertyValue::Float(f) => f.to_bits().hash(&mut hasher),
        PropertyValue::String(s) => s.hash(&mut hasher),
        PropertyValue::Array(arr) => {
            for item in arr {
                hash_property_value(item).hash(&mut hasher);
            }
        }
        PropertyValue::Map(m) => {
            for (k, v) in m {
                k.hash(&mut hasher);
                hash_property_value(v).hash(&mut hasher);
            }
        }
        PropertyValue::Bytes(b) => b.hash(&mut hasher),
        PropertyValue::Date(d) => d.hash(&mut hasher),
        PropertyValue::Time(t) => t.hash(&mut hasher),
        PropertyValue::DateTime(dt) => dt.hash(&mut hasher),
        PropertyValue::Duration(d) => d.hash(&mut hasher),
        PropertyValue::Point2D { x, y, srid } => {
            x.to_bits().hash(&mut hasher);
            y.to_bits().hash(&mut hasher);
            srid.hash(&mut hasher);
        }
        PropertyValue::Point3D { x, y, z, srid } => {
            x.to_bits().hash(&mut hasher);
            y.to_bits().hash(&mut hasher);
            z.to_bits().hash(&mut hasher);
            srid.hash(&mut hasher);
        }
    }
    hasher.finish()
}

/// Compare two property values for ordering
/// Returns -1 if a < b, 0 if a == b, 1 if a > b
/// For incompatible types, returns 0 (equal)
fn compare_property_values(a: &PropertyValue, b: &PropertyValue) -> i32 {
    match (a, b) {
        (PropertyValue::Null, PropertyValue::Null) => 0,
        (PropertyValue::Null, _) => -1,
        (_, PropertyValue::Null) => 1,

        (PropertyValue::Boolean(a), PropertyValue::Boolean(b)) => a.cmp(b) as i32,
        (PropertyValue::Integer(a), PropertyValue::Integer(b)) => a.cmp(b) as i32,
        (PropertyValue::Float(a), PropertyValue::Float(b)) => {
            a.partial_cmp(b).map(|o| o as i32).unwrap_or(0)
        }
        (PropertyValue::String(a), PropertyValue::String(b)) => a.cmp(b) as i32,

        // Allow integer/float comparison
        (PropertyValue::Integer(a), PropertyValue::Float(b)) => {
            (*a as f64).partial_cmp(b).map(|o| o as i32).unwrap_or(0)
        }
        (PropertyValue::Float(a), PropertyValue::Integer(b)) => {
            a.partial_cmp(&(*b as f64)).map(|o| o as i32).unwrap_or(0)
        }

        // Date/time comparisons
        (PropertyValue::Date(a), PropertyValue::Date(b)) => a.cmp(b) as i32,
        (PropertyValue::Time(a), PropertyValue::Time(b)) => a.cmp(b) as i32,
        (PropertyValue::DateTime(a), PropertyValue::DateTime(b)) => a.cmp(b) as i32,
        (PropertyValue::Duration(a), PropertyValue::Duration(b)) => a.cmp(b) as i32,

        // Incompatible types - treat as equal for filtering purposes
        _ => 0,
    }
}

/// Column family names
pub mod cf {
    pub const NODES: &str = "nodes";
    pub const RELATIONSHIPS: &str = "relationships";
    pub const LABEL_INDEX: &str = "label_index";
    pub const ADJACENCY_OUT: &str = "adjacency_out";
    pub const ADJACENCY_IN: &str = "adjacency_in";
    pub const PROPERTY_INDEX: &str = "property_index";
    pub const SCHEMA: &str = "schema";
    pub const META: &str = "meta";
    pub const MEMORY: &str = "memory";
}

/// All column families used by QilbeeDB
pub const COLUMN_FAMILIES: &[&str] = &[
    cf::NODES,
    cf::RELATIONSHIPS,
    cf::LABEL_INDEX,
    cf::ADJACENCY_OUT,
    cf::ADJACENCY_IN,
    cf::PROPERTY_INDEX,
    cf::SCHEMA,
    cf::META,
    cf::MEMORY,
];

/// The main storage engine for QilbeeDB
pub struct StorageEngine {
    db: Arc<DB>,
    options: StorageOptions,
}

impl StorageEngine {
    /// Open or create a new storage engine
    pub fn open(options: StorageOptions) -> Result<Self> {
        info!("Opening storage engine at {:?}", options.path);

        let mut db_opts = Options::default();
        db_opts.create_if_missing(options.create_if_missing);
        db_opts.create_missing_column_families(true);
        db_opts.set_write_buffer_size(options.write_buffer_size);
        db_opts.set_max_write_buffer_number(options.max_write_buffer_number);
        db_opts.set_target_file_size_base(options.target_file_size_base);
        db_opts.set_max_bytes_for_level_base(options.max_bytes_for_level_base);
        db_opts.set_max_background_jobs(options.max_background_jobs);

        if options.enable_compression {
            db_opts.set_compression_type(rocksdb::DBCompressionType::Lz4);
        }

        // Create column family descriptors
        let cf_descriptors: Vec<ColumnFamilyDescriptor> = COLUMN_FAMILIES
            .iter()
            .map(|name| {
                let mut cf_opts = Options::default();
                if options.enable_bloom_filter {
                    let mut block_opts = rocksdb::BlockBasedOptions::default();
                    block_opts
                        .set_bloom_filter(options.bloom_filter_bits_per_key as f64, false);
                    cf_opts.set_block_based_table_factory(&block_opts);
                }
                ColumnFamilyDescriptor::new(*name, cf_opts)
            })
            .collect();

        let db = DB::open_cf_descriptors(&db_opts, &options.path, cf_descriptors)
            .map_err(|e| Error::Storage(e.to_string()))?;

        info!("Storage engine opened successfully");

        Ok(Self {
            db: Arc::new(db),
            options,
        })
    }

    /// Get a reference to a column family
    fn cf(&self, name: &str) -> Result<&ColumnFamily> {
        self.db
            .cf_handle(name)
            .ok_or_else(|| Error::Internal(format!("Column family not found: {}", name)))
    }

    // ========== Node Operations ==========

    /// Store a node
    pub fn put_node(&self, graph_id: GraphId, node: &Node) -> Result<()> {
        let key = KeyBuilder::node(graph_id, node.id);
        let value = bincode::serialize(node).map_err(|e| Error::Serialization(e.to_string()))?;

        let mut batch = WriteBatch::default();

        // Store node data
        let cf = self.cf(cf::NODES)?;
        batch.put_cf(&cf, &key, &value);

        // Update label indices
        let label_cf = self.cf(cf::LABEL_INDEX)?;
        for label in &node.labels {
            let label_key = KeyBuilder::label_index(graph_id, label.name(), node.id);
            batch.put_cf(&label_cf, &label_key, &[]);
        }

        // Update property indices for each label+property combination
        let prop_cf = self.cf(cf::PROPERTY_INDEX)?;
        for label in &node.labels {
            for (prop_name, prop_value) in node.properties.iter() {
                let value_hash = hash_property_value(prop_value);
                let prop_key = KeyBuilder::property_index(
                    graph_id,
                    label.name(),
                    prop_name,
                    value_hash,
                    node.id.as_internal(),
                );
                // Store the serialized property value for retrieval
                let prop_value_bytes = bincode::serialize(prop_value)
                    .map_err(|e| Error::Serialization(e.to_string()))?;
                batch.put_cf(&prop_cf, &prop_key, &prop_value_bytes);
            }
        }

        self.db
            .write(batch)
            .map_err(|e| Error::Storage(e.to_string()))?;

        debug!("Stored node {:?} in graph {:?}", node.id, graph_id);
        Ok(())
    }

    /// Get a node by ID
    pub fn get_node(&self, graph_id: GraphId, node_id: NodeId) -> Result<Option<Node>> {
        let key = KeyBuilder::node(graph_id, node_id);
        let cf = self.cf(cf::NODES)?;

        match self.db.get_cf(&cf, &key) {
            Ok(Some(value)) => {
                let node: Node = bincode::deserialize(&value)
                    .map_err(|e| Error::Deserialization(e.to_string()))?;
                Ok(Some(node))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(Error::Storage(e.to_string())),
        }
    }

    /// Delete a node
    pub fn delete_node(&self, graph_id: GraphId, node_id: NodeId) -> Result<bool> {
        // First get the node to remove label and property indices
        let node = match self.get_node(graph_id, node_id)? {
            Some(n) => n,
            None => return Ok(false),
        };

        let mut batch = WriteBatch::default();

        // Remove node
        let key = KeyBuilder::node(graph_id, node_id);
        let cf = self.cf(cf::NODES)?;
        batch.delete_cf(&cf, &key);

        // Remove label indices
        let label_cf = self.cf(cf::LABEL_INDEX)?;
        for label in &node.labels {
            let label_key = KeyBuilder::label_index(graph_id, label.name(), node_id);
            batch.delete_cf(&label_cf, &label_key);
        }

        // Remove property indices
        let prop_cf = self.cf(cf::PROPERTY_INDEX)?;
        for label in &node.labels {
            for (prop_name, prop_value) in node.properties.iter() {
                let value_hash = hash_property_value(prop_value);
                let prop_key = KeyBuilder::property_index(
                    graph_id,
                    label.name(),
                    prop_name,
                    value_hash,
                    node_id.as_internal(),
                );
                batch.delete_cf(&prop_cf, &prop_key);
            }
        }

        self.db
            .write(batch)
            .map_err(|e| Error::Storage(e.to_string()))?;

        debug!("Deleted node {:?} from graph {:?}", node_id, graph_id);
        Ok(true)
    }

    /// Get all nodes in a graph
    pub fn get_all_nodes(&self, graph_id: GraphId) -> Result<Vec<Node>> {
        let prefix = KeyBuilder::node_prefix(graph_id);
        let cf = self.cf(cf::NODES)?;

        let mut nodes = Vec::new();
        let iter = self.db.prefix_iterator_cf(&cf, &prefix);

        for item in iter {
            let (key, value) = item.map_err(|e| Error::Storage(e.to_string()))?;

            // Check if we're still in the prefix
            if !key.starts_with(&prefix) {
                break;
            }

            // Deserialize the node directly from the value
            let node: Node = bincode::deserialize(&value)
                .map_err(|e| Error::Deserialization(e.to_string()))?;
            nodes.push(node);
        }

        Ok(nodes)
    }

    /// Get all nodes with a specific label
    pub fn get_nodes_by_label(&self, graph_id: GraphId, label: &str) -> Result<Vec<Node>> {
        let prefix = KeyBuilder::label_index_prefix(graph_id, label);
        let cf = self.cf(cf::LABEL_INDEX)?;

        let mut nodes = Vec::new();
        let iter = self.db.prefix_iterator_cf(&cf, &prefix);

        for item in iter {
            let (key, _) = item.map_err(|e| Error::Storage(e.to_string()))?;

            // Check if we're still in the prefix
            if !key.starts_with(&prefix) {
                break;
            }

            // Extract node ID from the end of the key
            if key.len() >= 8 {
                let node_id_bytes: [u8; 8] = key[key.len() - 8..].try_into().unwrap();
                let node_id = NodeId::from_internal(u64::from_be_bytes(node_id_bytes));

                if let Some(node) = self.get_node(graph_id, node_id)? {
                    nodes.push(node);
                }
            }
        }

        Ok(nodes)
    }

    /// Get nodes by property value using the property index
    /// This is an efficient lookup using the property index
    pub fn get_nodes_by_property(
        &self,
        graph_id: GraphId,
        label: &str,
        property: &str,
        value: &PropertyValue,
    ) -> Result<Vec<Node>> {
        let value_hash = hash_property_value(value);
        let prefix = KeyBuilder::property_index_value_prefix(graph_id, label, property, value_hash);
        let cf = self.cf(cf::PROPERTY_INDEX)?;

        let mut nodes = Vec::new();
        let iter = self.db.prefix_iterator_cf(&cf, &prefix);

        for item in iter {
            let (key, _) = item.map_err(|e| Error::Storage(e.to_string()))?;

            // Check if we're still in the prefix
            if !key.starts_with(&prefix) {
                break;
            }

            // Extract node ID from the end of the key (last 8 bytes)
            if key.len() >= 8 {
                let node_id_bytes: [u8; 8] = key[key.len() - 8..].try_into().unwrap();
                let node_id = NodeId::from_internal(u64::from_be_bytes(node_id_bytes));

                if let Some(node) = self.get_node(graph_id, node_id)? {
                    // Verify the property value matches (in case of hash collision)
                    if let Some(actual_value) = node.properties.get(property) {
                        if actual_value == value {
                            nodes.push(node);
                        }
                    }
                }
            }
        }

        Ok(nodes)
    }

    /// Get nodes that have a specific property (any value)
    pub fn get_nodes_with_property(
        &self,
        graph_id: GraphId,
        label: &str,
        property: &str,
    ) -> Result<Vec<Node>> {
        let prefix = KeyBuilder::property_index_prefix(graph_id, label, property);
        let cf = self.cf(cf::PROPERTY_INDEX)?;

        let mut node_ids = std::collections::HashSet::new();
        let iter = self.db.prefix_iterator_cf(&cf, &prefix);

        for item in iter {
            let (key, _) = item.map_err(|e| Error::Storage(e.to_string()))?;

            // Check if we're still in the prefix
            if !key.starts_with(&prefix) {
                break;
            }

            // Extract node ID from the end of the key (last 8 bytes)
            if key.len() >= 8 {
                let node_id_bytes: [u8; 8] = key[key.len() - 8..].try_into().unwrap();
                let node_id = NodeId::from_internal(u64::from_be_bytes(node_id_bytes));
                node_ids.insert(node_id);
            }
        }

        // Fetch all unique nodes
        let mut nodes = Vec::new();
        for node_id in node_ids {
            if let Some(node) = self.get_node(graph_id, node_id)? {
                nodes.push(node);
            }
        }

        Ok(nodes)
    }

    /// Get nodes by property range (for comparable values)
    /// Returns nodes where property value falls within [min, max] range
    pub fn get_nodes_by_property_range(
        &self,
        graph_id: GraphId,
        label: &str,
        property: &str,
        min_value: Option<&PropertyValue>,
        max_value: Option<&PropertyValue>,
    ) -> Result<Vec<Node>> {
        // For range queries, we need to scan all nodes with the property
        // and filter by range. This is less efficient than B-tree but works.
        let all_nodes = self.get_nodes_with_property(graph_id, label, property)?;

        let nodes: Vec<Node> = all_nodes
            .into_iter()
            .filter(|node| {
                if let Some(value) = node.properties.get(property) {
                    let above_min = min_value.map_or(true, |min| compare_property_values(value, min) >= 0);
                    let below_max = max_value.map_or(true, |max| compare_property_values(value, max) <= 0);
                    above_min && below_max
                } else {
                    false
                }
            })
            .collect();

        Ok(nodes)
    }

    // ========== Relationship Operations ==========

    /// Store a relationship
    pub fn put_relationship(&self, graph_id: GraphId, rel: &Relationship) -> Result<()> {
        let key = KeyBuilder::relationship(graph_id, rel.id);
        let value = bincode::serialize(rel).map_err(|e| Error::Serialization(e.to_string()))?;

        let mut batch = WriteBatch::default();

        // Store relationship data
        let cf = self.cf(cf::RELATIONSHIPS)?;
        batch.put_cf(&cf, &key, &value);

        // Store adjacency - outgoing
        let adj_out_cf = self.cf(cf::ADJACENCY_OUT)?;
        let adj_out_key =
            KeyBuilder::adjacency_out(graph_id, rel.source, rel.rel_type.name(), rel.id);
        let target_bytes = rel.target.as_internal().to_be_bytes();
        batch.put_cf(&adj_out_cf, &adj_out_key, &target_bytes);

        // Store adjacency - incoming
        let adj_in_cf = self.cf(cf::ADJACENCY_IN)?;
        let adj_in_key =
            KeyBuilder::adjacency_in(graph_id, rel.target, rel.rel_type.name(), rel.id);
        let source_bytes = rel.source.as_internal().to_be_bytes();
        batch.put_cf(&adj_in_cf, &adj_in_key, &source_bytes);

        self.db
            .write(batch)
            .map_err(|e| Error::Storage(e.to_string()))?;

        debug!(
            "Stored relationship {:?} ({:?})-[:{}]->({:?})",
            rel.id,
            rel.source,
            rel.rel_type.name(),
            rel.target
        );
        Ok(())
    }

    /// Get a relationship by ID
    pub fn get_relationship(
        &self,
        graph_id: GraphId,
        rel_id: RelationshipId,
    ) -> Result<Option<Relationship>> {
        let key = KeyBuilder::relationship(graph_id, rel_id);
        let cf = self.cf(cf::RELATIONSHIPS)?;

        match self.db.get_cf(&cf, &key) {
            Ok(Some(value)) => {
                let rel: Relationship = bincode::deserialize(&value)
                    .map_err(|e| Error::Deserialization(e.to_string()))?;
                Ok(Some(rel))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(Error::Storage(e.to_string())),
        }
    }

    /// Delete a relationship
    pub fn delete_relationship(
        &self,
        graph_id: GraphId,
        rel_id: RelationshipId,
    ) -> Result<bool> {
        // First get the relationship to remove adjacency indices
        let rel = match self.get_relationship(graph_id, rel_id)? {
            Some(r) => r,
            None => return Ok(false),
        };

        let mut batch = WriteBatch::default();

        // Remove relationship
        let key = KeyBuilder::relationship(graph_id, rel_id);
        let cf = self.cf(cf::RELATIONSHIPS)?;
        batch.delete_cf(&cf, &key);

        // Remove adjacency - outgoing
        let adj_out_cf = self.cf(cf::ADJACENCY_OUT)?;
        let adj_out_key =
            KeyBuilder::adjacency_out(graph_id, rel.source, rel.rel_type.name(), rel.id);
        batch.delete_cf(&adj_out_cf, &adj_out_key);

        // Remove adjacency - incoming
        let adj_in_cf = self.cf(cf::ADJACENCY_IN)?;
        let adj_in_key =
            KeyBuilder::adjacency_in(graph_id, rel.target, rel.rel_type.name(), rel.id);
        batch.delete_cf(&adj_in_cf, &adj_in_key);

        self.db
            .write(batch)
            .map_err(|e| Error::Storage(e.to_string()))?;

        debug!("Deleted relationship {:?} from graph {:?}", rel_id, graph_id);
        Ok(true)
    }

    /// Get outgoing relationships from a node
    pub fn get_outgoing_relationships(
        &self,
        graph_id: GraphId,
        node_id: NodeId,
    ) -> Result<Vec<Relationship>> {
        let prefix = KeyBuilder::adjacency_out_prefix(graph_id, node_id);
        let cf = self.cf(cf::ADJACENCY_OUT)?;

        let mut relationships = Vec::new();
        let iter = self.db.prefix_iterator_cf(&cf, &prefix);

        for item in iter {
            let (key, _) = item.map_err(|e| Error::Storage(e.to_string()))?;

            if !key.starts_with(&prefix) {
                break;
            }

            // Extract relationship ID from the end of the key
            if key.len() >= 8 {
                let rel_id_bytes: [u8; 8] = key[key.len() - 8..].try_into().unwrap();
                let rel_id = RelationshipId::from_internal(u64::from_be_bytes(rel_id_bytes));

                if let Some(rel) = self.get_relationship(graph_id, rel_id)? {
                    relationships.push(rel);
                }
            }
        }

        Ok(relationships)
    }

    /// Get incoming relationships to a node
    pub fn get_incoming_relationships(
        &self,
        graph_id: GraphId,
        node_id: NodeId,
    ) -> Result<Vec<Relationship>> {
        let prefix = KeyBuilder::adjacency_in_prefix(graph_id, node_id);
        let cf = self.cf(cf::ADJACENCY_IN)?;

        let mut relationships = Vec::new();
        let iter = self.db.prefix_iterator_cf(&cf, &prefix);

        for item in iter {
            let (key, _) = item.map_err(|e| Error::Storage(e.to_string()))?;

            if !key.starts_with(&prefix) {
                break;
            }

            // Extract relationship ID from the end of the key
            if key.len() >= 8 {
                let rel_id_bytes: [u8; 8] = key[key.len() - 8..].try_into().unwrap();
                let rel_id = RelationshipId::from_internal(u64::from_be_bytes(rel_id_bytes));

                if let Some(rel) = self.get_relationship(graph_id, rel_id)? {
                    relationships.push(rel);
                }
            }
        }

        Ok(relationships)
    }

    // ========== Metadata Operations ==========

    /// Store metadata
    pub fn put_meta(&self, key: &str, value: &[u8]) -> Result<()> {
        let storage_key = KeyBuilder::meta(key);
        let cf = self.cf(cf::META)?;

        self.db
            .put_cf(&cf, &storage_key, value)
            .map_err(|e| Error::Storage(e.to_string()))?;

        Ok(())
    }

    /// Get metadata
    pub fn get_meta(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let storage_key = KeyBuilder::meta(key);
        let cf = self.cf(cf::META)?;

        self.db
            .get_cf(&cf, &storage_key)
            .map_err(|e| Error::Storage(e.to_string()))
    }

    // ========== Transaction Operations ==========

    /// Begin a new transaction for a graph
    pub fn begin_transaction(&self, graph_id: GraphId) -> Result<crate::transaction::Transaction> {
        Ok(crate::transaction::Transaction::new(self.clone(), graph_id))
    }

    // ========== Utility Operations ==========

    /// Flush all in-memory data to disk
    pub fn flush(&self) -> Result<()> {
        self.db
            .flush()
            .map_err(|e| Error::Storage(e.to_string()))?;
        info!("Storage engine flushed");
        Ok(())
    }

    /// Get database statistics
    pub fn stats(&self) -> String {
        self.db
            .property_value("rocksdb.stats")
            .unwrap_or_default()
            .unwrap_or_default()
    }

    /// Compact the database
    pub fn compact(&self) -> Result<()> {
        for cf_name in COLUMN_FAMILIES {
            if let Ok(cf) = self.cf(cf_name) {
                self.db.compact_range_cf(&cf, None::<&[u8]>, None::<&[u8]>);
            }
        }
        info!("Storage engine compacted");
        Ok(())
    }
}

impl Clone for StorageEngine {
    fn clone(&self) -> Self {
        Self {
            db: Arc::clone(&self.db),
            options: self.options.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qilbee_core::{EntityId, IdGenerator, Property};
    use tempfile::TempDir;

    fn create_test_engine() -> (StorageEngine, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let options = StorageOptions::for_testing(temp_dir.path());
        let engine = StorageEngine::open(options).unwrap();
        (engine, temp_dir)
    }

    #[test]
    fn test_open_engine() {
        let (engine, _dir) = create_test_engine();
        assert!(engine.get_meta("test").unwrap().is_none());
    }

    #[test]
    fn test_node_crud() {
        let (engine, _dir) = create_test_engine();
        let id_gen = IdGenerator::new();
        let graph_id = GraphId::from_name("test");

        // Create node
        let mut node = Node::with_labels(id_gen.next_node_id(), ["Person"]);
        node.set_property("name", "Alice");

        // Store
        engine.put_node(graph_id, &node).unwrap();

        // Read
        let retrieved = engine.get_node(graph_id, node.id).unwrap().unwrap();
        assert_eq!(retrieved.id, node.id);
        assert!(retrieved.has_label_name("Person"));
        assert_eq!(
            retrieved.get_property("name").and_then(|v| v.as_str()),
            Some("Alice")
        );

        // Delete
        assert!(engine.delete_node(graph_id, node.id).unwrap());
        assert!(engine.get_node(graph_id, node.id).unwrap().is_none());
    }

    #[test]
    fn test_nodes_by_label() {
        let (engine, _dir) = create_test_engine();
        let id_gen = IdGenerator::new();
        let graph_id = GraphId::from_name("test");

        // Create nodes with different labels
        let node1 = Node::with_labels(id_gen.next_node_id(), ["Person"]);
        let node2 = Node::with_labels(id_gen.next_node_id(), ["Person"]);
        let node3 = Node::with_labels(id_gen.next_node_id(), ["Company"]);

        engine.put_node(graph_id, &node1).unwrap();
        engine.put_node(graph_id, &node2).unwrap();
        engine.put_node(graph_id, &node3).unwrap();

        // Query by label
        let people = engine.get_nodes_by_label(graph_id, "Person").unwrap();
        assert_eq!(people.len(), 2);

        let companies = engine.get_nodes_by_label(graph_id, "Company").unwrap();
        assert_eq!(companies.len(), 1);
    }

    #[test]
    fn test_relationship_crud() {
        let (engine, _dir) = create_test_engine();
        let id_gen = IdGenerator::new();
        let graph_id = GraphId::from_name("test");

        // Create nodes
        let node1 = Node::with_labels(id_gen.next_node_id(), ["Person"]);
        let node2 = Node::with_labels(id_gen.next_node_id(), ["Person"]);
        engine.put_node(graph_id, &node1).unwrap();
        engine.put_node(graph_id, &node2).unwrap();

        // Create relationship
        let rel = Relationship::new(id_gen.next_relationship_id(), "KNOWS", node1.id, node2.id);
        engine.put_relationship(graph_id, &rel).unwrap();

        // Read
        let retrieved = engine.get_relationship(graph_id, rel.id).unwrap().unwrap();
        assert_eq!(retrieved.id, rel.id);
        assert_eq!(retrieved.source, node1.id);
        assert_eq!(retrieved.target, node2.id);
        assert_eq!(retrieved.rel_type.name(), "KNOWS");

        // Delete
        assert!(engine.delete_relationship(graph_id, rel.id).unwrap());
        assert!(engine.get_relationship(graph_id, rel.id).unwrap().is_none());
    }

    #[test]
    fn test_adjacency_queries() {
        let (engine, _dir) = create_test_engine();
        let id_gen = IdGenerator::new();
        let graph_id = GraphId::from_name("test");

        // Create nodes
        let node1 = Node::with_labels(id_gen.next_node_id(), ["Person"]);
        let node2 = Node::with_labels(id_gen.next_node_id(), ["Person"]);
        let node3 = Node::with_labels(id_gen.next_node_id(), ["Person"]);
        engine.put_node(graph_id, &node1).unwrap();
        engine.put_node(graph_id, &node2).unwrap();
        engine.put_node(graph_id, &node3).unwrap();

        // Create relationships: node1 -> node2, node1 -> node3
        let rel1 = Relationship::new(id_gen.next_relationship_id(), "KNOWS", node1.id, node2.id);
        let rel2 = Relationship::new(id_gen.next_relationship_id(), "KNOWS", node1.id, node3.id);
        engine.put_relationship(graph_id, &rel1).unwrap();
        engine.put_relationship(graph_id, &rel2).unwrap();

        // Query outgoing from node1
        let outgoing = engine.get_outgoing_relationships(graph_id, node1.id).unwrap();
        assert_eq!(outgoing.len(), 2);

        // Query incoming to node2
        let incoming = engine.get_incoming_relationships(graph_id, node2.id).unwrap();
        assert_eq!(incoming.len(), 1);
        assert_eq!(incoming[0].source, node1.id);
    }

    #[test]
    fn test_metadata() {
        let (engine, _dir) = create_test_engine();

        engine.put_meta("version", b"1.0.0").unwrap();
        let value = engine.get_meta("version").unwrap().unwrap();
        assert_eq!(&value, b"1.0.0");
    }

    #[test]
    fn test_property_index_basic() {
        let (engine, _dir) = create_test_engine();
        let id_gen = IdGenerator::new();
        let graph_id = GraphId::from_name("test");

        // Create nodes with properties
        let mut node1 = Node::with_labels(id_gen.next_node_id(), ["Person"]);
        node1.set_property("name", "Alice");
        node1.set_property("age", 30i64);

        let mut node2 = Node::with_labels(id_gen.next_node_id(), ["Person"]);
        node2.set_property("name", "Bob");
        node2.set_property("age", 25i64);

        let mut node3 = Node::with_labels(id_gen.next_node_id(), ["Person"]);
        node3.set_property("name", "Alice"); // Same name as node1
        node3.set_property("age", 35i64);

        engine.put_node(graph_id, &node1).unwrap();
        engine.put_node(graph_id, &node2).unwrap();
        engine.put_node(graph_id, &node3).unwrap();

        // Query by property value
        let alices = engine
            .get_nodes_by_property(graph_id, "Person", "name", &PropertyValue::String("Alice".to_string()))
            .unwrap();
        assert_eq!(alices.len(), 2);

        let bobs = engine
            .get_nodes_by_property(graph_id, "Person", "name", &PropertyValue::String("Bob".to_string()))
            .unwrap();
        assert_eq!(bobs.len(), 1);
        assert_eq!(bobs[0].id, node2.id);
    }

    #[test]
    fn test_property_index_update() {
        let (engine, _dir) = create_test_engine();
        let id_gen = IdGenerator::new();
        let graph_id = GraphId::from_name("test");

        // Create a node
        let mut node = Node::with_labels(id_gen.next_node_id(), ["Person"]);
        node.set_property("name", "Alice");
        engine.put_node(graph_id, &node).unwrap();

        // Verify initial index
        let alices = engine
            .get_nodes_by_property(graph_id, "Person", "name", &PropertyValue::String("Alice".to_string()))
            .unwrap();
        assert_eq!(alices.len(), 1);

        // Update the node's name
        node.set_property("name", "Alicia");
        engine.put_node(graph_id, &node).unwrap();

        // Old value should not be found
        let alices = engine
            .get_nodes_by_property(graph_id, "Person", "name", &PropertyValue::String("Alice".to_string()))
            .unwrap();
        assert_eq!(alices.len(), 0);

        // New value should be found
        let alicias = engine
            .get_nodes_by_property(graph_id, "Person", "name", &PropertyValue::String("Alicia".to_string()))
            .unwrap();
        assert_eq!(alicias.len(), 1);
    }

    #[test]
    fn test_property_index_delete() {
        let (engine, _dir) = create_test_engine();
        let id_gen = IdGenerator::new();
        let graph_id = GraphId::from_name("test");

        // Create a node
        let mut node = Node::with_labels(id_gen.next_node_id(), ["Person"]);
        node.set_property("name", "Alice");
        engine.put_node(graph_id, &node).unwrap();

        // Verify index
        let alices = engine
            .get_nodes_by_property(graph_id, "Person", "name", &PropertyValue::String("Alice".to_string()))
            .unwrap();
        assert_eq!(alices.len(), 1);

        // Delete the node
        engine.delete_node(graph_id, node.id).unwrap();

        // Index should be empty
        let alices = engine
            .get_nodes_by_property(graph_id, "Person", "name", &PropertyValue::String("Alice".to_string()))
            .unwrap();
        assert_eq!(alices.len(), 0);
    }

    #[test]
    fn test_property_index_with_property() {
        let (engine, _dir) = create_test_engine();
        let id_gen = IdGenerator::new();
        let graph_id = GraphId::from_name("test");

        // Create nodes - some with email, some without
        let mut node1 = Node::with_labels(id_gen.next_node_id(), ["Person"]);
        node1.set_property("name", "Alice");
        node1.set_property("email", "alice@example.com");

        let mut node2 = Node::with_labels(id_gen.next_node_id(), ["Person"]);
        node2.set_property("name", "Bob");
        // No email

        let mut node3 = Node::with_labels(id_gen.next_node_id(), ["Person"]);
        node3.set_property("name", "Charlie");
        node3.set_property("email", "charlie@example.com");

        engine.put_node(graph_id, &node1).unwrap();
        engine.put_node(graph_id, &node2).unwrap();
        engine.put_node(graph_id, &node3).unwrap();

        // Query nodes that have email property
        let with_email = engine
            .get_nodes_with_property(graph_id, "Person", "email")
            .unwrap();
        assert_eq!(with_email.len(), 2);

        // Query nodes that have name property (all of them)
        let with_name = engine
            .get_nodes_with_property(graph_id, "Person", "name")
            .unwrap();
        assert_eq!(with_name.len(), 3);
    }

    #[test]
    fn test_property_index_integer_values() {
        let (engine, _dir) = create_test_engine();
        let id_gen = IdGenerator::new();
        let graph_id = GraphId::from_name("test");

        // Create nodes with integer properties
        let mut node1 = Node::with_labels(id_gen.next_node_id(), ["Person"]);
        node1.set_property("age", 30i64);

        let mut node2 = Node::with_labels(id_gen.next_node_id(), ["Person"]);
        node2.set_property("age", 25i64);

        let mut node3 = Node::with_labels(id_gen.next_node_id(), ["Person"]);
        node3.set_property("age", 30i64); // Same age as node1

        engine.put_node(graph_id, &node1).unwrap();
        engine.put_node(graph_id, &node2).unwrap();
        engine.put_node(graph_id, &node3).unwrap();

        // Query by integer value
        let age_30 = engine
            .get_nodes_by_property(graph_id, "Person", "age", &PropertyValue::Integer(30))
            .unwrap();
        assert_eq!(age_30.len(), 2);

        let age_25 = engine
            .get_nodes_by_property(graph_id, "Person", "age", &PropertyValue::Integer(25))
            .unwrap();
        assert_eq!(age_25.len(), 1);
    }

    #[test]
    fn test_property_index_boolean_values() {
        let (engine, _dir) = create_test_engine();
        let id_gen = IdGenerator::new();
        let graph_id = GraphId::from_name("test");

        // Create nodes with boolean properties
        let mut node1 = Node::with_labels(id_gen.next_node_id(), ["Person"]);
        node1.set_property("active", true);

        let mut node2 = Node::with_labels(id_gen.next_node_id(), ["Person"]);
        node2.set_property("active", false);

        let mut node3 = Node::with_labels(id_gen.next_node_id(), ["Person"]);
        node3.set_property("active", true);

        engine.put_node(graph_id, &node1).unwrap();
        engine.put_node(graph_id, &node2).unwrap();
        engine.put_node(graph_id, &node3).unwrap();

        // Query by boolean value
        let active = engine
            .get_nodes_by_property(graph_id, "Person", "active", &PropertyValue::Boolean(true))
            .unwrap();
        assert_eq!(active.len(), 2);

        let inactive = engine
            .get_nodes_by_property(graph_id, "Person", "active", &PropertyValue::Boolean(false))
            .unwrap();
        assert_eq!(inactive.len(), 1);
    }

    #[test]
    fn test_property_index_multiple_labels() {
        let (engine, _dir) = create_test_engine();
        let id_gen = IdGenerator::new();
        let graph_id = GraphId::from_name("test");

        // Create nodes with different labels but same property
        let mut person = Node::with_labels(id_gen.next_node_id(), ["Person"]);
        person.set_property("name", "Alice");

        let mut company = Node::with_labels(id_gen.next_node_id(), ["Company"]);
        company.set_property("name", "Alice Corp");

        engine.put_node(graph_id, &person).unwrap();
        engine.put_node(graph_id, &company).unwrap();

        // Query Person nodes with name
        let person_alice = engine
            .get_nodes_by_property(graph_id, "Person", "name", &PropertyValue::String("Alice".to_string()))
            .unwrap();
        assert_eq!(person_alice.len(), 1);

        // Query Company nodes with name
        let company_alice = engine
            .get_nodes_by_property(graph_id, "Company", "name", &PropertyValue::String("Alice Corp".to_string()))
            .unwrap();
        assert_eq!(company_alice.len(), 1);
    }

    #[test]
    fn test_property_index_range_query() {
        let (engine, _dir) = create_test_engine();
        let id_gen = IdGenerator::new();
        let graph_id = GraphId::from_name("test");

        // Create nodes with integer properties
        let mut node1 = Node::with_labels(id_gen.next_node_id(), ["Person"]);
        node1.set_property("age", 20i64);

        let mut node2 = Node::with_labels(id_gen.next_node_id(), ["Person"]);
        node2.set_property("age", 30i64);

        let mut node3 = Node::with_labels(id_gen.next_node_id(), ["Person"]);
        node3.set_property("age", 40i64);

        let mut node4 = Node::with_labels(id_gen.next_node_id(), ["Person"]);
        node4.set_property("age", 50i64);

        engine.put_node(graph_id, &node1).unwrap();
        engine.put_node(graph_id, &node2).unwrap();
        engine.put_node(graph_id, &node3).unwrap();
        engine.put_node(graph_id, &node4).unwrap();

        // Query range: 25 <= age <= 45
        let min_age = PropertyValue::Integer(25);
        let max_age = PropertyValue::Integer(45);
        let in_range = engine
            .get_nodes_by_property_range(graph_id, "Person", "age", Some(&min_age), Some(&max_age))
            .unwrap();
        assert_eq!(in_range.len(), 2); // ages 30 and 40

        // Query range: age >= 35
        let min_age = PropertyValue::Integer(35);
        let at_least_35 = engine
            .get_nodes_by_property_range(graph_id, "Person", "age", Some(&min_age), None)
            .unwrap();
        assert_eq!(at_least_35.len(), 2); // ages 40 and 50

        // Query range: age <= 35
        let max_age = PropertyValue::Integer(35);
        let at_most_35 = engine
            .get_nodes_by_property_range(graph_id, "Person", "age", None, Some(&max_age))
            .unwrap();
        assert_eq!(at_most_35.len(), 2); // ages 20 and 30
    }
}
