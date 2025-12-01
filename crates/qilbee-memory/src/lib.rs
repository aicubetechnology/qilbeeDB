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
//! - Persistent storage with RocksDB backend
//! - Write-ahead logging for durability

pub mod agent;
pub mod consolidation;
pub mod embeddings;
pub mod episode;
pub mod llm;
pub mod storage;
pub mod types;
pub mod vector_index;

pub use agent::{
    AgentMemory, HybridSearchResult, MemoryStatistics, PersistentAgentMemory, SemanticSearchConfig,
    SemanticSearchResult,
};
pub use consolidation::{
    ConsolidationConfig, ConsolidationResult, ConsolidationService, ConsolidationStrategy,
    ExtractedFact,
};
pub use embeddings::{
    cosine_similarity, create_provider as create_embedding_provider, dot_product,
    euclidean_distance, find_top_k, normalize_vector, similarity, EmbeddingConfig, EmbeddingError,
    EmbeddingProvider, EmbeddingProviderType, EmbeddingResult, MockEmbeddingProvider,
    SimilarityMetric, SimilarityResult,
};
pub use episode::{Episode, EpisodeContent, EpisodeType};
pub use llm::{
    create_provider as create_llm_provider, ChatMessage, LLMConfig, LLMError, LLMProvider,
    LLMProviderType, LLMResponse, LLMResult, LLMService, LLMStatus, MessageRole, MockLLMProvider,
    TokenUsage,
};
pub use storage::{InMemoryStorage, MemoryStorage, MemoryStorageConfig, RocksDbMemoryStorage};
pub use types::{MemoryConfig, MemoryType};
pub use vector_index::{HnswConfig, HnswError, HnswIndex, HnswResult, SearchResult};
