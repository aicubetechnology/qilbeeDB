//! Error types for QilbeeDB
//!
//! Provides a comprehensive error hierarchy for all database operations.

use thiserror::Error;

/// The main error type for QilbeeDB operations
#[derive(Error, Debug)]
pub enum Error {
    // ========== Storage Errors ==========
    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Key not found: {0}")]
    KeyNotFound(String),

    #[error("Data corruption detected: {0}")]
    DataCorruption(String),

    // ========== Graph Errors ==========
    #[error("Node not found: {0}")]
    NodeNotFound(String),

    #[error("Relationship not found: {0}")]
    RelationshipNotFound(String),

    #[error("Graph not found: {0}")]
    GraphNotFound(String),

    #[error("Invalid graph operation: {0}")]
    InvalidGraphOperation(String),

    // ========== Query Errors ==========
    #[error("Query parse error: {0}")]
    QueryParse(String),

    #[error("Query execution error: {0}")]
    QueryExecution(String),

    #[error("Invalid query: {0}")]
    InvalidQuery(String),

    #[error("Type mismatch: expected {expected}, found {found}")]
    TypeMismatch { expected: String, found: String },

    // ========== Constraint Errors ==========
    #[error("Constraint violation: {0}")]
    ConstraintViolation(String),

    #[error("Unique constraint violation on {property} for label {label}")]
    UniqueViolation { label: String, property: String },

    #[error("Node key constraint violation: {0}")]
    NodeKeyViolation(String),

    // ========== Transaction Errors ==========
    #[error("Transaction aborted: {0}")]
    TransactionAborted(String),

    #[error("Transaction conflict: {0}")]
    TransactionConflict(String),

    #[error("Transaction timeout")]
    TransactionTimeout,

    // ========== Memory Errors ==========
    #[error("Memory operation error: {0}")]
    MemoryOperation(String),

    #[error("Invalid temporal range: {0}")]
    InvalidTemporalRange(String),

    // ========== Index Errors ==========
    #[error("Index not found: {0}")]
    IndexNotFound(String),

    #[error("Index already exists: {0}")]
    IndexAlreadyExists(String),

    #[error("Index operation error: {0}")]
    IndexOperation(String),

    // ========== Serialization Errors ==========
    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Deserialization error: {0}")]
    Deserialization(String),

    // ========== IO Errors ==========
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    // ========== Configuration Errors ==========
    #[error("Configuration error: {0}")]
    Configuration(String),

    // ========== Authentication/Authorization Errors ==========
    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    #[error("Token revoked: {0}")]
    TokenRevoked(String),

    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    // ========== Validation Errors ==========
    #[error("Weak password: {0}")]
    WeakPassword(String),

    #[error("Validation error: {0}")]
    ValidationError(String),

    // ========== Internal Errors ==========
    #[error("Internal error: {0}")]
    Internal(String),
}

/// Result type alias for QilbeeDB operations
pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// Returns true if this error is recoverable
    pub fn is_recoverable(&self) -> bool {
        matches!(
            self,
            Error::TransactionConflict(_)
                | Error::TransactionTimeout
                | Error::KeyNotFound(_)
                | Error::NodeNotFound(_)
                | Error::RelationshipNotFound(_)
        )
    }

    /// Returns true if this error indicates data corruption
    pub fn is_corruption(&self) -> bool {
        matches!(self, Error::DataCorruption(_))
    }

    /// Returns true if this error is a constraint violation
    pub fn is_constraint_violation(&self) -> bool {
        matches!(
            self,
            Error::ConstraintViolation(_)
                | Error::UniqueViolation { .. }
                | Error::NodeKeyViolation(_)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = Error::NodeNotFound("123".to_string());
        assert_eq!(err.to_string(), "Node not found: 123");
    }

    #[test]
    fn test_error_recoverable() {
        assert!(Error::TransactionConflict("test".to_string()).is_recoverable());
        assert!(Error::TransactionTimeout.is_recoverable());
        assert!(!Error::DataCorruption("test".to_string()).is_recoverable());
    }

    #[test]
    fn test_error_constraint_violation() {
        assert!(Error::UniqueViolation {
            label: "User".to_string(),
            property: "email".to_string()
        }
        .is_constraint_violation());
        assert!(!Error::NodeNotFound("123".to_string()).is_constraint_violation());
    }
}
