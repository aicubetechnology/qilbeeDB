//! QilbeeDB Query Engine
//!
//! Provides Cypher query parsing and execution.
//!
//! # Overview
//!
//! The query engine implements:
//! - Complete OpenCypher lexer and parser
//! - Cost-based query optimization
//! - Execution planning
//! - Vectorized execution
//! - Result streaming

pub mod lexer;
pub mod parser;
pub mod simple_parser;
pub mod planner;
pub mod executor;

pub use lexer::{tokenize, Token};
pub use parser::parse;
pub use simple_parser::parse_simple;
pub use planner::{QueryPlanner, ExecutionPlan, PhysicalOperator};
pub use executor::{QueryExecutor, QueryResult, ExecutionStats};

// Type alias for lexer (uses logos::Lexer)
pub type CypherLexer<'a> = logos::Lexer<'a, Token>;

// Convenience function alias
pub use parse as parse_cypher;
