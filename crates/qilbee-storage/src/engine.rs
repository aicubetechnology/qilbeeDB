//! Storage engine implementation using RocksDB

use crate::keys::KeyBuilder;
use crate::options::StorageOptions;
use crate::transaction::TransactionOperation;
use qilbee_core::{
    EntityId, Error, GraphId, Node, NodeId, PropertyValue, Relationship, RelationshipId, Result,
};
use rocksdb::{ColumnFamily, ColumnFamilyDescriptor, DB, Options, WriteBatch};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tracing::{debug, info, warn};

#[path = "graph_lifecycle.rs"]
mod graph_lifecycle;
pub use graph_lifecycle::GraphIdentity;

fn ordered_meta_key(key: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(key.len() + 1);
    bytes.push(0xf0);
    bytes.extend_from_slice(key.as_bytes());
    bytes
}

#[path = "property_index.rs"]
mod property_index;
use property_index::hash_property_value;

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
    mutation_lock: Arc<Mutex<()>>,
}

impl StorageEngine {
    /// Directory containing this engine's durable state.
    pub fn path(&self) -> &std::path::Path {
        &self.options.path
    }

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
                    block_opts.set_bloom_filter(options.bloom_filter_bits_per_key as f64, false);
                    cf_opts.set_block_based_table_factory(&block_opts);
                }
                ColumnFamilyDescriptor::new(*name, cf_opts)
            })
            .collect();

        let db = DB::open_cf_descriptors(&db_opts, &options.path, cf_descriptors)
            .map_err(|e| Error::Storage(e.to_string()))?;

        let engine = Self {
            db: Arc::new(db),
            options,
            mutation_lock: Arc::new(Mutex::new(())),
        };
        engine.ensure_property_index()?;
        info!("Storage engine opened successfully");
        Ok(engine)
    }

    /// Open a stopped store read-only for offline verification.
    ///
    /// Refuses stores held open by another process and stores whose column
    /// families differ from this binary's. Nothing is created or repaired;
    /// the property index bootstrap performed by `open` is skipped.
    pub fn open_read_only(path: &std::path::Path) -> Result<Self> {
        let mut families = vec!["default"];
        families.extend_from_slice(COLUMN_FAMILIES);
        let db = crate::verification::open_read_only(path, &families)?;
        let mut options = StorageOptions::new(path);
        options.create_if_missing = false;
        Ok(Self {
            db: Arc::new(db),
            options,
            mutation_lock: Arc::new(Mutex::new(())),
        })
    }

    /// Physical inventory of every column family, including `default`.
    pub fn inventory(&self) -> Result<Vec<crate::verification::FamilyInventory>> {
        std::iter::once("default")
            .chain(COLUMN_FAMILIES.iter().copied())
            .map(|family| crate::verification::family_inventory(&self.db, family, |_| false))
            .collect()
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
        self.apply_operations(graph_id, &[TransactionOperation::PutNode(node.clone())])
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
        let _guard = self
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Storage mutation lock poisoned".into()))?;
        let exists = self.get_node(graph_id, node_id)?.is_some();
        self.apply_operations_locked(graph_id, &[TransactionOperation::DeleteNode(node_id)])?;
        Ok(exists)
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
            let node: Node =
                bincode::deserialize(&value).map_err(|e| Error::Deserialization(e.to_string()))?;
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
                    let above_min =
                        min_value.map_or(true, |min| compare_property_values(value, min) >= 0);
                    let below_max =
                        max_value.map_or(true, |max| compare_property_values(value, max) <= 0);
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
        self.apply_operations(
            graph_id,
            &[TransactionOperation::PutRelationship(rel.clone())],
        )
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
    pub fn delete_relationship(&self, graph_id: GraphId, rel_id: RelationshipId) -> Result<bool> {
        let _guard = self
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Storage mutation lock poisoned".into()))?;
        let exists = self.get_relationship(graph_id, rel_id)?.is_some();
        self.apply_operations_locked(
            graph_id,
            &[TransactionOperation::DeleteRelationship(rel_id)],
        )?;
        Ok(exists)
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

    /// Atomically compare metadata values and apply a guarded batch.
    ///
    /// Every write must have one condition, and keys must be unique in each list.
    /// Read-only conditions may guard authority or related records. A failed
    /// comparison returns false without writing anything. Successful mutations
    /// always use WAL and synchronous writes, independent of bulk graph settings.
    /// This serializes writers sharing this engine; it is not a read snapshot.
    pub fn compare_and_write_meta(
        &self,
        expected: &[crate::MetadataCondition],
        writes: &[crate::MetadataWrite],
    ) -> Result<bool> {
        let mut guards = std::collections::HashSet::new();
        for condition in expected {
            if !guards.insert(condition.key.as_str()) {
                return Err(Error::ValidationError(
                    "Duplicate metadata condition".into(),
                ));
            }
        }
        let mut keys = std::collections::HashSet::new();
        for write in writes {
            if !guards.contains(write.key.as_str()) || !keys.insert(write.key.as_str()) {
                return Err(Error::ValidationError(
                    "Metadata writes require unique guarded keys".into(),
                ));
            }
        }
        let _guard = self
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Storage mutation lock poisoned".into()))?;
        for condition in expected {
            if self.get_meta(&condition.key)? != condition.expected {
                return Ok(false);
            }
        }
        let cf = self.cf(cf::META)?;
        let mut batch = WriteBatch::default();
        for write in writes {
            let key = KeyBuilder::meta(&write.key);
            match &write.value {
                Some(value) => {
                    batch.put_cf(cf, key, value);
                    batch.put_cf(cf, ordered_meta_key(&write.key), []);
                }
                None => {
                    batch.delete_cf(cf, key);
                    batch.delete_cf(cf, ordered_meta_key(&write.key));
                }
            }
        }
        let mut options = rocksdb::WriteOptions::default();
        options.disable_wal(false);
        options.set_sync(true);
        self.db
            .write_opt(batch, &options)
            .map_err(|e| Error::Storage(e.to_string()))?;
        Ok(true)
    }

    /// Rebuild ordered logical-key markers before serving traffic. Legacy metadata
    /// uses a length-prefixed physical key, which cannot support lexical prefix seeks.
    /// Repeating this repair also handles metadata written by an older binary.
    pub fn ensure_ordered_metadata_keys(&self) -> Result<()> {
        let _guard = self
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Storage mutation lock poisoned".into()))?;
        let cf = self.cf(cf::META)?;
        // Remove stale markers too, including deletions performed by an older binary.
        let mut old = self.db.raw_iterator_cf(cf);
        old.seek([0xf0]);
        let mut cleanup = WriteBatch::default();
        let mut pending = 0;
        while let Some(key) = old.key() {
            if key.first() != Some(&0xf0) {
                break;
            }
            cleanup.delete_cf(cf, key);
            pending += 1;
            if pending == 1000 {
                self.db
                    .write(cleanup)
                    .map_err(|e| Error::Storage(e.to_string()))?;
                cleanup = WriteBatch::default();
                pending = 0;
            }
            old.next();
        }
        old.status().map_err(|e| Error::Storage(e.to_string()))?;
        self.db
            .write(cleanup)
            .map_err(|e| Error::Storage(e.to_string()))?;
        let mut iter = self.db.raw_iterator_cf(cf);
        iter.seek([crate::keys::prefix::META]);
        let mut batch = WriteBatch::default();
        let mut count = 0;
        while let Some(key) = iter.key() {
            if key.first() != Some(&crate::keys::prefix::META) {
                break;
            }
            if key.len() >= 3 && u16::from_be_bytes([key[1], key[2]]) as usize == key.len() - 3 {
                if let Ok(logical) = std::str::from_utf8(&key[3..]) {
                    batch.put_cf(cf, ordered_meta_key(logical), []);
                    count += 1;
                    if count == 1000 {
                        self.db
                            .write(batch)
                            .map_err(|e| Error::Storage(e.to_string()))?;
                        batch = WriteBatch::default();
                        count = 0;
                    }
                }
            }
            iter.next();
        }
        iter.status().map_err(|e| Error::Storage(e.to_string()))?;
        self.db
            .write(batch)
            .map_err(|e| Error::Storage(e.to_string()))?;
        Ok(())
    }

    /// Live, forward-only pagination inside one exact server-selected logical prefix.
    /// The cursor is the last returned key; pages do not form a database snapshot.
    pub fn scan_meta_keys(
        &self,
        prefix: &str,
        after: Option<&str>,
        limit: usize,
    ) -> Result<(Vec<String>, bool)> {
        if prefix.is_empty()
            || prefix.len() > 4096
            || !(1..=100).contains(&limit)
            || after.is_some_and(|key| !key.starts_with(prefix) || key.len() > 4096)
        {
            return Err(Error::ValidationError(
                "Invalid metadata page boundary".into(),
            ));
        }
        let physical_prefix = ordered_meta_key(prefix);
        let start = ordered_meta_key(after.unwrap_or(prefix));
        let cf = self.cf(cf::META)?;
        let mut iter = self.db.raw_iterator_cf(cf);
        iter.seek(&start);
        let mut keys = Vec::new();
        let mut more = false;
        while let Some(key) = iter.key() {
            if !key.starts_with(&physical_prefix) {
                break;
            }
            if after.is_none() || key > start.as_slice() {
                if keys.len() == limit {
                    more = true;
                    break;
                }
                let logical = std::str::from_utf8(&key[1..])
                    .map_err(|_| Error::DataCorruption("Invalid metadata directory key".into()))?;
                keys.push(logical.to_owned());
            }
            iter.next();
        }
        iter.status().map_err(|e| Error::Storage(e.to_string()))?;
        Ok((keys, more))
    }

    /// Store metadata
    pub fn put_meta(&self, key: &str, value: &[u8]) -> Result<()> {
        let _guard = self
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Storage mutation lock poisoned".into()))?;
        let storage_key = KeyBuilder::meta(key);
        let cf = self.cf(cf::META)?;

        let mut batch = WriteBatch::default();
        batch.put_cf(cf, storage_key, value);
        batch.put_cf(cf, ordered_meta_key(key), []);
        self.db
            .write(batch)
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

    pub(crate) fn transaction_node_bytes(
        &self,
        graph: GraphId,
        id: NodeId,
    ) -> Result<Option<Vec<u8>>> {
        self.db
            .get_cf(self.cf(cf::NODES)?, KeyBuilder::node(graph, id))
            .map_err(|e| Error::Storage(e.to_string()))
    }

    pub(crate) fn transaction_relationship_bytes(
        &self,
        graph: GraphId,
        id: RelationshipId,
    ) -> Result<Option<Vec<u8>>> {
        self.db
            .get_cf(
                self.cf(cf::RELATIONSHIPS)?,
                KeyBuilder::relationship(graph, id),
            )
            .map_err(|e| Error::Storage(e.to_string()))
    }

    /// Validate point observations and publish the complete batch under one lock.
    pub(crate) fn apply_observed_transaction(
        &self,
        graph: GraphId,
        identity: Option<&GraphIdentity>,
        operations: &[TransactionOperation],
        nodes: &HashMap<NodeId, Option<Vec<u8>>>,
        relationships: &HashMap<RelationshipId, Option<Vec<u8>>>,
    ) -> Result<()> {
        let _guard = self
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Storage mutation lock poisoned".into()))?;
        if let Some(identity) = identity {
            self.validate_graph_identity(identity)?;
            if identity.id() != graph {
                return Err(Error::InvalidGraphOperation(
                    "Transaction graph identity mismatch".into(),
                ));
            }
        }
        for (id, observed) in nodes {
            if self.transaction_node_bytes(graph, *id)? != *observed {
                return Err(Error::TransactionAborted(
                    "A node observed by the transaction changed".into(),
                ));
            }
        }
        for (id, observed) in relationships {
            if self.transaction_relationship_bytes(graph, *id)? != *observed {
                return Err(Error::TransactionAborted(
                    "A relationship observed by the transaction changed".into(),
                ));
            }
        }
        self.apply_operations_locked(graph, operations)
    }

    pub(crate) fn apply_operations(
        &self,
        graph_id: GraphId,
        operations: &[TransactionOperation],
    ) -> Result<()> {
        let _guard = self
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Storage mutation lock poisoned".into()))?;
        self.apply_operations_locked(graph_id, operations)
    }

    /// Build the complete final write set before publishing any entity or index.
    /// The caller holds the shared writer lock across reading and committing.
    fn apply_operations_locked(
        &self,
        graph_id: GraphId,
        operations: &[TransactionOperation],
    ) -> Result<()> {
        let mut nodes = HashMap::<NodeId, Option<Node>>::new();
        let mut relationships = HashMap::<RelationshipId, Option<Relationship>>::new();
        for operation in operations {
            match operation {
                TransactionOperation::PutNode(node) => {
                    nodes.insert(node.id, Some(node.clone()));
                }
                TransactionOperation::DeleteNode(id) => {
                    nodes.insert(*id, None);
                }
                TransactionOperation::PutRelationship(rel) => {
                    relationships.insert(rel.id, Some(rel.clone()));
                }
                TransactionOperation::DeleteRelationship(id) => {
                    relationships.insert(*id, None);
                }
            }
        }

        let mut batch = WriteBatch::default();
        for (id, final_node) in nodes {
            if let Some(previous) = self.get_node(graph_id, id)? {
                self.stage_node(&mut batch, graph_id, &previous, false)?;
            }
            if let Some(node) = final_node {
                self.stage_node(&mut batch, graph_id, &node, true)?;
            }
        }
        for (id, final_relationship) in relationships {
            if let Some(previous) = self.get_relationship(graph_id, id)? {
                self.stage_relationship(&mut batch, graph_id, &previous, false)?;
            }
            if let Some(rel) = final_relationship {
                self.stage_relationship(&mut batch, graph_id, &rel, true)?;
            }
        }
        let mut options = rocksdb::WriteOptions::default();
        let managed = self.stage_graph_allocator(&mut batch, graph_id, operations)?;
        options.disable_wal(!managed && !self.options.enable_wal);
        options.set_sync(managed || (self.options.enable_wal && self.options.sync_wal));
        self.db
            .write_opt(batch, &options)
            .map_err(|e| Error::Storage(e.to_string()))
    }

    fn stage_node(
        &self,
        batch: &mut WriteBatch,
        graph_id: GraphId,
        node: &Node,
        put: bool,
    ) -> Result<()> {
        let key = KeyBuilder::node(graph_id, node.id);
        let node_cf = self.cf(cf::NODES)?;
        if put {
            batch.put_cf(
                node_cf,
                key,
                bincode::serialize(node).map_err(|e| Error::Serialization(e.to_string()))?,
            );
        } else {
            batch.delete_cf(node_cf, key);
        }
        let label_cf = self.cf(cf::LABEL_INDEX)?;
        let property_cf = self.cf(cf::PROPERTY_INDEX)?;
        for label in &node.labels {
            let key = KeyBuilder::label_index(graph_id, label.name(), node.id);
            if put {
                batch.put_cf(label_cf, key, []);
            } else {
                batch.delete_cf(label_cf, key);
            }
            for (name, value) in node.properties.iter() {
                let key = KeyBuilder::property_index(
                    graph_id,
                    label.name(),
                    name,
                    hash_property_value(value),
                    node.id.as_internal(),
                );
                if put {
                    batch.put_cf(
                        property_cf,
                        key,
                        bincode::serialize(value)
                            .map_err(|e| Error::Serialization(e.to_string()))?,
                    );
                } else {
                    batch.delete_cf(property_cf, key);
                }
            }
        }
        Ok(())
    }

    fn stage_relationship(
        &self,
        batch: &mut WriteBatch,
        graph_id: GraphId,
        rel: &Relationship,
        put: bool,
    ) -> Result<()> {
        let key = KeyBuilder::relationship(graph_id, rel.id);
        let relationship_cf = self.cf(cf::RELATIONSHIPS)?;
        let outgoing_cf = self.cf(cf::ADJACENCY_OUT)?;
        let incoming_cf = self.cf(cf::ADJACENCY_IN)?;
        let outgoing = KeyBuilder::adjacency_out(graph_id, rel.source, rel.rel_type.name(), rel.id);
        let incoming = KeyBuilder::adjacency_in(graph_id, rel.target, rel.rel_type.name(), rel.id);
        if put {
            batch.put_cf(
                relationship_cf,
                key,
                bincode::serialize(rel).map_err(|e| Error::Serialization(e.to_string()))?,
            );
            batch.put_cf(
                outgoing_cf,
                outgoing,
                rel.target.as_internal().to_be_bytes(),
            );
            batch.put_cf(
                incoming_cf,
                incoming,
                rel.source.as_internal().to_be_bytes(),
            );
        } else {
            batch.delete_cf(relationship_cf, key);
            batch.delete_cf(outgoing_cf, outgoing);
            batch.delete_cf(incoming_cf, incoming);
        }
        Ok(())
    }

    /// Begin a new transaction for a graph
    pub fn begin_transaction(&self, graph_id: GraphId) -> Result<crate::transaction::Transaction> {
        Ok(crate::transaction::Transaction::new(self.clone(), graph_id))
    }

    // ========== Utility Operations ==========

    /// Flush all in-memory data to disk
    pub fn flush(&self) -> Result<()> {
        self.db.flush().map_err(|e| Error::Storage(e.to_string()))?;
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
            mutation_lock: Arc::clone(&self.mutation_lock),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qilbee_core::{EntityId, IdGenerator, Property};
    use tempfile::TempDir;

    #[test]
    fn atomic_commit_failure_does_not_publish_earlier_operations() {
        let (engine, _dir) = create_test_engine();
        let ids = IdGenerator::new();
        let graph = GraphId::from_name("atomic");
        let new_node = Node::with_labels(ids.next_node_id(), ["New"]);
        let corrupt_id = ids.next_node_id();
        engine
            .db
            .put_cf(
                engine.cf(cf::NODES).unwrap(),
                KeyBuilder::node(graph, corrupt_id),
                b"corrupt",
            )
            .unwrap();
        let mut tx = engine.begin_transaction(graph).unwrap();
        tx.put_node(new_node.clone()).unwrap();
        tx.delete_node(corrupt_id).unwrap();
        assert!(tx.commit().is_err());
        assert!(engine.get_node(graph, new_node.id).unwrap().is_none());
        assert!(engine.get_nodes_by_label(graph, "New").unwrap().is_empty());
    }

    #[test]
    fn atomic_repeated_node_operations_leave_only_final_indexes() {
        let (engine, _dir) = create_test_engine();
        let ids = IdGenerator::new();
        let graph = GraphId::from_name("atomic");
        let mut original = Node::with_labels(ids.next_node_id(), ["Old"]);
        original.set_property("name", "before");
        engine.put_node(graph, &original).unwrap();
        let middle = Node::with_labels(original.id, ["Middle"]);
        let mut final_node = Node::with_labels(original.id, ["Final"]);
        final_node.set_property("name", "after");
        let mut tx = engine.begin_transaction(graph).unwrap();
        tx.put_node(middle).unwrap();
        tx.delete_node(original.id).unwrap();
        tx.put_node(final_node).unwrap();
        tx.commit().unwrap();
        assert!(engine.get_nodes_by_label(graph, "Old").unwrap().is_empty());
        assert!(
            engine
                .get_nodes_by_label(graph, "Middle")
                .unwrap()
                .is_empty()
        );
        assert_eq!(engine.get_nodes_by_label(graph, "Final").unwrap().len(), 1);
        let old_key = KeyBuilder::property_index(
            graph,
            "Old",
            "name",
            hash_property_value(&PropertyValue::String("before".into())),
            original.id.as_internal(),
        );
        assert!(
            engine
                .db
                .get_cf(engine.cf(cf::PROPERTY_INDEX).unwrap(), old_key)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn atomic_direct_node_replacement_removes_old_labels_and_properties() {
        let (engine, _dir) = create_test_engine();
        let graph = GraphId::from_name("atomic");
        let ids = IdGenerator::new();
        let mut old = Node::with_labels(ids.next_node_id(), ["Old"]);
        old.set_property("name", "old");
        engine.put_node(graph, &old).unwrap();
        engine
            .put_node(graph, &Node::with_labels(old.id, ["New"]))
            .unwrap();
        assert!(engine.get_nodes_by_label(graph, "Old").unwrap().is_empty());
        assert_eq!(engine.get_nodes_by_label(graph, "New").unwrap().len(), 1);
        let old_key = KeyBuilder::property_index(
            graph,
            "Old",
            "name",
            hash_property_value(&PropertyValue::String("old".into())),
            old.id.as_internal(),
        );
        assert!(
            engine
                .db
                .get_cf(engine.cf(cf::PROPERTY_INDEX).unwrap(), old_key)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn atomic_relationship_replacement_removes_both_old_adjacencies() {
        let (engine, _dir) = create_test_engine();
        let graph = GraphId::from_name("atomic");
        let ids = IdGenerator::new();
        let a = ids.next_node_id();
        let b = ids.next_node_id();
        let c = ids.next_node_id();
        let old = Relationship::new(ids.next_relationship_id(), "OLD", a, b);
        engine.put_relationship(graph, &old).unwrap();
        let new = Relationship::new(old.id, "NEW", c, a);
        let mut tx = engine.begin_transaction(graph).unwrap();
        tx.put_relationship(new).unwrap();
        tx.commit().unwrap();
        assert!(
            engine
                .get_outgoing_relationships(graph, a)
                .unwrap()
                .is_empty()
        );
        assert!(
            engine
                .get_incoming_relationships(graph, b)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            engine.get_outgoing_relationships(graph, c).unwrap().len(),
            1
        );
        assert_eq!(
            engine.get_incoming_relationships(graph, a).unwrap().len(),
            1
        );
    }

    #[test]
    fn atomic_concurrent_cloned_writers_leave_only_current_label() {
        let (engine, _dir) = create_test_engine();
        let graph = GraphId::from_name("atomic");
        let id = IdGenerator::new().next_node_id();
        engine
            .put_node(graph, &Node::with_labels(id, ["initial"]))
            .unwrap();
        let workers: Vec<_> = (0..8)
            .map(|index| {
                let engine = engine.clone();
                std::thread::spawn(move || {
                    for _ in 0..8 {
                        engine
                            .put_node(graph, &Node::with_labels(id, [format!("label-{index}")]))
                            .unwrap();
                    }
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
        assert!(
            engine
                .get_nodes_by_label(graph, "initial")
                .unwrap()
                .is_empty()
        );
        let indexed: usize = (0..8)
            .map(|index| {
                engine
                    .get_nodes_by_label(graph, &format!("label-{index}"))
                    .unwrap()
                    .len()
            })
            .sum();
        assert_eq!(indexed, 1);
    }

    #[test]
    fn atomic_snapshots_never_observe_a_partial_two_node_commit() {
        let (engine, _dir) = create_test_engine();
        let graph = GraphId::from_name("atomic");
        let ids = IdGenerator::new();
        let a = ids.next_node_id();
        let b = ids.next_node_id();
        let node = |id, value: i64| {
            let mut node = Node::with_labels(id, ["Pair"]);
            node.set_property("generation", value);
            node
        };
        engine.put_node(graph, &node(a, 0)).unwrap();
        engine.put_node(graph, &node(b, 0)).unwrap();
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let writer = engine.clone();
        let start = barrier.clone();
        let worker = std::thread::spawn(move || {
            start.wait();
            for generation in 1..100 {
                let mut tx = writer.begin_transaction(graph).unwrap();
                tx.put_node(node(a, generation)).unwrap();
                tx.put_node(node(b, generation)).unwrap();
                tx.commit().unwrap();
            }
        });
        barrier.wait();
        for _ in 0..1024 {
            let snapshot = engine.db.snapshot();
            let read = |id| -> Node {
                let bytes = snapshot
                    .get_cf(engine.cf(cf::NODES).unwrap(), KeyBuilder::node(graph, id))
                    .unwrap()
                    .unwrap();
                bincode::deserialize(&bytes).unwrap()
            };
            assert_eq!(
                read(a).get_property("generation"),
                read(b).get_property("generation")
            );
        }
        worker.join().unwrap();
    }

    #[test]
    fn atomic_committed_entities_and_indexes_survive_reopen() {
        let dir = TempDir::new().unwrap();
        let options = StorageOptions::for_testing(dir.path()).sync_wal(true);
        let ids = IdGenerator::new();
        let graph = GraphId::from_name("atomic");
        let a = Node::with_labels(ids.next_node_id(), ["Source"]);
        let b = Node::with_labels(ids.next_node_id(), ["Target"]);
        let rel = Relationship::new(ids.next_relationship_id(), "LINK", a.id, b.id);
        {
            let engine = StorageEngine::open(options.clone()).unwrap();
            let mut tx = engine.begin_transaction(graph).unwrap();
            tx.put_node(a.clone()).unwrap();
            tx.put_node(b.clone()).unwrap();
            tx.put_relationship(rel.clone()).unwrap();
            tx.commit().unwrap();
        }
        let engine = StorageEngine::open(options).unwrap();
        assert_eq!(
            engine.get_nodes_by_label(graph, "Source").unwrap()[0].id,
            a.id
        );
        assert_eq!(
            engine.get_nodes_by_label(graph, "Target").unwrap()[0].id,
            b.id
        );
        assert_eq!(
            engine.get_outgoing_relationships(graph, a.id).unwrap()[0].id,
            rel.id
        );
        assert_eq!(
            engine.get_incoming_relationships(graph, b.id).unwrap()[0].id,
            rel.id
        );
    }

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
        let outgoing = engine
            .get_outgoing_relationships(graph_id, node1.id)
            .unwrap();
        assert_eq!(outgoing.len(), 2);

        // Query incoming to node2
        let incoming = engine
            .get_incoming_relationships(graph_id, node2.id)
            .unwrap();
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
    fn property_index_equal_signed_zero_must_share_lookup_identity() {
        let (engine, _dir) = create_test_engine();
        let graph = GraphId::from_name("zero-index");
        let mut node = Node::with_labels(IdGenerator::new().next_node_id(), ["Evidence"]);
        node.set_property("score", PropertyValue::Float(-0.0));
        engine.put_node(graph, &node).unwrap();
        assert_eq!(PropertyValue::Float(-0.0), PropertyValue::Float(0.0));
        let found = engine
            .get_nodes_by_property(graph, "Evidence", "score", &PropertyValue::Float(0.0))
            .unwrap();
        assert_eq!(
            found.len(),
            1,
            "equal signed zero values must retrieve the stored node"
        );
    }

    #[test]
    fn property_index_equal_maps_must_share_lookup_identity() {
        let (engine, _dir) = create_test_engine();
        let graph = GraphId::from_name("map-index");
        let entries = (0..16)
            .map(|i| (format!("field-{i}"), PropertyValue::Integer(i)))
            .collect::<Vec<_>>();
        let original = PropertyValue::Map(entries.iter().cloned().collect());
        let mut node = Node::with_labels(IdGenerator::new().next_node_id(), ["Evidence"]);
        node.set_property("attributes", original.clone());
        engine.put_node(graph, &node).unwrap();
        for _ in 0..16 {
            let equivalent = PropertyValue::Map(entries.iter().rev().cloned().collect());
            assert_eq!(equivalent, original);
            let found = engine
                .get_nodes_by_property(graph, "Evidence", "attributes", &equivalent)
                .unwrap();
            assert_eq!(
                found.len(),
                1,
                "equal map values must retrieve the stored node regardless of iteration order"
            );
        }
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
            .get_nodes_by_property(
                graph_id,
                "Person",
                "name",
                &PropertyValue::String("Alice".to_string()),
            )
            .unwrap();
        assert_eq!(alices.len(), 2);

        let bobs = engine
            .get_nodes_by_property(
                graph_id,
                "Person",
                "name",
                &PropertyValue::String("Bob".to_string()),
            )
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
            .get_nodes_by_property(
                graph_id,
                "Person",
                "name",
                &PropertyValue::String("Alice".to_string()),
            )
            .unwrap();
        assert_eq!(alices.len(), 1);

        // Update the node's name
        node.set_property("name", "Alicia");
        engine.put_node(graph_id, &node).unwrap();

        // Old value should not be found
        let alices = engine
            .get_nodes_by_property(
                graph_id,
                "Person",
                "name",
                &PropertyValue::String("Alice".to_string()),
            )
            .unwrap();
        assert_eq!(alices.len(), 0);

        // New value should be found
        let alicias = engine
            .get_nodes_by_property(
                graph_id,
                "Person",
                "name",
                &PropertyValue::String("Alicia".to_string()),
            )
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
            .get_nodes_by_property(
                graph_id,
                "Person",
                "name",
                &PropertyValue::String("Alice".to_string()),
            )
            .unwrap();
        assert_eq!(alices.len(), 1);

        // Delete the node
        engine.delete_node(graph_id, node.id).unwrap();

        // Index should be empty
        let alices = engine
            .get_nodes_by_property(
                graph_id,
                "Person",
                "name",
                &PropertyValue::String("Alice".to_string()),
            )
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
            .get_nodes_by_property(
                graph_id,
                "Person",
                "name",
                &PropertyValue::String("Alice".to_string()),
            )
            .unwrap();
        assert_eq!(person_alice.len(), 1);

        // Query Company nodes with name
        let company_alice = engine
            .get_nodes_by_property(
                graph_id,
                "Company",
                "name",
                &PropertyValue::String("Alice Corp".to_string()),
            )
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

#[cfg(test)]
mod ordered_metadata_tests {
    use super::*;
    #[test]
    fn legacy_length_prefixed_keys_rebuild_and_page_by_exact_logical_prefix() {
        let dir = tempfile::TempDir::new().unwrap();
        let db = StorageEngine::open(StorageOptions::for_testing(dir.path())).unwrap();
        let cf = db.cf(cf::META).unwrap();
        for key in [
            "identity/a/z",
            "identity/a/longer-name",
            "identity/a/a",
            "identity/ab/secret",
        ] {
            // Emulate an older binary, which wrote only the original physical key.
            db.db.put_cf(cf, KeyBuilder::meta(key), b"legacy").unwrap();
        }
        assert!(
            db.scan_meta_keys("identity/a/", None, 2)
                .unwrap()
                .0
                .is_empty()
        );
        db.ensure_ordered_metadata_keys().unwrap();
        let (first, more) = db.scan_meta_keys("identity/a/", None, 2).unwrap();
        assert_eq!(first, ["identity/a/a", "identity/a/longer-name"]);
        assert!(more);
        let (last, more) = db
            .scan_meta_keys("identity/a/", first.last().map(String::as_str), 2)
            .unwrap();
        assert_eq!(last, ["identity/a/z"]);
        assert!(!more);
        assert!(
            db.scan_meta_keys("identity/a/", Some("identity/ab/secret"), 2)
                .is_err()
        );
        let key = "identity/a/new";
        db.compare_and_write_meta(
            &[crate::MetadataCondition {
                key: key.into(),
                expected: None,
            }],
            &[crate::MetadataWrite {
                key: key.into(),
                value: Some(b"new".to_vec()),
            }],
        )
        .unwrap();
        assert!(
            db.scan_meta_keys("identity/a/", None, 100)
                .unwrap()
                .0
                .contains(&key.to_owned())
        );
        db.compare_and_write_meta(
            &[crate::MetadataCondition {
                key: key.into(),
                expected: Some(b"new".to_vec()),
            }],
            &[crate::MetadataWrite {
                key: key.into(),
                value: None,
            }],
        )
        .unwrap();
        assert!(
            !db.scan_meta_keys("identity/a/", None, 100)
                .unwrap()
                .0
                .contains(&key.to_owned())
        );
        db.ensure_ordered_metadata_keys().unwrap();
        assert_eq!(
            db.scan_meta_keys("identity/a/", None, 100).unwrap().0.len(),
            3
        );
        db.db
            .delete_cf(cf, KeyBuilder::meta("identity/a/a"))
            .unwrap();
        db.ensure_ordered_metadata_keys().unwrap();
        assert_eq!(
            db.scan_meta_keys("identity/a/", None, 100).unwrap().0.len(),
            2
        );
    }
}
