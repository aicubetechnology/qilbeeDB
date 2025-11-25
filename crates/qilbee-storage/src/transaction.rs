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
/// Provides ACID semantics for a sequence of graph operations.
/// Changes are only visible after commit.
pub struct Transaction {
    /// Transaction ID
    id: u64,

    /// Graph this transaction operates on
    graph_id: GraphId,

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
}

impl Transaction {
    /// Create a new transaction
    pub fn new(engine: StorageEngine, graph_id: GraphId) -> Self {
        Self {
            id: TRANSACTION_COUNTER.fetch_add(1, Ordering::SeqCst),
            graph_id,
            engine,
            state: TransactionState::Active,
            operations: Vec::new(),
            node_cache: HashMap::new(),
            rel_cache: HashMap::new(),
        }
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
        if !self.is_active() {
            return Err(Error::TransactionAborted(
                "Transaction is no longer active".to_string(),
            ));
        }
        Ok(())
    }

    // ========== Node Operations ==========

    /// Create or update a node
    pub fn put_node(&mut self, node: Node) -> Result<()> {
        self.check_active()?;

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

        // Read from storage
        let node = self.engine.get_node(self.graph_id, node_id)?;

        // Cache the result
        self.node_cache.insert(node_id, node.clone());

        Ok(node)
    }

    /// Delete a node
    pub fn delete_node(&mut self, node_id: NodeId) -> Result<()> {
        self.check_active()?;

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

        // Read from storage
        let rel = self.engine.get_relationship(self.graph_id, rel_id)?;

        // Cache the result
        self.rel_cache.insert(rel_id, rel.clone());

        Ok(rel)
    }

    /// Delete a relationship
    pub fn delete_relationship(&mut self, rel_id: RelationshipId) -> Result<()> {
        self.check_active()?;

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

        // Apply all operations
        for op in self.operations.drain(..) {
            match op {
                TransactionOperation::PutNode(node) => {
                    self.engine.put_node(self.graph_id, &node)?;
                }
                TransactionOperation::DeleteNode(node_id) => {
                    self.engine.delete_node(self.graph_id, node_id)?;
                }
                TransactionOperation::PutRelationship(rel) => {
                    self.engine.put_relationship(self.graph_id, &rel)?;
                }
                TransactionOperation::DeleteRelationship(rel_id) => {
                    self.engine.delete_relationship(self.graph_id, rel_id)?;
                }
            }
        }

        self.state = TransactionState::Committed;
        Ok(())
    }

    /// Rollback the transaction
    ///
    /// Discards all pending operations.
    pub fn rollback(mut self) -> Result<()> {
        self.check_active()?;

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
