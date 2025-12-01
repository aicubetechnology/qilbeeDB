//! Graph instance implementation

use crate::schema::Schema;
use qilbee_core::{
    Direction, EntityId, Error, GraphId, IdGenerator, Label, Node, NodeId, Property,
    PropertyValue, Relationship, RelationshipId, Result,
};
use qilbee_storage::{StorageEngine, Transaction};
use std::sync::{Arc, RwLock};
use tracing::{debug, info};

/// A graph instance in QilbeeDB
pub struct Graph {
    /// Graph identifier
    id: GraphId,

    /// Name of the graph
    name: String,

    /// Storage engine reference
    storage: StorageEngine,

    /// ID generator for this graph
    id_gen: Arc<IdGenerator>,

    /// Schema for this graph
    schema: Arc<RwLock<Schema>>,
}

impl Graph {
    /// Create a new graph instance (internal use)
    pub(crate) fn new(name: String, storage: StorageEngine) -> Self {
        let id = GraphId::from_name(&name);
        Self {
            id,
            name,
            storage,
            id_gen: Arc::new(IdGenerator::new()),
            schema: Arc::new(RwLock::new(Schema::new())),
        }
    }

    /// Get the graph ID
    pub fn id(&self) -> GraphId {
        self.id
    }

    /// Get the graph name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get a reference to the schema
    pub fn schema(&self) -> &Arc<RwLock<Schema>> {
        &self.schema
    }

    /// Get a reference to the storage engine
    pub fn storage(&self) -> StorageEngine {
        self.storage.clone()
    }

    // ========== Node Operations ==========

    /// Create a new node with the given labels
    pub fn create_node<I, L>(&self, labels: I) -> Result<Node>
    where
        I: IntoIterator<Item = L>,
        L: Into<Label>,
    {
        let node = Node::with_labels(self.id_gen.next_node_id(), labels);
        self.storage.put_node(self.id, &node)?;
        debug!("Created node {:?} in graph {}", node.id, self.name);
        Ok(node)
    }

    /// Create a new node with labels and properties
    pub fn create_node_with_properties<I, L>(
        &self,
        labels: I,
        properties: Property,
    ) -> Result<Node>
    where
        I: IntoIterator<Item = L>,
        L: Into<Label>,
    {
        let node = Node::with_labels_and_properties(self.id_gen.next_node_id(), labels, properties);

        // Check unique constraints
        self.check_node_constraints(&node)?;

        self.storage.put_node(self.id, &node)?;
        debug!("Created node {:?} in graph {}", node.id, self.name);
        Ok(node)
    }

    /// Get a node by ID
    pub fn get_node(&self, node_id: NodeId) -> Result<Option<Node>> {
        self.storage.get_node(self.id, node_id)
    }

    /// Update a node
    pub fn update_node(&self, node: &Node) -> Result<()> {
        // Verify node exists
        if self.storage.get_node(self.id, node.id)?.is_none() {
            return Err(Error::NodeNotFound(format!("{:?}", node.id)));
        }

        // Check constraints
        self.check_node_constraints(node)?;

        self.storage.put_node(self.id, node)?;
        debug!("Updated node {:?} in graph {}", node.id, self.name);
        Ok(())
    }

    /// Delete a node (must have no relationships)
    pub fn delete_node(&self, node_id: NodeId) -> Result<bool> {
        // Check for relationships
        let outgoing = self.storage.get_outgoing_relationships(self.id, node_id)?;
        let incoming = self.storage.get_incoming_relationships(self.id, node_id)?;

        if !outgoing.is_empty() || !incoming.is_empty() {
            return Err(Error::InvalidGraphOperation(format!(
                "Cannot delete node {:?}: has {} outgoing and {} incoming relationships. Use detach_delete_node instead.",
                node_id,
                outgoing.len(),
                incoming.len()
            )));
        }

        self.storage.delete_node(self.id, node_id)
    }

    /// Delete a node and all its relationships
    pub fn detach_delete_node(&self, node_id: NodeId) -> Result<bool> {
        // Delete all relationships first
        let outgoing = self.storage.get_outgoing_relationships(self.id, node_id)?;
        for rel in outgoing {
            self.storage.delete_relationship(self.id, rel.id)?;
        }

        let incoming = self.storage.get_incoming_relationships(self.id, node_id)?;
        for rel in incoming {
            self.storage.delete_relationship(self.id, rel.id)?;
        }

        // Now delete the node
        self.storage.delete_node(self.id, node_id)
    }

    /// Find nodes by label
    pub fn find_nodes_by_label(&self, label: &str) -> Result<Vec<Node>> {
        self.storage.get_nodes_by_label(self.id, label)
    }

    /// Get all nodes in this graph
    pub fn get_all_nodes(&self) -> Result<Vec<Node>> {
        self.storage.get_all_nodes(self.id)
    }

    /// Find nodes by label and property value using property index
    /// This is an efficient lookup that uses the property index
    pub fn find_nodes_by_label_and_property(
        &self,
        label: &str,
        property: &str,
        value: &PropertyValue,
    ) -> Result<Vec<Node>> {
        // Use property index for efficient lookup
        self.storage.get_nodes_by_property(self.id, label, property, value)
    }

    /// Find nodes by label and property range
    /// Returns nodes where the property value is between min and max (inclusive)
    pub fn find_nodes_by_property_range(
        &self,
        label: &str,
        property: &str,
        min_value: Option<&PropertyValue>,
        max_value: Option<&PropertyValue>,
    ) -> Result<Vec<Node>> {
        self.storage.get_nodes_by_property_range(self.id, label, property, min_value, max_value)
    }

    /// Find nodes that have a specific property (any value)
    pub fn find_nodes_with_property(
        &self,
        label: &str,
        property: &str,
    ) -> Result<Vec<Node>> {
        self.storage.get_nodes_with_property(self.id, label, property)
    }

    // ========== Relationship Operations ==========

    /// Create a relationship between two nodes
    pub fn create_relationship<L: Into<Label>>(
        &self,
        source: NodeId,
        rel_type: L,
        target: NodeId,
    ) -> Result<Relationship> {
        // Verify both nodes exist
        if self.storage.get_node(self.id, source)?.is_none() {
            return Err(Error::NodeNotFound(format!("{:?}", source)));
        }
        if self.storage.get_node(self.id, target)?.is_none() {
            return Err(Error::NodeNotFound(format!("{:?}", target)));
        }

        let rel = Relationship::new(self.id_gen.next_relationship_id(), rel_type, source, target);
        self.storage.put_relationship(self.id, &rel)?;

        debug!(
            "Created relationship {:?} in graph {}",
            rel.id, self.name
        );
        Ok(rel)
    }

    /// Create a relationship with properties
    pub fn create_relationship_with_properties<L: Into<Label>>(
        &self,
        source: NodeId,
        rel_type: L,
        target: NodeId,
        properties: Property,
    ) -> Result<Relationship> {
        // Verify both nodes exist
        if self.storage.get_node(self.id, source)?.is_none() {
            return Err(Error::NodeNotFound(format!("{:?}", source)));
        }
        if self.storage.get_node(self.id, target)?.is_none() {
            return Err(Error::NodeNotFound(format!("{:?}", target)));
        }

        let rel = Relationship::with_properties(
            self.id_gen.next_relationship_id(),
            rel_type,
            source,
            target,
            properties,
        );
        self.storage.put_relationship(self.id, &rel)?;

        debug!(
            "Created relationship {:?} in graph {}",
            rel.id, self.name
        );
        Ok(rel)
    }

    /// Get a relationship by ID
    pub fn get_relationship(&self, rel_id: RelationshipId) -> Result<Option<Relationship>> {
        self.storage.get_relationship(self.id, rel_id)
    }

    /// Update a relationship
    pub fn update_relationship(&self, rel: &Relationship) -> Result<()> {
        // Verify relationship exists
        if self.storage.get_relationship(self.id, rel.id)?.is_none() {
            return Err(Error::RelationshipNotFound(format!("{:?}", rel.id)));
        }

        self.storage.put_relationship(self.id, rel)?;
        debug!("Updated relationship {:?} in graph {}", rel.id, self.name);
        Ok(())
    }

    /// Delete a relationship
    pub fn delete_relationship(&self, rel_id: RelationshipId) -> Result<bool> {
        self.storage.delete_relationship(self.id, rel_id)
    }

    /// Get relationships from a node
    pub fn get_relationships(
        &self,
        node_id: NodeId,
        direction: Direction,
    ) -> Result<Vec<Relationship>> {
        match direction {
            Direction::Outgoing => self.storage.get_outgoing_relationships(self.id, node_id),
            Direction::Incoming => self.storage.get_incoming_relationships(self.id, node_id),
            Direction::Both => {
                let mut rels = self.storage.get_outgoing_relationships(self.id, node_id)?;
                rels.extend(self.storage.get_incoming_relationships(self.id, node_id)?);
                Ok(rels)
            }
        }
    }

    /// Get relationships from a node with a specific type
    pub fn get_relationships_by_type(
        &self,
        node_id: NodeId,
        direction: Direction,
        rel_type: &str,
    ) -> Result<Vec<Relationship>> {
        let rels = self.get_relationships(node_id, direction)?;
        Ok(rels
            .into_iter()
            .filter(|r| r.rel_type.name() == rel_type)
            .collect())
    }

    /// Get neighbors of a node
    pub fn get_neighbors(&self, node_id: NodeId, direction: Direction) -> Result<Vec<Node>> {
        let rels = self.get_relationships(node_id, direction)?;
        let mut neighbors = Vec::with_capacity(rels.len());

        for rel in rels {
            let neighbor_id = match direction {
                Direction::Outgoing => rel.target,
                Direction::Incoming => rel.source,
                Direction::Both => rel.other(node_id).unwrap_or(rel.target),
            };

            if let Some(node) = self.storage.get_node(self.id, neighbor_id)? {
                neighbors.push(node);
            }
        }

        Ok(neighbors)
    }

    // ========== Transaction Support ==========

    /// Begin a new transaction
    pub fn begin_transaction(&self) -> Transaction {
        Transaction::new(self.storage.clone(), self.id)
    }

    // ========== Private Helpers ==========

    fn check_node_constraints(&self, node: &Node) -> Result<()> {
        let schema = self.schema.read().map_err(|_| {
            Error::Internal("Failed to acquire schema lock".to_string())
        })?;

        for label in &node.labels {
            for constraint in schema.constraints_for_label(label) {
                match constraint.constraint_type {
                    crate::schema::ConstraintType::Unique => {
                        // Check uniqueness for each property in the constraint
                        for prop_name in &constraint.properties {
                            if let Some(value) = node.get_property(prop_name) {
                                let existing = self.find_nodes_by_label_and_property(
                                    label.name(),
                                    prop_name,
                                    value,
                                )?;

                                // Allow if only match is the node itself
                                let conflicts: Vec<_> = existing
                                    .into_iter()
                                    .filter(|n| n.id != node.id)
                                    .collect();

                                if !conflicts.is_empty() {
                                    return Err(Error::UniqueViolation {
                                        label: label.name().to_string(),
                                        property: prop_name.clone(),
                                    });
                                }
                            }
                        }
                    }
                    crate::schema::ConstraintType::Exists => {
                        for prop_name in &constraint.properties {
                            if !node.properties.contains(prop_name) {
                                return Err(Error::ConstraintViolation(format!(
                                    "Property '{}' is required for label '{}'",
                                    prop_name,
                                    label.name()
                                )));
                            }
                        }
                    }
                    crate::schema::ConstraintType::NodeKey => {
                        // Node key = Exists + Unique for all properties combined
                        for prop_name in &constraint.properties {
                            if !node.properties.contains(prop_name) {
                                return Err(Error::NodeKeyViolation(format!(
                                    "Property '{}' is required for node key on label '{}'",
                                    prop_name,
                                    label.name()
                                )));
                            }
                        }
                        // TODO: Check uniqueness of combined properties
                    }
                }
            }
        }

        Ok(())
    }
}

impl Clone for Graph {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            name: self.name.clone(),
            storage: self.storage.clone(),
            id_gen: Arc::clone(&self.id_gen),
            schema: Arc::clone(&self.schema),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::Constraint;
    use qilbee_storage::StorageOptions;
    use tempfile::TempDir;

    fn create_test_graph() -> (Graph, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let options = StorageOptions::for_testing(temp_dir.path());
        let storage = StorageEngine::open(options).unwrap();
        let graph = Graph::new("test".to_string(), storage);
        (graph, temp_dir)
    }

    #[test]
    fn test_create_node() {
        let (graph, _dir) = create_test_graph();

        let node = graph.create_node(["Person"]).unwrap();
        assert!(node.has_label_name("Person"));

        let retrieved = graph.get_node(node.id).unwrap().unwrap();
        assert_eq!(retrieved.id, node.id);
    }

    #[test]
    fn test_create_node_with_properties() {
        let (graph, _dir) = create_test_graph();

        let mut props = Property::new();
        props.set("name", "Alice");
        props.set("age", 30i64);

        let node = graph
            .create_node_with_properties(["Person"], props)
            .unwrap();

        assert_eq!(
            node.get_property("name").and_then(|v| v.as_str()),
            Some("Alice")
        );
    }

    #[test]
    fn test_find_nodes_by_label() {
        let (graph, _dir) = create_test_graph();

        graph.create_node(["Person"]).unwrap();
        graph.create_node(["Person"]).unwrap();
        graph.create_node(["Company"]).unwrap();

        let people = graph.find_nodes_by_label("Person").unwrap();
        assert_eq!(people.len(), 2);

        let companies = graph.find_nodes_by_label("Company").unwrap();
        assert_eq!(companies.len(), 1);
    }

    #[test]
    fn test_create_relationship() {
        let (graph, _dir) = create_test_graph();

        let alice = graph.create_node(["Person"]).unwrap();
        let bob = graph.create_node(["Person"]).unwrap();

        let rel = graph.create_relationship(alice.id, "KNOWS", bob.id).unwrap();

        assert_eq!(rel.source, alice.id);
        assert_eq!(rel.target, bob.id);
        assert_eq!(rel.rel_type.name(), "KNOWS");
    }

    #[test]
    fn test_get_relationships() {
        let (graph, _dir) = create_test_graph();

        let alice = graph.create_node(["Person"]).unwrap();
        let bob = graph.create_node(["Person"]).unwrap();
        let charlie = graph.create_node(["Person"]).unwrap();

        graph.create_relationship(alice.id, "KNOWS", bob.id).unwrap();
        graph
            .create_relationship(alice.id, "KNOWS", charlie.id)
            .unwrap();

        let outgoing = graph.get_relationships(alice.id, Direction::Outgoing).unwrap();
        assert_eq!(outgoing.len(), 2);

        let incoming = graph.get_relationships(bob.id, Direction::Incoming).unwrap();
        assert_eq!(incoming.len(), 1);
    }

    #[test]
    fn test_get_neighbors() {
        let (graph, _dir) = create_test_graph();

        let alice = graph.create_node(["Person"]).unwrap();
        let bob = graph.create_node(["Person"]).unwrap();
        let charlie = graph.create_node(["Person"]).unwrap();

        graph.create_relationship(alice.id, "KNOWS", bob.id).unwrap();
        graph
            .create_relationship(alice.id, "KNOWS", charlie.id)
            .unwrap();

        let neighbors = graph.get_neighbors(alice.id, Direction::Outgoing).unwrap();
        assert_eq!(neighbors.len(), 2);
    }

    #[test]
    fn test_delete_node_with_relationships() {
        let (graph, _dir) = create_test_graph();

        let alice = graph.create_node(["Person"]).unwrap();
        let bob = graph.create_node(["Person"]).unwrap();
        graph.create_relationship(alice.id, "KNOWS", bob.id).unwrap();

        // Should fail - has relationships
        assert!(graph.delete_node(alice.id).is_err());

        // Should succeed with detach
        assert!(graph.detach_delete_node(alice.id).unwrap());
        assert!(graph.get_node(alice.id).unwrap().is_none());
    }

    #[test]
    fn test_unique_constraint() {
        let (graph, _dir) = create_test_graph();

        // Add unique constraint
        {
            let mut schema = graph.schema.write().unwrap();
            schema.add_constraint(Constraint::unique("uniq_email", "User", "email"));
        }

        // Create first user
        let mut props = Property::new();
        props.set("email", "alice@example.com");
        graph
            .create_node_with_properties(["User"], props)
            .unwrap();

        // Try to create duplicate - should fail
        let mut props2 = Property::new();
        props2.set("email", "alice@example.com");
        let result = graph.create_node_with_properties(["User"], props2);
        assert!(result.is_err());
    }

    #[test]
    fn test_find_nodes_by_property() {
        let (graph, _dir) = create_test_graph();

        // Create nodes with properties
        let mut props1 = Property::new();
        props1.set("name", "Alice");
        props1.set("age", 30i64);
        graph.create_node_with_properties(["Person"], props1).unwrap();

        let mut props2 = Property::new();
        props2.set("name", "Bob");
        props2.set("age", 25i64);
        graph.create_node_with_properties(["Person"], props2).unwrap();

        let mut props3 = Property::new();
        props3.set("name", "Alice");
        props3.set("age", 35i64);
        graph.create_node_with_properties(["Person"], props3).unwrap();

        // Find by name
        let alices = graph
            .find_nodes_by_label_and_property("Person", "name", &PropertyValue::String("Alice".to_string()))
            .unwrap();
        assert_eq!(alices.len(), 2);

        let bobs = graph
            .find_nodes_by_label_and_property("Person", "name", &PropertyValue::String("Bob".to_string()))
            .unwrap();
        assert_eq!(bobs.len(), 1);
    }

    #[test]
    fn test_find_nodes_with_property() {
        let (graph, _dir) = create_test_graph();

        // Create nodes - some with email, some without
        let mut props1 = Property::new();
        props1.set("name", "Alice");
        props1.set("email", "alice@example.com");
        graph.create_node_with_properties(["Person"], props1).unwrap();

        let mut props2 = Property::new();
        props2.set("name", "Bob");
        // No email
        graph.create_node_with_properties(["Person"], props2).unwrap();

        let mut props3 = Property::new();
        props3.set("name", "Charlie");
        props3.set("email", "charlie@example.com");
        graph.create_node_with_properties(["Person"], props3).unwrap();

        // Find nodes that have email property
        let with_email = graph.find_nodes_with_property("Person", "email").unwrap();
        assert_eq!(with_email.len(), 2);

        // Find nodes that have name property
        let with_name = graph.find_nodes_with_property("Person", "name").unwrap();
        assert_eq!(with_name.len(), 3);
    }

    #[test]
    fn test_find_nodes_by_property_range() {
        let (graph, _dir) = create_test_graph();

        // Create nodes with integer properties
        for age in [20i64, 30, 40, 50] {
            let mut props = Property::new();
            props.set("age", age);
            graph.create_node_with_properties(["Person"], props).unwrap();
        }

        // Range: 25 <= age <= 45
        let min_age = PropertyValue::Integer(25);
        let max_age = PropertyValue::Integer(45);
        let in_range = graph
            .find_nodes_by_property_range("Person", "age", Some(&min_age), Some(&max_age))
            .unwrap();
        assert_eq!(in_range.len(), 2); // ages 30 and 40

        // Range: age >= 35
        let min_age = PropertyValue::Integer(35);
        let at_least_35 = graph
            .find_nodes_by_property_range("Person", "age", Some(&min_age), None)
            .unwrap();
        assert_eq!(at_least_35.len(), 2); // ages 40 and 50
    }

    #[test]
    fn test_property_index_after_update() {
        let (graph, _dir) = create_test_graph();

        // Create a node
        let mut props = Property::new();
        props.set("name", "Alice");
        let mut node = graph.create_node_with_properties(["Person"], props).unwrap();

        // Verify initial index
        let alices = graph
            .find_nodes_by_label_and_property("Person", "name", &PropertyValue::String("Alice".to_string()))
            .unwrap();
        assert_eq!(alices.len(), 1);

        // Update the node
        node.set_property("name", "Alicia");
        graph.update_node(&node).unwrap();

        // Old value should not be found
        let alices = graph
            .find_nodes_by_label_and_property("Person", "name", &PropertyValue::String("Alice".to_string()))
            .unwrap();
        assert_eq!(alices.len(), 0);

        // New value should be found
        let alicias = graph
            .find_nodes_by_label_and_property("Person", "name", &PropertyValue::String("Alicia".to_string()))
            .unwrap();
        assert_eq!(alicias.len(), 1);
    }

    #[test]
    fn test_property_index_after_delete() {
        let (graph, _dir) = create_test_graph();

        // Create a node
        let mut props = Property::new();
        props.set("name", "Alice");
        let node = graph.create_node_with_properties(["Person"], props).unwrap();

        // Verify index
        let alices = graph
            .find_nodes_by_label_and_property("Person", "name", &PropertyValue::String("Alice".to_string()))
            .unwrap();
        assert_eq!(alices.len(), 1);

        // Delete the node
        graph.delete_node(node.id).unwrap();

        // Index should be empty
        let alices = graph
            .find_nodes_by_label_and_property("Person", "name", &PropertyValue::String("Alice".to_string()))
            .unwrap();
        assert_eq!(alices.len(), 0);
    }
}
