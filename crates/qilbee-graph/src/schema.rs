//! Schema management for graph databases

use qilbee_core::Label;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Type of index
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum IndexType {
    /// B-tree range index for ordered data
    Range,
    /// Full-text search index
    FullText,
    /// Vector similarity index (HNSW)
    Vector,
    /// Composite index on multiple properties
    Composite,
}

/// An index definition
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Index {
    /// Name of the index
    pub name: String,

    /// Type of index
    pub index_type: IndexType,

    /// Label this index applies to (for nodes)
    pub label: Label,

    /// Properties included in this index
    pub properties: Vec<String>,

    /// Whether the index is unique
    pub unique: bool,

    /// Vector dimension (for vector indexes)
    pub vector_dimension: Option<usize>,
}

impl Index {
    /// Create a new range index
    pub fn range<L: Into<Label>>(name: &str, label: L, property: &str) -> Self {
        Self {
            name: name.to_string(),
            index_type: IndexType::Range,
            label: label.into(),
            properties: vec![property.to_string()],
            unique: false,
            vector_dimension: None,
        }
    }

    /// Create a unique range index
    pub fn unique<L: Into<Label>>(name: &str, label: L, property: &str) -> Self {
        Self {
            name: name.to_string(),
            index_type: IndexType::Range,
            label: label.into(),
            properties: vec![property.to_string()],
            unique: true,
            vector_dimension: None,
        }
    }

    /// Create a full-text index
    pub fn fulltext<L: Into<Label>>(name: &str, label: L, properties: Vec<&str>) -> Self {
        Self {
            name: name.to_string(),
            index_type: IndexType::FullText,
            label: label.into(),
            properties: properties.into_iter().map(String::from).collect(),
            unique: false,
            vector_dimension: None,
        }
    }

    /// Create a vector index
    pub fn vector<L: Into<Label>>(
        name: &str,
        label: L,
        property: &str,
        dimension: usize,
    ) -> Self {
        Self {
            name: name.to_string(),
            index_type: IndexType::Vector,
            label: label.into(),
            properties: vec![property.to_string()],
            unique: false,
            vector_dimension: Some(dimension),
        }
    }

    /// Create a composite index
    pub fn composite<L: Into<Label>>(name: &str, label: L, properties: Vec<&str>) -> Self {
        Self {
            name: name.to_string(),
            index_type: IndexType::Composite,
            label: label.into(),
            properties: properties.into_iter().map(String::from).collect(),
            unique: false,
            vector_dimension: None,
        }
    }
}

/// Type of constraint
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConstraintType {
    /// Unique property value within a label
    Unique,
    /// Property must exist
    Exists,
    /// Node key (unique + exists for multiple properties)
    NodeKey,
}

/// A constraint definition
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Constraint {
    /// Name of the constraint
    pub name: String,

    /// Type of constraint
    pub constraint_type: ConstraintType,

    /// Label this constraint applies to
    pub label: Label,

    /// Properties included in this constraint
    pub properties: Vec<String>,
}

impl Constraint {
    /// Create a unique constraint
    pub fn unique<L: Into<Label>>(name: &str, label: L, property: &str) -> Self {
        Self {
            name: name.to_string(),
            constraint_type: ConstraintType::Unique,
            label: label.into(),
            properties: vec![property.to_string()],
        }
    }

    /// Create an existence constraint
    pub fn exists<L: Into<Label>>(name: &str, label: L, property: &str) -> Self {
        Self {
            name: name.to_string(),
            constraint_type: ConstraintType::Exists,
            label: label.into(),
            properties: vec![property.to_string()],
        }
    }

    /// Create a node key constraint
    pub fn node_key<L: Into<Label>>(name: &str, label: L, properties: Vec<&str>) -> Self {
        Self {
            name: name.to_string(),
            constraint_type: ConstraintType::NodeKey,
            label: label.into(),
            properties: properties.into_iter().map(String::from).collect(),
        }
    }
}

/// Schema for a graph, containing all indexes and constraints
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Schema {
    /// Indexes by name
    pub indexes: HashMap<String, Index>,

    /// Constraints by name
    pub constraints: HashMap<String, Constraint>,
}

impl Schema {
    /// Create a new empty schema
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an index
    pub fn add_index(&mut self, index: Index) -> bool {
        if self.indexes.contains_key(&index.name) {
            return false;
        }
        self.indexes.insert(index.name.clone(), index);
        true
    }

    /// Remove an index
    pub fn remove_index(&mut self, name: &str) -> Option<Index> {
        self.indexes.remove(name)
    }

    /// Get an index by name
    pub fn get_index(&self, name: &str) -> Option<&Index> {
        self.indexes.get(name)
    }

    /// Get indexes for a label
    pub fn indexes_for_label(&self, label: &Label) -> Vec<&Index> {
        self.indexes
            .values()
            .filter(|idx| &idx.label == label)
            .collect()
    }

    /// Add a constraint
    pub fn add_constraint(&mut self, constraint: Constraint) -> bool {
        if self.constraints.contains_key(&constraint.name) {
            return false;
        }
        self.constraints.insert(constraint.name.clone(), constraint);
        true
    }

    /// Remove a constraint
    pub fn remove_constraint(&mut self, name: &str) -> Option<Constraint> {
        self.constraints.remove(name)
    }

    /// Get a constraint by name
    pub fn get_constraint(&self, name: &str) -> Option<&Constraint> {
        self.constraints.get(name)
    }

    /// Get constraints for a label
    pub fn constraints_for_label(&self, label: &Label) -> Vec<&Constraint> {
        self.constraints
            .values()
            .filter(|c| &c.label == label)
            .collect()
    }

    /// Check if there's a unique constraint for a label/property
    pub fn has_unique_constraint(&self, label: &Label, property: &str) -> bool {
        self.constraints.values().any(|c| {
            &c.label == label
                && c.properties.contains(&property.to_string())
                && matches!(
                    c.constraint_type,
                    ConstraintType::Unique | ConstraintType::NodeKey
                )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_range_index() {
        let index = Index::range("idx_person_name", "Person", "name");
        assert_eq!(index.index_type, IndexType::Range);
        assert!(!index.unique);
        assert_eq!(index.properties, vec!["name"]);
    }

    #[test]
    fn test_unique_index() {
        let index = Index::unique("idx_user_email", "User", "email");
        assert!(index.unique);
    }

    #[test]
    fn test_vector_index() {
        let index = Index::vector("idx_doc_embedding", "Document", "embedding", 1536);
        assert_eq!(index.index_type, IndexType::Vector);
        assert_eq!(index.vector_dimension, Some(1536));
    }

    #[test]
    fn test_constraint() {
        let constraint = Constraint::unique("uniq_email", "User", "email");
        assert_eq!(constraint.constraint_type, ConstraintType::Unique);
    }

    #[test]
    fn test_node_key_constraint() {
        let constraint = Constraint::node_key("key_person", "Person", vec!["ssn", "country"]);
        assert_eq!(constraint.constraint_type, ConstraintType::NodeKey);
        assert_eq!(constraint.properties.len(), 2);
    }

    #[test]
    fn test_schema() {
        let mut schema = Schema::new();

        let index = Index::range("idx_name", "Person", "name");
        assert!(schema.add_index(index.clone()));
        assert!(!schema.add_index(index)); // Duplicate

        let constraint = Constraint::unique("uniq_email", "User", "email");
        assert!(schema.add_constraint(constraint));

        assert!(schema.get_index("idx_name").is_some());
        assert!(schema.get_constraint("uniq_email").is_some());
    }

    #[test]
    fn test_indexes_for_label() {
        let mut schema = Schema::new();

        schema.add_index(Index::range("idx1", "Person", "name"));
        schema.add_index(Index::range("idx2", "Person", "age"));
        schema.add_index(Index::range("idx3", "Company", "name"));

        let person_label = Label::new("Person");
        let indexes = schema.indexes_for_label(&person_label);
        assert_eq!(indexes.len(), 2);
    }

    #[test]
    fn test_has_unique_constraint() {
        let mut schema = Schema::new();
        schema.add_constraint(Constraint::unique("uniq", "User", "email"));

        let user_label = Label::new("User");
        assert!(schema.has_unique_constraint(&user_label, "email"));
        assert!(!schema.has_unique_constraint(&user_label, "name"));
    }
}
