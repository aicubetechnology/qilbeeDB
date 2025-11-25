//! Protocol message types

use qilbee_core::PropertyValue;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A request from a client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {
    /// Execute a Cypher query
    Query {
        statement: String,
        parameters: HashMap<String, PropertyValue>,
    },

    /// Execute a read-only query
    ReadOnlyQuery {
        statement: String,
        parameters: HashMap<String, PropertyValue>,
    },

    /// Begin a transaction
    BeginTransaction,

    /// Commit current transaction
    Commit,

    /// Rollback current transaction
    Rollback,

    /// Get graph metadata
    GetGraphInfo { graph_name: String },

    /// List all graphs
    ListGraphs,

    /// Create a new graph
    CreateGraph { name: String },

    /// Delete a graph
    DeleteGraph { name: String },

    /// Ping/heartbeat
    Ping,

    /// Close connection
    Close,
}

impl Request {
    /// Create a query request
    pub fn query(statement: &str) -> Self {
        Self::Query {
            statement: statement.to_string(),
            parameters: HashMap::new(),
        }
    }

    /// Create a query request with parameters
    pub fn query_with_params(
        statement: &str,
        parameters: HashMap<String, PropertyValue>,
    ) -> Self {
        Self::Query {
            statement: statement.to_string(),
            parameters,
        }
    }

    /// Check if this is a read-only request
    pub fn is_read_only(&self) -> bool {
        matches!(
            self,
            Request::ReadOnlyQuery { .. }
                | Request::GetGraphInfo { .. }
                | Request::ListGraphs
                | Request::Ping
        )
    }
}

/// A response to a client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Response {
    /// Successful query result
    Success(QueryResult),

    /// Transaction started
    TransactionStarted { tx_id: u64 },

    /// Transaction committed
    Committed,

    /// Transaction rolled back
    RolledBack,

    /// Graph information
    GraphInfo(GraphInfo),

    /// List of graph names
    GraphList(Vec<String>),

    /// Graph created
    GraphCreated { name: String },

    /// Graph deleted
    GraphDeleted { name: String },

    /// Pong response
    Pong,

    /// Error response
    Error(ErrorResponse),

    /// Connection closed
    Closed,
}

impl Response {
    /// Create a success response
    pub fn success(result: QueryResult) -> Self {
        Self::Success(result)
    }

    /// Create an error response
    pub fn error(code: &str, message: &str) -> Self {
        Self::Error(ErrorResponse {
            code: code.to_string(),
            message: message.to_string(),
            details: None,
        })
    }

    /// Check if this is an error response
    pub fn is_error(&self) -> bool {
        matches!(self, Response::Error(_))
    }
}

/// Query result data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    /// Column names
    pub columns: Vec<String>,

    /// Rows of data
    pub rows: Vec<Vec<PropertyValue>>,

    /// Query statistics
    pub stats: QueryStats,
}

impl QueryResult {
    /// Create an empty result
    pub fn empty() -> Self {
        Self {
            columns: Vec::new(),
            rows: Vec::new(),
            stats: QueryStats::default(),
        }
    }

    /// Create a result with columns and rows
    pub fn new(columns: Vec<String>, rows: Vec<Vec<PropertyValue>>) -> Self {
        Self {
            columns,
            rows,
            stats: QueryStats::default(),
        }
    }

    /// Get row count
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }
}

/// Query execution statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QueryStats {
    /// Nodes created
    pub nodes_created: u64,

    /// Nodes deleted
    pub nodes_deleted: u64,

    /// Relationships created
    pub relationships_created: u64,

    /// Relationships deleted
    pub relationships_deleted: u64,

    /// Properties set
    pub properties_set: u64,

    /// Labels added
    pub labels_added: u64,

    /// Labels removed
    pub labels_removed: u64,

    /// Execution time in milliseconds
    pub execution_time_ms: u64,
}

/// Graph information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphInfo {
    /// Graph name
    pub name: String,

    /// Number of nodes
    pub node_count: u64,

    /// Number of relationships
    pub relationship_count: u64,

    /// Available labels
    pub labels: Vec<String>,

    /// Available relationship types
    pub relationship_types: Vec<String>,
}

/// Error response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponse {
    /// Error code
    pub code: String,

    /// Human-readable message
    pub message: String,

    /// Additional details
    pub details: Option<serde_json::Value>,
}

impl ErrorResponse {
    /// Create a new error response
    pub fn new(code: &str, message: &str) -> Self {
        Self {
            code: code.to_string(),
            message: message.to_string(),
            details: None,
        }
    }

    /// Add details to error
    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }
}

/// Common error codes
pub mod error_codes {
    pub const SYNTAX_ERROR: &str = "SYNTAX_ERROR";
    pub const SEMANTIC_ERROR: &str = "SEMANTIC_ERROR";
    pub const CONSTRAINT_VIOLATION: &str = "CONSTRAINT_VIOLATION";
    pub const ENTITY_NOT_FOUND: &str = "ENTITY_NOT_FOUND";
    pub const TRANSACTION_ERROR: &str = "TRANSACTION_ERROR";
    pub const INTERNAL_ERROR: &str = "INTERNAL_ERROR";
    pub const AUTHENTICATION_ERROR: &str = "AUTHENTICATION_ERROR";
    pub const AUTHORIZATION_ERROR: &str = "AUTHORIZATION_ERROR";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_creation() {
        let req = Request::query("MATCH (n) RETURN n");
        assert!(!req.is_read_only());

        let req = Request::Ping;
        assert!(req.is_read_only());
    }

    #[test]
    fn test_response_creation() {
        let resp = Response::success(QueryResult::empty());
        assert!(!resp.is_error());

        let resp = Response::error("TEST", "Test error");
        assert!(resp.is_error());
    }

    #[test]
    fn test_query_result() {
        let result = QueryResult::new(
            vec!["name".to_string()],
            vec![vec![PropertyValue::String("Alice".to_string())]],
        );

        assert_eq!(result.row_count(), 1);
        assert_eq!(result.columns.len(), 1);
    }

    #[test]
    fn test_error_response() {
        let err = ErrorResponse::new("TEST", "Test message")
            .with_details(serde_json::json!({"key": "value"}));

        assert_eq!(err.code, "TEST");
        assert!(err.details.is_some());
    }
}
