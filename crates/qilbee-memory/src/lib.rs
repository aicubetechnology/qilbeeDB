//! QilbeeDB Agent Memory System
//!
//! Provides bi-temporal memory management for AI agents.
//!
//! # Memory Types
//!
//! - **Episodic Memory**: Specific events and interactions
//! - **Semantic Memory**: General knowledge and concepts
//! - **Procedural Memory**: How-to knowledge and workflows
//! - **Factual Memory**: User preferences and persistent facts
//!
//! # Features
//!
//! - Bi-temporal data model (event time + transaction time)
//! - Memory consolidation (STM -> LTM)
//! - Active forgetting with relevance decay
//! - Temporal queries and time-travel

pub mod agent;
pub mod consolidation;
pub mod episode;
pub mod types;

pub use agent::{AgentMemory, MemoryStatistics};
pub use episode::{Episode, EpisodeContent, EpisodeType};
pub use types::{MemoryConfig, MemoryType};
