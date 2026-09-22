//! Transaction support for QilbeeDB storage

use crate::engine::StorageEngine;
use qilbee_core::{Error, GraphId, Node, NodeId, Relationship, RelationshipId, Result};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

/// Global transaction ID counter
static TRANSACTION_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Transaction state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionState {
    /// Transaction is active and accepting operations
    Active,
    /// Transaction has been committed
    Committed,
    /// Transaction has been rolled back
    RolledBack,
}

/// A pending operation in the transaction
#[derive(Debug, Clone)]
pub enum TransactionOperation {
    /// Create or update a node
    PutNode(Node),
    /// Delete a node
    DeleteNode(NodeId),
    /// Create or update a relationship
    PutRelationship(Relationship),
    /// Delete a relationship
    DeleteRelationship(RelationshipId),
}

/// A transaction for atomic graph operations
///
/// Commits entity and index mutations in one atomic RocksDB batch.
/// Original entity bytes, including absence, are validated under the writer
/// lock before commit. Conflicts abort without publishing any mutations.
/// This is point-read validation, not a historical snapshot or range locking.
pub struct Transaction {
    /// Transaction ID
    id: u64,

    /// Graph this transaction operates on
    graph_id: GraphId,

    graph_identity: Option<crate::GraphIdentity>,

    /// Reference to the storage engine
    engine: StorageEngine,

    /// Current state
    state: TransactionState,

    /// Pending operations
    operations: Vec<TransactionOperation>,

    /// Node read cache for this transaction
    node_cache: HashMap<NodeId, Option<Node>>,

    /// Relationship read cache for this transaction
    rel_cache: HashMap<RelationshipId, Option<Relationship>>,

    node_observations: HashMap<NodeId, Option<Vec<u8>>>,
    rel_observations: HashMap<RelationshipId, Option<Vec<u8>>>,
}

impl Transaction {
    /// Create a new transaction
    pub fn new(engine: StorageEngine, graph_id: GraphId) -> Self {
        Self {
            id: TRANSACTION_COUNTER.fetch_add(1, Ordering::SeqCst),
            graph_id,
            graph_identity: None,
            engine,
            state: TransactionState::Active,
            operations: Vec::new(),
            node_cache: HashMap::new(),
            rel_cache: HashMap::new(),
            node_observations: HashMap::new(),
            rel_observations: HashMap::new(),
        }
    }

    /// Bind every transaction operation and commit to one active named generation.
    pub fn for_graph(engine: StorageEngine, identity: crate::GraphIdentity) -> Self {
        let mut transaction = Self::new(engine, identity.id());
        transaction.graph_identity = Some(identity);
        transaction
    }

    /// Get the transaction ID
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Get the transaction state
    pub fn state(&self) -> TransactionState {
        self.state
    }

    /// Check if transaction is active
    pub fn is_active(&self) -> bool {
        self.state == TransactionState::Active
    }

    fn check_active(&self) -> Result<()> {
        if let Some(identity) = &self.graph_identity {
            self.engine.validate_graph_identity(identity)?;
        }
        if !self.is_active() {
            return Err(Error::TransactionAborted(
                "Transaction is no longer active".to_string(),
            ));
        }
        Ok(())
    }

    fn observe_node_for_write(&mut self, id: NodeId) -> Result<()> {
        if !self.node_observations.contains_key(&id) {
            let bytes = self.engine.transaction_node_bytes(self.graph_id, id)?;
            self.node_observations.insert(id, bytes);
        }
        Ok(())
    }

    fn observe_relationship_for_write(&mut self, id: RelationshipId) -> Result<()> {
        if !self.rel_observations.contains_key(&id) {
            let bytes = self
                .engine
                .transaction_relationship_bytes(self.graph_id, id)?;
            self.rel_observations.insert(id, bytes);
        }
        Ok(())
    }

    // ========== Node Operations ==========

    /// Create or update a node
    pub fn put_node(&mut self, node: Node) -> Result<()> {
        self.check_active()?;

        self.observe_node_for_write(node.id)?;
        // Update cache
        self.node_cache.insert(node.id, Some(node.clone()));

        // Add to pending operations
        self.operations.push(TransactionOperation::PutNode(node));

        Ok(())
    }

    /// Get a node by ID
    ///
    /// Returns the node from the transaction's view, which includes
    /// any pending changes.
    pub fn get_node(&mut self, node_id: NodeId) -> Result<Option<Node>> {
        self.check_active()?;

        // Check cache first (includes pending changes)
        if let Some(cached) = self.node_cache.get(&node_id) {
            return Ok(cached.clone());
        }

        let bytes = self.engine.transaction_node_bytes(self.graph_id, node_id)?;
        let node: Option<Node> = bytes
            .as_deref()
            .map(bincode::deserialize)
            .transpose()
            .map_err(|e| Error::Deserialization(e.to_string()))?;
        self.node_observations.insert(node_id, bytes);

        // Cache the result
        self.node_cache.insert(node_id, node.clone());

        Ok(node)
    }

    /// Delete a node
    pub fn delete_node(&mut self, node_id: NodeId) -> Result<()> {
        self.check_active()?;

        self.observe_node_for_write(node_id)?;
        // Mark as deleted in cache
        self.node_cache.insert(node_id, None);

        // Add to pending operations
        self.operations
            .push(TransactionOperation::DeleteNode(node_id));

        Ok(())
    }

    // ========== Relationship Operations ==========

    /// Create or update a relationship
    pub fn put_relationship(&mut self, rel: Relationship) -> Result<()> {
        self.check_active()?;

        self.observe_relationship_for_write(rel.id)?;
        // Update cache
        self.rel_cache.insert(rel.id, Some(rel.clone()));

        // Add to pending operations
        self.operations
            .push(TransactionOperation::PutRelationship(rel));

        Ok(())
    }

    /// Get a relationship by ID
    pub fn get_relationship(&mut self, rel_id: RelationshipId) -> Result<Option<Relationship>> {
        self.check_active()?;

        // Check cache first
        if let Some(cached) = self.rel_cache.get(&rel_id) {
            return Ok(cached.clone());
        }

        let bytes = self
            .engine
            .transaction_relationship_bytes(self.graph_id, rel_id)?;
        let rel: Option<Relationship> = bytes
            .as_deref()
            .map(bincode::deserialize)
            .transpose()
            .map_err(|e| Error::Deserialization(e.to_string()))?;
        self.rel_observations.insert(rel_id, bytes);

        // Cache the result
        self.rel_cache.insert(rel_id, rel.clone());

        Ok(rel)
    }

    /// Delete a relationship
    pub fn delete_relationship(&mut self, rel_id: RelationshipId) -> Result<()> {
        self.check_active()?;

        self.observe_relationship_for_write(rel_id)?;
        // Mark as deleted in cache
        self.rel_cache.insert(rel_id, None);

        // Add to pending operations
        self.operations
            .push(TransactionOperation::DeleteRelationship(rel_id));

        Ok(())
    }

    // ========== Transaction Control ==========

    /// Commit the transaction
    ///
    /// Applies all pending operations atomically.
    pub fn commit(mut self) -> Result<()> {
        self.check_active()?;

        self.engine.apply_observed_transaction(
            self.graph_id,
            self.graph_identity.as_ref(),
            &self.operations,
            &self.node_observations,
            &self.rel_observations,
        )?;

        self.state = TransactionState::Committed;
        Ok(())
    }

    /// Rollback the transaction
    ///
    /// Discards all pending operations.
    pub fn rollback(mut self) -> Result<()> {
        if !self.is_active() {
            return Err(Error::TransactionAborted(
                "Transaction is no longer active".into(),
            ));
        }

        // Clear all pending operations
        self.operations.clear();
        self.node_cache.clear();
        self.rel_cache.clear();

        self.state = TransactionState::RolledBack;
        Ok(())
    }

    /// Get the number of pending operations
    pub fn pending_operations(&self) -> usize {
        self.operations.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::StorageOptions;
    use qilbee_core::{EntityId, IdGenerator};
    use tempfile::TempDir;

    fn create_test_engine() -> (StorageEngine, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let options = StorageOptions::for_testing(temp_dir.path());
        let engine = StorageEngine::open(options).unwrap();
        (engine, temp_dir)
    }

    #[test]
    fn concurrent_read_modify_write_rejects_stale_commit() {
        let (engine, _dir) = create_test_engine();
        let graph_id = GraphId::from_name("conflict");
        let node = Node::with_labels(IdGenerator::new().next_node_id(), ["Counter"]);
        engine.put_node(graph_id, &node).unwrap();
        let mut first = Transaction::new(engine.clone(), graph_id);
        let mut second = Transaction::new(engine.clone(), graph_id);
        let mut a = first.get_node(node.id).unwrap().unwrap();
        let mut b = second.get_node(node.id).unwrap().unwrap();
        a.set_property("value", 1i64);
        b.set_property("value", 2i64);
        first.put_node(a.clone()).unwrap();
        second.put_node(b).unwrap();
        first.commit().unwrap();
        let outcome = second.commit();
        let preserved = engine.get_node(graph_id, node.id).unwrap() == Some(a);
        assert!(
            matches!(&outcome, Err(Error::TransactionAborted(_))),
            "stale commit outcome={outcome:?}; first committed value preserved={preserved}"
        );
        assert!(preserved);
    }

    #[test]
    fn transaction_conflict_negative_read_protects_dependent_writes() {
        let (engine, _dir) = create_test_engine();
        let graph = GraphId::from_name("negative-read");
        let ids = IdGenerator::new();
        let absent = Node::with_labels(ids.next_node_id(), ["Reserved"]);
        let dependent = Node::with_labels(ids.next_node_id(), ["Dependent"]);
        let mut tx = Transaction::new(engine.clone(), graph);
        assert!(tx.get_node(absent.id).unwrap().is_none());
        tx.put_node(dependent.clone()).unwrap();
        engine.put_node(graph, &absent).unwrap();
        let outcome = tx.commit();
        assert!(
            matches!(&outcome, Err(Error::TransactionAborted(_))),
            "{outcome:?}"
        );
        assert!(engine.get_node(graph, dependent.id).unwrap().is_none());
    }

    #[test]
    fn transaction_conflict_relationship_delete_preserves_new_value() {
        let (engine, _dir) = create_test_engine();
        let graph = GraphId::from_name("relationship-conflict");
        let ids = IdGenerator::new();
        let mut relation = Relationship::new(
            ids.next_relationship_id(),
            "KNOWS",
            ids.next_node_id(),
            ids.next_node_id(),
        );
        engine.put_relationship(graph, &relation).unwrap();
        let mut tx = Transaction::new(engine.clone(), graph);
        tx.get_relationship(relation.id).unwrap();
        tx.delete_relationship(relation.id).unwrap();
        relation.set_property("revision", 2i64);
        engine.put_relationship(graph, &relation).unwrap();
        let outcome = tx.commit();
        assert!(
            matches!(&outcome, Err(Error::TransactionAborted(_))),
            "{outcome:?}"
        );
        assert_eq!(
            engine.get_relationship(graph, relation.id).unwrap(),
            Some(relation)
        );
    }

    #[test]
    fn transaction_conflict_blind_writes_observe_first_mutation() {
        let (engine, _dir) = create_test_engine();
        let graph = GraphId::from_name("blind-write-conflict");
        let mut node = Node::with_labels(IdGenerator::new().next_node_id(), ["Value"]);
        let mut first = Transaction::new(engine.clone(), graph);
        let mut second = Transaction::new(engine.clone(), graph);
        first.put_node(node.clone()).unwrap();
        node.set_property("value", 2i64);
        second.put_node(node).unwrap();
        first.commit().unwrap();
        let outcome = second.commit();
        assert!(
            matches!(&outcome, Err(Error::TransactionAborted(_))),
            "{outcome:?}"
        );
    }

    #[test]
    fn transaction_conflict_simultaneous_commits_have_one_winner() {
        let (engine, _dir) = create_test_engine();
        let graph = GraphId::from_name("race");
        let node = Node::with_labels(IdGenerator::new().next_node_id(), ["Counter"]);
        engine.put_node(graph, &node).unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let handles: Vec<_> = (1..=2)
            .map(|value| {
                let engine = engine.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let mut tx = Transaction::new(engine, graph);
                    let mut node = tx.get_node(node.id).unwrap().unwrap();
                    node.set_property("value", value as i64);
                    tx.put_node(node).unwrap();
                    barrier.wait();
                    tx.commit()
                })
            })
            .collect();
        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|r| matches!(r, Err(Error::TransactionAborted(_))))
                .count(),
            1
        );
    }

    #[test]
    fn transaction_conflict_read_only_dependencies_are_checked() {
        let (engine, _dir) = create_test_engine();
        let graph = GraphId::from_name("read-only");
        let node = Node::with_labels(IdGenerator::new().next_node_id(), ["Value"]);
        let mut tx = Transaction::new(engine.clone(), graph);
        assert!(tx.get_node(node.id).unwrap().is_none());
        engine.put_node(graph, &node).unwrap();
        assert!(matches!(tx.commit(), Err(Error::TransactionAborted(_))));
    }

    #[test]
    fn transaction_conflict_unrelated_entities_and_own_writes_remain_valid() {
        let (engine, _dir) = create_test_engine();
        let graph = GraphId::from_name("independent");
        let ids = IdGenerator::new();
        let mut a = Node::with_labels(ids.next_node_id(), ["A"]);
        let b = Node::with_labels(ids.next_node_id(), ["B"]);
        let mut first = Transaction::new(engine.clone(), graph);
        let mut second = Transaction::new(engine.clone(), graph);
        first.put_node(a.clone()).unwrap();
        first.delete_node(a.id).unwrap();
        assert!(first.get_node(a.id).unwrap().is_none());
        a.set_property("last", true);
        first.put_node(a.clone()).unwrap();
        assert_eq!(first.get_node(a.id).unwrap(), Some(a.clone()));
        second.put_node(b.clone()).unwrap();
        second.commit().unwrap();
        first.commit().unwrap();
        assert_eq!(engine.get_node(graph, a.id).unwrap(), Some(a));
        assert_eq!(engine.get_node(graph, b.id).unwrap(), Some(b));
    }

    #[test]
    fn transaction_conflict_preserves_all_indexes_after_reopen() {
        let directory = TempDir::new().unwrap();
        let mut options = StorageOptions::for_testing(directory.path());
        options.enable_wal = true;
        options.sync_wal = true;
        let engine = StorageEngine::open(options.clone()).unwrap();
        let graph = GraphId::from_name("conflict-recovery");
        let ids = IdGenerator::new();
        let mut source = Node::with_labels(ids.next_node_id(), ["Source"]);
        source.set_property("revision", 1i64);
        let mut candidate = Node::with_labels(ids.next_node_id(), ["Candidate"]);
        candidate.set_property("name", "must-not-publish");
        engine.put_node(graph, &source).unwrap();
        let relation = Relationship::new(
            ids.next_relationship_id(),
            "DERIVED",
            source.id,
            candidate.id,
        );
        let mut tx = Transaction::new(engine.clone(), graph);
        tx.get_node(source.id).unwrap();
        tx.put_node(candidate.clone()).unwrap();
        tx.put_relationship(relation.clone()).unwrap();
        source.set_property("revision", 2i64);
        engine.put_node(graph, &source).unwrap();
        assert!(matches!(tx.commit(), Err(Error::TransactionAborted(_))));
        let verify = |db: &StorageEngine| {
            assert_eq!(db.get_node(graph, source.id).unwrap(), Some(source.clone()));
            assert!(db.get_node(graph, candidate.id).unwrap().is_none());
            assert!(db.get_relationship(graph, relation.id).unwrap().is_none());
            assert!(
                db.get_nodes_by_label(graph, "Candidate")
                    .unwrap()
                    .is_empty()
            );
            assert!(
                db.get_nodes_by_property(
                    graph,
                    "Candidate",
                    "name",
                    &qilbee_core::PropertyValue::String("must-not-publish".into())
                )
                .unwrap()
                .is_empty()
            );
            assert!(
                db.get_nodes_by_property(
                    graph,
                    "Source",
                    "revision",
                    &qilbee_core::PropertyValue::Integer(1)
                )
                .unwrap()
                .is_empty()
            );
            assert_eq!(
                db.get_nodes_by_property(
                    graph,
                    "Source",
                    "revision",
                    &qilbee_core::PropertyValue::Integer(2)
                )
                .unwrap()
                .len(),
                1
            );
            assert!(
                db.get_outgoing_relationships(graph, source.id)
                    .unwrap()
                    .is_empty()
            );
            assert!(
                db.get_incoming_relationships(graph, candidate.id)
                    .unwrap()
                    .is_empty()
            );
        };
        verify(&engine);
        drop(engine);
        let reopened = StorageEngine::open(options).unwrap();
        verify(&reopened);
    }

    #[test]
    fn test_transaction_creation() {
        let (engine, _dir) = create_test_engine();
        let graph_id = GraphId::from_name("test");

        let tx = Transaction::new(engine, graph_id);
        assert!(tx.is_active());
        assert_eq!(tx.pending_operations(), 0);
    }

    #[test]
    fn test_transaction_commit() {
        let (engine, _dir) = create_test_engine();
        let id_gen = IdGenerator::new();
        let graph_id = GraphId::from_name("test");

        // Start transaction
        let mut tx = Transaction::new(engine.clone(), graph_id);

        // Create node
        let node = Node::with_labels(id_gen.next_node_id(), ["Person"]);
        tx.put_node(node.clone()).unwrap();

        // Node should be visible in transaction
        assert!(tx.get_node(node.id).unwrap().is_some());

        // Node should NOT be visible outside transaction yet
        assert!(engine.get_node(graph_id, node.id).unwrap().is_none());

        // Commit
        tx.commit().unwrap();

        // Now node should be visible
        assert!(engine.get_node(graph_id, node.id).unwrap().is_some());
    }

    #[test]
    fn test_transaction_rollback() {
        let (engine, _dir) = create_test_engine();
        let id_gen = IdGenerator::new();
        let graph_id = GraphId::from_name("test");

        // Start transaction
        let mut tx = Transaction::new(engine.clone(), graph_id);

        // Create node
        let node = Node::with_labels(id_gen.next_node_id(), ["Person"]);
        tx.put_node(node.clone()).unwrap();

        // Rollback
        tx.rollback().unwrap();

        // Node should NOT be visible
        assert!(engine.get_node(graph_id, node.id).unwrap().is_none());
    }

    #[test]
    fn test_transaction_delete() {
        let (engine, _dir) = create_test_engine();
        let id_gen = IdGenerator::new();
        let graph_id = GraphId::from_name("test");

        // Create node outside transaction
        let node = Node::with_labels(id_gen.next_node_id(), ["Person"]);
        engine.put_node(graph_id, &node).unwrap();

        // Start transaction and delete
        let mut tx = Transaction::new(engine.clone(), graph_id);
        tx.delete_node(node.id).unwrap();

        // Node should be marked as deleted in transaction
        assert!(tx.get_node(node.id).unwrap().is_none());

        // But still visible outside transaction
        assert!(engine.get_node(graph_id, node.id).unwrap().is_some());

        // Commit
        tx.commit().unwrap();

        // Now node should be deleted
        assert!(engine.get_node(graph_id, node.id).unwrap().is_none());
    }

    #[test]
    fn test_transaction_state() {
        let (engine, _dir) = create_test_engine();
        let graph_id = GraphId::from_name("test");

        let tx = Transaction::new(engine.clone(), graph_id);
        assert_eq!(tx.state(), TransactionState::Active);

        let tx2 = Transaction::new(engine.clone(), graph_id);
        tx2.commit().unwrap();

        let tx3 = Transaction::new(engine, graph_id);
        tx3.rollback().unwrap();
    }
}
