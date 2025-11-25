//! Core graph types for QilbeeDB
//!
//! Defines the fundamental building blocks: nodes, relationships, labels, and directions.

use crate::id::{NodeId, RelationshipId};
use crate::property::Property;
use crate::temporal::{BiTemporal, EventTime, TransactionTime};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// A label for nodes or relationship types
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Label(String);

impl Label {
    /// Create a new label
    pub fn new<S: Into<String>>(name: S) -> Self {
        Self(name.into())
    }

    /// Get the label name
    pub fn name(&self) -> &str {
        &self.0
    }

    /// Convert to owned string
    pub fn into_string(self) -> String {
        self.0
    }
}

impl From<&str> for Label {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl From<String> for Label {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

impl std::fmt::Display for Label {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Direction of a relationship traversal
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Direction {
    /// Outgoing relationship (->)
    Outgoing,
    /// Incoming relationship (<-)
    Incoming,
    /// Both directions (--)
    Both,
}

impl Direction {
    /// Returns the opposite direction
    pub fn reverse(self) -> Self {
        match self {
            Direction::Outgoing => Direction::Incoming,
            Direction::Incoming => Direction::Outgoing,
            Direction::Both => Direction::Both,
        }
    }
}

/// A node in the property graph
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    /// Unique identifier
    pub id: NodeId,

    /// Labels attached to this node
    pub labels: HashSet<Label>,

    /// Properties of this node
    pub properties: Property,

    /// When this node was created (event time)
    pub created_at: EventTime,

    /// When this node was stored (transaction time)
    pub stored_at: TransactionTime,
}

impl Node {
    /// Create a new node with the given ID
    pub fn new(id: NodeId) -> Self {
        Self {
            id,
            labels: HashSet::new(),
            properties: Property::new(),
            created_at: EventTime::now(),
            stored_at: TransactionTime::now(),
        }
    }

    /// Create a node with labels
    pub fn with_labels<I, L>(id: NodeId, labels: I) -> Self
    where
        I: IntoIterator<Item = L>,
        L: Into<Label>,
    {
        Self {
            id,
            labels: labels.into_iter().map(Into::into).collect(),
            properties: Property::new(),
            created_at: EventTime::now(),
            stored_at: TransactionTime::now(),
        }
    }

    /// Create a node with labels and properties
    pub fn with_labels_and_properties<I, L>(id: NodeId, labels: I, properties: Property) -> Self
    where
        I: IntoIterator<Item = L>,
        L: Into<Label>,
    {
        Self {
            id,
            labels: labels.into_iter().map(Into::into).collect(),
            properties,
            created_at: EventTime::now(),
            stored_at: TransactionTime::now(),
        }
    }

    /// Add a label to this node
    pub fn add_label<L: Into<Label>>(&mut self, label: L) {
        self.labels.insert(label.into());
    }

    /// Remove a label from this node
    pub fn remove_label(&mut self, label: &Label) -> bool {
        self.labels.remove(label)
    }

    /// Check if node has a specific label
    pub fn has_label(&self, label: &Label) -> bool {
        self.labels.contains(label)
    }

    /// Check if node has a label by name
    pub fn has_label_name(&self, name: &str) -> bool {
        self.labels.iter().any(|l| l.name() == name)
    }

    /// Set a property
    pub fn set_property<K: Into<String>, V: Into<crate::property::PropertyValue>>(
        &mut self,
        key: K,
        value: V,
    ) {
        self.properties.set(key, value);
    }

    /// Get a property
    pub fn get_property(&self, key: &str) -> Option<&crate::property::PropertyValue> {
        self.properties.get(key)
    }

    /// Remove a property
    pub fn remove_property(&mut self, key: &str) -> Option<crate::property::PropertyValue> {
        self.properties.remove(key)
    }
}

/// A relationship between two nodes
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Relationship {
    /// Unique identifier
    pub id: RelationshipId,

    /// The relationship type (like a label)
    pub rel_type: Label,

    /// Source node ID
    pub source: NodeId,

    /// Target node ID
    pub target: NodeId,

    /// Properties of this relationship
    pub properties: Property,

    /// When this relationship was created (event time)
    pub created_at: EventTime,

    /// When this relationship was stored (transaction time)
    pub stored_at: TransactionTime,
}

impl Relationship {
    /// Create a new relationship
    pub fn new<L: Into<Label>>(
        id: RelationshipId,
        rel_type: L,
        source: NodeId,
        target: NodeId,
    ) -> Self {
        Self {
            id,
            rel_type: rel_type.into(),
            source,
            target,
            properties: Property::new(),
            created_at: EventTime::now(),
            stored_at: TransactionTime::now(),
        }
    }

    /// Create a relationship with properties
    pub fn with_properties<L: Into<Label>>(
        id: RelationshipId,
        rel_type: L,
        source: NodeId,
        target: NodeId,
        properties: Property,
    ) -> Self {
        Self {
            id,
            rel_type: rel_type.into(),
            source,
            target,
            properties,
            created_at: EventTime::now(),
            stored_at: TransactionTime::now(),
        }
    }

    /// Get the node ID at the other end of the relationship
    pub fn other(&self, node_id: NodeId) -> Option<NodeId> {
        if self.source == node_id {
            Some(self.target)
        } else if self.target == node_id {
            Some(self.source)
        } else {
            None
        }
    }

    /// Check if this relationship connects to a node
    pub fn connects(&self, node_id: NodeId) -> bool {
        self.source == node_id || self.target == node_id
    }

    /// Check if this relationship connects two specific nodes
    pub fn connects_nodes(&self, a: NodeId, b: NodeId) -> bool {
        (self.source == a && self.target == b) || (self.source == b && self.target == a)
    }

    /// Set a property
    pub fn set_property<K: Into<String>, V: Into<crate::property::PropertyValue>>(
        &mut self,
        key: K,
        value: V,
    ) {
        self.properties.set(key, value);
    }

    /// Get a property
    pub fn get_property(&self, key: &str) -> Option<&crate::property::PropertyValue> {
        self.properties.get(key)
    }

    /// Remove a property
    pub fn remove_property(&mut self, key: &str) -> Option<crate::property::PropertyValue> {
        self.properties.remove(key)
    }
}

/// A path through the graph
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Path {
    /// Nodes in the path (in order)
    pub nodes: Vec<Node>,

    /// Relationships in the path (in order)
    pub relationships: Vec<Relationship>,
}

impl Path {
    /// Create an empty path
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            relationships: Vec::new(),
        }
    }

    /// Create a path starting with a single node
    pub fn from_node(node: Node) -> Self {
        Self {
            nodes: vec![node],
            relationships: Vec::new(),
        }
    }

    /// Add a relationship and target node to the path
    pub fn extend(&mut self, relationship: Relationship, node: Node) {
        self.relationships.push(relationship);
        self.nodes.push(node);
    }

    /// Get the length of the path (number of relationships)
    pub fn len(&self) -> usize {
        self.relationships.len()
    }

    /// Check if the path is empty (no nodes)
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Get the start node
    pub fn start(&self) -> Option<&Node> {
        self.nodes.first()
    }

    /// Get the end node
    pub fn end(&self) -> Option<&Node> {
        self.nodes.last()
    }
}

impl Default for Path {
    fn default() -> Self {
        Self::new()
    }
}

/// Graph entity that can be either a node or relationship
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum GraphEntity {
    Node(Node),
    Relationship(Relationship),
}

impl GraphEntity {
    /// Get properties regardless of entity type
    pub fn properties(&self) -> &Property {
        match self {
            GraphEntity::Node(n) => &n.properties,
            GraphEntity::Relationship(r) => &r.properties,
        }
    }

    /// Get mutable properties
    pub fn properties_mut(&mut self) -> &mut Property {
        match self {
            GraphEntity::Node(n) => &mut n.properties,
            GraphEntity::Relationship(r) => &mut r.properties,
        }
    }
}

impl From<Node> for GraphEntity {
    fn from(node: Node) -> Self {
        GraphEntity::Node(node)
    }
}

impl From<Relationship> for GraphEntity {
    fn from(rel: Relationship) -> Self {
        GraphEntity::Relationship(rel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::IdGenerator;

    #[test]
    fn test_label_creation() {
        let label = Label::new("Person");
        assert_eq!(label.name(), "Person");

        let label2: Label = "Company".into();
        assert_eq!(label2.name(), "Company");
    }

    #[test]
    fn test_direction_reverse() {
        assert_eq!(Direction::Outgoing.reverse(), Direction::Incoming);
        assert_eq!(Direction::Incoming.reverse(), Direction::Outgoing);
        assert_eq!(Direction::Both.reverse(), Direction::Both);
    }

    #[test]
    fn test_node_creation() {
        let id_gen = IdGenerator::new();
        let node = Node::new(id_gen.next_node_id());

        assert!(node.labels.is_empty());
        assert!(node.properties.is_empty());
    }

    #[test]
    fn test_node_with_labels() {
        let id_gen = IdGenerator::new();
        let node = Node::with_labels(id_gen.next_node_id(), ["Person", "Employee"]);

        assert!(node.has_label_name("Person"));
        assert!(node.has_label_name("Employee"));
        assert!(!node.has_label_name("Company"));
    }

    #[test]
    fn test_node_properties() {
        let id_gen = IdGenerator::new();
        let mut node = Node::new(id_gen.next_node_id());

        node.set_property("name", "Alice");
        node.set_property("age", 30i64);

        assert_eq!(
            node.get_property("name").and_then(|v| v.as_str()),
            Some("Alice")
        );
        assert_eq!(
            node.get_property("age").and_then(|v| v.as_integer()),
            Some(30)
        );
    }

    #[test]
    fn test_relationship_creation() {
        let id_gen = IdGenerator::new();
        let source = id_gen.next_node_id();
        let target = id_gen.next_node_id();

        let rel = Relationship::new(id_gen.next_relationship_id(), "KNOWS", source, target);

        assert_eq!(rel.rel_type.name(), "KNOWS");
        assert_eq!(rel.source, source);
        assert_eq!(rel.target, target);
    }

    #[test]
    fn test_relationship_other() {
        let id_gen = IdGenerator::new();
        let source = id_gen.next_node_id();
        let target = id_gen.next_node_id();
        let other = id_gen.next_node_id();

        let rel = Relationship::new(id_gen.next_relationship_id(), "KNOWS", source, target);

        assert_eq!(rel.other(source), Some(target));
        assert_eq!(rel.other(target), Some(source));
        assert_eq!(rel.other(other), None);
    }

    #[test]
    fn test_relationship_connects() {
        let id_gen = IdGenerator::new();
        let source = id_gen.next_node_id();
        let target = id_gen.next_node_id();
        let other = id_gen.next_node_id();

        let rel = Relationship::new(id_gen.next_relationship_id(), "KNOWS", source, target);

        assert!(rel.connects(source));
        assert!(rel.connects(target));
        assert!(!rel.connects(other));
        assert!(rel.connects_nodes(source, target));
        assert!(rel.connects_nodes(target, source));
    }

    #[test]
    fn test_path() {
        let id_gen = IdGenerator::new();
        let node1 = Node::with_labels(id_gen.next_node_id(), ["Person"]);
        let node2 = Node::with_labels(id_gen.next_node_id(), ["Person"]);

        let rel = Relationship::new(
            id_gen.next_relationship_id(),
            "KNOWS",
            node1.id,
            node2.id,
        );

        let mut path = Path::from_node(node1.clone());
        path.extend(rel, node2.clone());

        assert_eq!(path.len(), 1);
        assert_eq!(path.start().map(|n| n.id), Some(node1.id));
        assert_eq!(path.end().map(|n| n.id), Some(node2.id));
    }

    #[test]
    fn test_graph_entity() {
        let id_gen = IdGenerator::new();
        let mut node = Node::new(id_gen.next_node_id());
        node.set_property("name", "Test");

        let entity: GraphEntity = node.into();

        assert_eq!(
            entity.properties().get("name").and_then(|v| v.as_str()),
            Some("Test")
        );
    }
}
