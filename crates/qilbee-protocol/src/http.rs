//! HTTP/REST API types (placeholder)

use crate::message::{QueryResult, Request, Response};
use serde::{Deserialize, Serialize};

/// HTTP methods
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
    Patch,
    Head,
    Options,
}

/// HTTP status codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusCode(pub u16);

impl StatusCode {
    pub const OK: Self = Self(200);
    pub const CREATED: Self = Self(201);
    pub const NO_CONTENT: Self = Self(204);
    pub const BAD_REQUEST: Self = Self(400);
    pub const UNAUTHORIZED: Self = Self(401);
    pub const FORBIDDEN: Self = Self(403);
    pub const NOT_FOUND: Self = Self(404);
    pub const INTERNAL_SERVER_ERROR: Self = Self(500);
}

/// HTTP request
#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub method: HttpMethod,
    pub path: String,
    pub headers: std::collections::HashMap<String, String>,
    pub body: Option<Vec<u8>>,
}

/// HTTP response
#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: StatusCode,
    pub headers: std::collections::HashMap<String, String>,
    pub body: Option<Vec<u8>>,
}

/// HTTP API endpoint paths
pub mod endpoints {
    pub const QUERY: &str = "/db/{graph}/query";
    pub const TRANSACTION: &str = "/db/{graph}/transaction";
    pub const GRAPHS: &str = "/db";
    pub const GRAPH_INFO: &str = "/db/{graph}";
    pub const SCHEMA: &str = "/db/{graph}/schema";
    pub const MEMORY: &str = "/db/{graph}/memory";
    pub const HEALTH: &str = "/health";
}

/// HTTP query request body
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryRequest {
    /// Cypher query statement
    pub statement: String,

    /// Query parameters
    #[serde(default)]
    pub parameters: std::collections::HashMap<String, serde_json::Value>,
}

/// HTTP query response body
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResponse {
    /// Whether the query was successful
    pub success: bool,

    /// Result data (if successful)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<QueryResultDto>,

    /// Error information (if failed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorDto>,
}

/// Query result for HTTP responses
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResultDto {
    /// Column names
    pub columns: Vec<String>,

    /// Row data
    pub data: Vec<Vec<serde_json::Value>>,

    /// Execution statistics
    pub stats: QueryStatsDto,
}

/// Query statistics for HTTP responses
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QueryStatsDto {
    pub nodes_created: u64,
    pub nodes_deleted: u64,
    pub relationships_created: u64,
    pub relationships_deleted: u64,
    pub properties_set: u64,
    pub execution_time_ms: u64,
}

/// Error information for HTTP responses
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorDto {
    /// Error code
    pub code: String,

    /// Human-readable message
    pub message: String,
}

/// Health check response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    /// Service status
    pub status: String,

    /// Version information
    pub version: String,

    /// Uptime in seconds
    pub uptime_seconds: u64,
}

impl QueryResponse {
    /// Create a success response
    pub fn success(result: QueryResultDto) -> Self {
        Self {
            success: true,
            result: Some(result),
            error: None,
        }
    }

    /// Create an error response
    pub fn error(code: &str, message: &str) -> Self {
        Self {
            success: false,
            result: None,
            error: Some(ErrorDto {
                code: code.to_string(),
                message: message.to_string(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_request_serialization() {
        let req = QueryRequest {
            statement: "MATCH (n) RETURN n".to_string(),
            parameters: Default::default(),
        };

        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("MATCH"));
    }

    #[test]
    fn test_query_response_success() {
        let result = QueryResultDto {
            columns: vec!["n".to_string()],
            data: vec![],
            stats: QueryStatsDto::default(),
        };

        let resp = QueryResponse::success(result);
        assert!(resp.success);
        assert!(resp.result.is_some());
        assert!(resp.error.is_none());
    }

    #[test]
    fn test_query_response_error() {
        let resp = QueryResponse::error("TEST", "Test error");
        assert!(!resp.success);
        assert!(resp.result.is_none());
        assert!(resp.error.is_some());
    }
}
