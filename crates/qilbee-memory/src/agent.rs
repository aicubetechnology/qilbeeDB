//! Agent memory manager

use crate::embeddings::{
    create_provider, similarity, EmbeddingConfig, EmbeddingProvider, SimilarityMetric,
};
use crate::episode::{Episode, EpisodeId, EpisodeType};
use crate::retrieval::{KeywordSearchResult, rank_episodes};
use crate::storage::{InMemoryStorage, MemoryStorage, MemoryStorageConfig, RocksDbMemoryStorage};
use crate::types::{MemoryConfig, MemoryType, Relevance};
use crate::vector_index::{HnswConfig, HnswIndex};
use qilbee_core::temporal::{EventTime, TemporalRange};
use qilbee_core::{Error, Result};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tracing::{debug, info};

fn validate_episode_write(config: &MemoryConfig, episode: &Episode) -> Result<()> {
    if config.agent_id.is_empty() || episode.agent_id != config.agent_id {
        return Err(Error::ValidationError("Episode must belong to the configured agent".into()));
    }
    if config.max_episodes == 0 {
        return Err(Error::MemoryOperation("Episode capacity is zero".into()));
    }
    Ok(())
}

/// Compare source identity and payload without treating access accounting as
/// a content revision. NaN-bearing values fail equality conservatively.
fn same_vector_source(indexed: &Episode, current: &Episode) -> bool {
    indexed.id == current.id
        && indexed.agent_id == current.agent_id
        && indexed.episode_type == current.episode_type
        && indexed.event_time == current.event_time
        && indexed.transaction_time == current.transaction_time
        && indexed.content.primary == current.content.primary
        && indexed.content.secondary == current.content.secondary
        && indexed.content.context == current.content.context
        && indexed.content.data == current.content.data
        && indexed.content.embedding == current.content.embedding
        && indexed.metadata == current.metadata
        && indexed.consolidated == current.consolidated
        && indexed.invalidated_at == current.invalidated_at
}

/// Statistics about agent memory
#[derive(Debug, Clone)]
pub struct MemoryStatistics {
    /// Total number of episodes
    pub total_episodes: usize,

    /// Oldest episode timestamp
    pub oldest_episode: Option<i64>,

    /// Newest episode timestamp
    pub newest_episode: Option<i64>,

    /// Average relevance score
    pub avg_relevance: f64,
}

/// Agent memory manager
///
/// Provides memory operations for a single AI agent.
pub struct AgentMemory {
    /// Configuration
    config: MemoryConfig,

    /// Episodic memories (in-memory store, would be backed by graph in production)
    episodes: Arc<RwLock<HashMap<EpisodeId, Episode>>>,
}

impl AgentMemory {
    /// Create a new agent memory manager
    pub fn new(config: MemoryConfig) -> Self {
        info!("Created agent memory for '{}'", config.agent_id);
        Self {
            config,
            episodes: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create with default config for an agent
    pub fn for_agent(agent_id: &str) -> Self {
        Self::new(MemoryConfig::new(agent_id))
    }

    /// Get agent ID
    pub fn agent_id(&self) -> &str {
        &self.config.agent_id
    }

    /// Get configuration
    pub fn config(&self) -> &MemoryConfig {
        &self.config
    }

    // ========== Episodic Memory ==========

    /// Store an episode
    pub fn store_episode(&self, episode: Episode) -> Result<EpisodeId> {
        validate_episode_write(&self.config, &episode)?;
        if !self.config.enable_episodic {
            return Err(Error::MemoryOperation(
                "Episodic memory is disabled".to_string(),
            ));
        }

        let id = episode.id;

        let mut episodes = self.episodes.write().map_err(|_| {
            Error::Internal("Failed to acquire episodes lock".to_string())
        })?;

        // Check max episodes limit
        let adds_valid_episode = episode.is_valid()
            && !episodes.get(&id).is_some_and(Episode::is_valid);
        if adds_valid_episode
            && episodes.values().filter(|existing| existing.is_valid()).count() >= self.config.max_episodes {
            // Remove oldest low-relevance episode
            self.evict_low_relevance_episode(&mut episodes)?;
        }

        episodes.insert(id, episode);
        debug!("Stored episode {} for agent {}", id, self.config.agent_id);

        Ok(id)
    }

    /// Get an episode by ID
    pub fn get_episode(&self, id: EpisodeId) -> Result<Option<Episode>> {
        let mut episodes = self.episodes.write().map_err(|_| {
            Error::Internal("Failed to acquire episodes lock".to_string())
        })?;

        if let Some(episode) = episodes.get_mut(&id) {
            if !episode.is_valid() { return Ok(None); }
            episode.access()?;
            Ok(Some(episode.clone()))
        } else {
            Ok(None)
        }
    }

    /// Get episodes by type
    pub fn get_episodes_by_type(&self, episode_type: &EpisodeType) -> Result<Vec<Episode>> {
        let episodes = self.episodes.read().map_err(|_| {
            Error::Internal("Failed to acquire episodes lock".to_string())
        })?;

        Ok(episodes
            .values()
            .filter(|e| e.is_valid() && &e.episode_type == episode_type)
            .cloned()
            .collect())
    }

    /// Get episodes in a time range
    pub fn get_episodes_in_range(&self, range: &TemporalRange) -> Result<Vec<Episode>> {
        let episodes = self.episodes.read().map_err(|_| {
            Error::Internal("Failed to acquire episodes lock".to_string())
        })?;

        Ok(episodes
            .values()
            .filter(|e| e.is_valid() && range.contains(e.event_time))
            .cloned()
            .collect())
    }

    /// Get recent episodes (last N)
    pub fn get_recent_episodes(&self, limit: usize) -> Result<Vec<Episode>> {
        let episodes = self.episodes.read().map_err(|_| {
            Error::Internal("Failed to acquire episodes lock".to_string())
        })?;

        let mut valid: Vec<_> = episodes.values().filter(|e| e.is_valid()).cloned().collect();

        // Sort by event time descending
        valid.sort_by(|a, b| b.event_time.as_millis().cmp(&a.event_time.as_millis()));

        Ok(valid.into_iter().take(limit).collect())
    }

    /// Search episodes by content (simple substring match)
    pub fn search_episodes(&self, query: &str) -> Result<Vec<Episode>> {
        let query_lower = query.to_lowercase();

        let episodes = self.episodes.read().map_err(|_| {
            Error::Internal("Failed to acquire episodes lock".to_string())
        })?;

        Ok(episodes
            .values()
            .filter(|e| {
                e.is_valid()
                    && (e.content.primary.to_lowercase().contains(&query_lower)
                        || e.content
                            .secondary
                            .as_ref()
                            .map(|s| s.to_lowercase().contains(&query_lower))
                            .unwrap_or(false))
            })
            .cloned()
            .collect())
    }

    /// Invalidate an episode
    pub fn invalidate_episode(&self, id: EpisodeId) -> Result<bool> {
        let mut episodes = self.episodes.write().map_err(|_| {
            Error::Internal("Failed to acquire episodes lock".to_string())
        })?;

        if let Some(episode) = episodes.get_mut(&id) {
            episode.invalidate();
            debug!(
                "Invalidated episode {} for agent {}",
                id, self.config.agent_id
            );
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Rank valid episodes using BM25. This is a full scan, not an inverted index.
    pub fn keyword_search(&self, query: &str, limit: usize) -> Result<Vec<KeywordSearchResult>> {
        Ok(rank_episodes(self.get_all_episodes()?, query, limit))
    }

    /// Get episode count
    pub fn episode_count(&self) -> Result<usize> {
        let episodes = self.episodes.read().map_err(|_| {
            Error::Internal("Failed to acquire episodes lock".to_string())
        })?;

        Ok(episodes.values().filter(|e| e.is_valid()).count())
    }

    // ========== Memory Operations ==========

    /// Apply relevance decay to all episodes
    pub fn apply_decay(&self) -> Result<()> {
        let decay_rate = MemoryType::Episodic.default_decay_rate();

        let mut episodes = self.episodes.write().map_err(|_| {
            Error::Internal("Failed to acquire episodes lock".to_string())
        })?;

        for episode in episodes.values_mut() {
            episode.relevance.decay(decay_rate)?;
        }

        debug!(
            "Applied decay to {} episodes for agent {}",
            episodes.len(),
            self.config.agent_id
        );

        Ok(())
    }

    /// Forget low-relevance episodes
    pub fn forget(&self) -> Result<usize> {
        if !self.config.auto_forget {
            return Ok(0);
        }

        let mut episodes = self.episodes.write().map_err(|_| {
            Error::Internal("Failed to acquire episodes lock".to_string())
        })?;

        let min_relevance = self.config.min_relevance;
        let to_forget: Vec<_> = episodes
            .iter()
            .filter(|(_, e)| e.relevance.should_forget(min_relevance))
            .map(|(id, _)| *id)
            .collect();

        let count = to_forget.len();

        for id in to_forget {
            if let Some(mut episode) = episodes.remove(&id) {
                episode.invalidate();
            }
        }

        if count > 0 {
            info!(
                "Forgot {} episodes for agent {}",
                count, self.config.agent_id
            );
        }

        Ok(count)
    }

    /// Clear all episodes
    pub fn clear(&self) -> Result<()> {
        let mut episodes = self.episodes.write().map_err(|_| {
            Error::Internal("Failed to acquire episodes lock".to_string())
        })?;

        episodes.clear();
        info!("Cleared all episodes for agent {}", self.config.agent_id);

        Ok(())
    }

    /// Mark an episode as consolidated
    pub fn mark_consolidated(&self, id: EpisodeId) -> Result<bool> {
        let mut episodes = self.episodes.write().map_err(|_| {
            Error::Internal("Failed to acquire episodes lock".to_string())
        })?;

        if let Some(episode) = episodes.get_mut(&id) {
            episode.mark_consolidated();
            debug!("Marked episode {} as consolidated", id);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Get all episodes
    pub fn get_all_episodes(&self) -> Result<Vec<Episode>> {
        let episodes = self.episodes.read().map_err(|_| {
            Error::Internal("Failed to acquire episodes lock".to_string())
        })?;

        Ok(episodes
            .values()
            .filter(|e| e.is_valid())
            .cloned()
            .collect())
    }

    /// Get memory statistics
    pub fn get_statistics(&self) -> Result<MemoryStatistics> {
        let episodes = self.episodes.read().map_err(|_| {
            Error::Internal("Failed to acquire episodes lock".to_string())
        })?;

        let valid_episodes: Vec<_> = episodes.values().filter(|e| e.is_valid()).collect();
        let total_episodes = valid_episodes.len();

        let oldest_episode = valid_episodes
            .iter()
            .map(|e| e.event_time.as_millis())
            .min();

        let newest_episode = valid_episodes
            .iter()
            .map(|e| e.event_time.as_millis())
            .max();

        let avg_relevance = if total_episodes > 0 {
            valid_episodes
                .iter()
                .map(|e| e.relevance.score)
                .sum::<f64>() / total_episodes as f64
        } else {
            0.0
        };

        Ok(MemoryStatistics {
            total_episodes,
            oldest_episode,
            newest_episode,
            avg_relevance,
        })
    }

    // ========== Private Helpers ==========

    fn evict_low_relevance_episode(
        &self,
        episodes: &mut HashMap<EpisodeId, Episode>,
    ) -> Result<()> {
        // Find the episode with lowest relevance
        let lowest = episodes
            .iter()
            .filter(|(_, e)| e.is_valid())
            .min_by(|(_, a), (_, b)| {
                a.relevance
                    .score
                    .partial_cmp(&b.relevance.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(id, _)| *id);

        if let Some(id) = lowest {
            if let Some(mut episode) = episodes.remove(&id) {
                episode.invalidate();
                debug!(
                    "Evicted low-relevance episode {} for agent {}",
                    id, self.config.agent_id
                );
            }
        }

        Ok(())
    }
}

impl Clone for AgentMemory {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            episodes: Arc::clone(&self.episodes),
        }
    }
}

/// Semantic search result containing episode and similarity score
#[derive(Debug, Clone)]
pub struct SemanticSearchResult {
    /// The matched episode
    pub episode: Episode,
    /// Similarity score (0.0 to 1.0, higher is more similar)
    pub score: f32,
}

/// Native ANN candidate resolution accounting, not an exhaustive corpus scan.
#[derive(Debug, Clone)]
pub struct NativeSemanticSearchReport {
    pub results: Vec<SemanticSearchResult>,
    /// Records present in the index when candidate selection started.
    pub indexed_records: usize,
    /// ANN hits selected, before source resolution. Not graph nodes examined.
    pub candidates_selected: usize,
    pub unbound_candidates: usize,
    pub missing_sources: usize,
    pub invalid_sources: usize,
    pub mismatched_sources: usize,
    /// Always false: HNSW and post-selection validation are not exhaustive.
    pub exhaustive: bool,
}

/// Hybrid search result containing episode and combined score
#[derive(Debug, Clone)]
pub struct HybridSearchResult {
    /// The matched episode
    pub episode: Episode,
    /// Combined relevance score
    pub score: f32,
    /// Semantic similarity component (if available)
    pub semantic_score: Option<f32>,
    /// Keyword match component (if available)
    pub keyword_score: Option<f32>,
}

/// Configuration for semantic search
#[derive(Debug, Clone)]
pub struct SemanticSearchConfig {
    /// Embedding configuration
    pub embedding_config: EmbeddingConfig,
    /// HNSW index configuration
    pub hnsw_config: HnswConfig,
    /// Whether to auto-generate embeddings on store
    pub auto_embed: bool,
}

impl Default for SemanticSearchConfig {
    fn default() -> Self {
        Self {
            embedding_config: EmbeddingConfig::default(),
            hnsw_config: HnswConfig::small(),
            auto_embed: true,
        }
    }
}

impl SemanticSearchConfig {
    /// Create config with mock embeddings (for testing)
    pub fn mock(dimensions: usize) -> Self {
        Self {
            embedding_config: EmbeddingConfig::mock(dimensions),
            hnsw_config: HnswConfig::small().with_dimension(dimensions),
            auto_embed: true,
        }
    }

    /// Create config for OpenAI embeddings
    pub fn openai(api_key: &str) -> Self {
        Self {
            embedding_config: EmbeddingConfig::openai_small(api_key),
            hnsw_config: HnswConfig::medium().with_dimension(1536),
            auto_embed: true,
        }
    }
}

/// Persistent agent memory manager backed by RocksDB
///
/// This is the enterprise-grade implementation that persists episodes to disk
/// using RocksDB storage. All episodes survive server restarts.
///
/// Supports semantic search via vector embeddings and HNSW indexing.
pub struct PersistentAgentMemory {
    /// Configuration
    config: MemoryConfig,

    /// Storage backend
    storage: Arc<dyn MemoryStorage>,

    /// Embedding provider (optional, for semantic search)
    embedding_provider: Option<Arc<dyn EmbeddingProvider>>,

    /// HNSW vector index (optional, for semantic search)
    vector_index: Option<Arc<RwLock<HnswIndex>>>,

    /// Serializes native index preparation and publication across clones.
    index_mutation: Arc<tokio::sync::Mutex<()>>,
    /// Sources are read/written while holding the index lock first.
    vector_sources: Arc<RwLock<HashMap<String, Arc<Episode>>>>,

    /// Semantic search configuration
    semantic_config: Option<SemanticSearchConfig>,
}

impl PersistentAgentMemory {
    /// Create a new persistent agent memory with RocksDB storage
    pub fn new(config: MemoryConfig, storage_config: MemoryStorageConfig) -> Result<Self> {
        let storage_path = storage_config.path.clone();
        let storage = RocksDbMemoryStorage::open(storage_config).map_err(|e| {
            Error::Storage(format!("Failed to create RocksDB storage: {}", e))
        })?;

        info!(
            "Created persistent agent memory for '{}' at {}",
            config.agent_id,
            storage_path
        );

        Ok(Self {
            config,
            storage: Arc::new(storage),
            embedding_provider: None,
            vector_index: None,
            index_mutation: Arc::new(tokio::sync::Mutex::new(())),
            vector_sources: Arc::new(RwLock::new(HashMap::new())),
            semantic_config: None,
        })
    }

    /// Create with an existing storage backend
    pub fn with_storage(config: MemoryConfig, storage: Arc<dyn MemoryStorage>) -> Self {
        info!(
            "Created persistent agent memory for '{}' with custom storage",
            config.agent_id
        );
        Self {
            config,
            storage,
            embedding_provider: None,
            vector_index: None,
            index_mutation: Arc::new(tokio::sync::Mutex::new(())),
            vector_sources: Arc::new(RwLock::new(HashMap::new())),
            semantic_config: None,
        }
    }

    /// Create with in-memory storage (for testing)
    pub fn in_memory(config: MemoryConfig) -> Self {
        info!(
            "Created in-memory persistent agent memory for '{}'",
            config.agent_id
        );
        Self {
            config,
            storage: Arc::new(InMemoryStorage::new()),
            embedding_provider: None,
            vector_index: None,
            index_mutation: Arc::new(tokio::sync::Mutex::new(())),
            vector_sources: Arc::new(RwLock::new(HashMap::new())),
            semantic_config: None,
        }
    }

    /// Enable semantic search with the given configuration
    pub fn with_semantic_search(mut self, semantic_config: SemanticSearchConfig) -> Result<Self> {
        let provider = create_provider(semantic_config.embedding_config.clone())
            .map_err(|e| Error::Internal(format!("Failed to create embedding provider: {}", e)))?;

        let index = HnswIndex::try_new(semantic_config.hnsw_config.clone())
            .map_err(|e| Error::MemoryOperation(format!("Invalid vector index configuration: {}", e)))?;

        info!(
            "Enabled semantic search for agent '{}' with {} dimensions",
            self.config.agent_id,
            provider.dimensions()
        );

        self.embedding_provider = Some(provider);
        self.vector_index = Some(Arc::new(RwLock::new(index)));
        self.vector_sources = Arc::new(RwLock::new(HashMap::new()));
        self.semantic_config = Some(semantic_config);

        Ok(self)
    }

    /// Enable semantic search with mock embeddings (for testing)
    pub fn with_mock_semantic_search(self, dimensions: usize) -> Result<Self> {
        self.with_semantic_search(SemanticSearchConfig::mock(dimensions))
    }

    /// Check if semantic search is enabled
    pub fn has_semantic_search(&self) -> bool {
        self.embedding_provider.is_some() && self.vector_index.is_some()
    }

    /// Get the embedding dimensions (if semantic search is enabled)
    pub fn embedding_dimensions(&self) -> Option<usize> {
        self.embedding_provider.as_ref().map(|p| p.dimensions())
    }

    /// Get agent ID
    pub fn agent_id(&self) -> &str {
        &self.config.agent_id
    }

    /// Get configuration
    pub fn config(&self) -> &MemoryConfig {
        &self.config
    }

    /// Get reference to the storage backend
    pub fn storage(&self) -> &Arc<dyn MemoryStorage> {
        &self.storage
    }

    // ========== Episodic Memory (Async) ==========

    /// Store an episode
    pub async fn store_episode(&self, episode: Episode) -> Result<EpisodeId> {
        validate_episode_write(&self.config, &episode)?;
        if !self.config.enable_episodic {
            return Err(Error::MemoryOperation(
                "Episodic memory is disabled".to_string(),
            ));
        }

        let id = episode.id;

        // Check max episodes limit
        let count = self.storage.episode_count(&self.config.agent_id).await.map_err(|e| {
            Error::Storage(format!("Failed to get episode count: {}", e))
        })?;

        let existing = self.storage.get_episode(&self.config.agent_id, id).await?;
        let adds_valid_episode = episode.is_valid()
            && !existing.as_ref().is_some_and(Episode::is_valid);
        if adds_valid_episode && count >= self.config.max_episodes {
            // Evict oldest low-relevance episode
            self.evict_low_relevance_episode().await?;
        }

        self.storage
            .store_episode(&self.config.agent_id, &episode)
            .await
            .map_err(|e| Error::Storage(format!("Failed to store episode: {}", e)))?;

        debug!("Stored episode {} for agent {}", id, self.config.agent_id);

        Ok(id)
    }

    /// Get an episode by ID
    pub async fn get_episode(&self, id: EpisodeId) -> Result<Option<Episode>> {
        let result = self.storage.apply_relevance_change(&self.config.agent_id, id,
            crate::relevance_accounting::RelevanceChange::AccessNow {
                // Preserve the historical get_episode access boost. Direct
                // storage callers can supply their own explicit boost.
                boost: 0.1,
            }).await?;
        Ok(result.map(|update| update.episode))
    }

    /// Get episodes by type
    pub async fn get_episodes_by_type(&self, episode_type: &EpisodeType) -> Result<Vec<Episode>> {
        let all_episodes = self
            .storage
            .get_all_episodes(&self.config.agent_id)
            .await
            .map_err(|e| Error::Storage(format!("Failed to get episodes: {}", e)))?;

        Ok(all_episodes
            .into_iter()
            .filter(|e| e.is_valid() && &e.episode_type == episode_type)
            .collect())
    }

    /// Get episodes in a time range
    pub async fn get_episodes_in_range(&self, range: &TemporalRange) -> Result<Vec<Episode>> {
        let episodes = self
            .storage
            .get_episodes_in_range(
                &self.config.agent_id,
                range.start.as_millis(),
                range.end.as_millis(),
            )
            .await
            .map_err(|e| Error::Storage(format!("Failed to get episodes in range: {}", e)))?;

        Ok(episodes.into_iter().filter(|e| e.is_valid()).collect())
    }

    /// Get recent episodes (last N)
    pub async fn get_recent_episodes(&self, limit: usize) -> Result<Vec<Episode>> {
        let all_episodes = self
            .storage
            .get_all_episodes(&self.config.agent_id)
            .await
            .map_err(|e| Error::Storage(format!("Failed to get episodes: {}", e)))?;

        let mut valid: Vec<_> = all_episodes.into_iter().filter(|e| e.is_valid()).collect();

        // Sort by event time descending
        valid.sort_by(|a, b| b.event_time.as_millis().cmp(&a.event_time.as_millis()));

        Ok(valid.into_iter().take(limit).collect())
    }

    /// Search episodes by content (simple substring match)
    pub async fn search_episodes(&self, query: &str) -> Result<Vec<Episode>> {
        let query_lower = query.to_lowercase();

        let all_episodes = self
            .storage
            .get_all_episodes(&self.config.agent_id)
            .await
            .map_err(|e| Error::Storage(format!("Failed to get episodes: {}", e)))?;

        Ok(all_episodes
            .into_iter()
            .filter(|e| {
                e.is_valid()
                    && (e.content.primary.to_lowercase().contains(&query_lower)
                        || e.content
                            .secondary
                            .as_ref()
                            .map(|s| s.to_lowercase().contains(&query_lower))
                            .unwrap_or(false))
            })
            .collect())
    }

    /// Invalidate the current episode without overwriting concurrent relevance updates.
    pub async fn invalidate_episode(&self, id: EpisodeId) -> Result<bool> {
        self.storage.invalidate_current_episode(&self.config.agent_id, id).await
            .map_err(|e| Error::Storage(format!("Failed to invalidate episode: {}", e)))
    }

    /// Get episode count
    pub async fn episode_count(&self) -> Result<usize> {
        self.storage
            .episode_count(&self.config.agent_id)
            .await
            .map_err(|e| Error::Storage(format!("Failed to get episode count: {}", e)))
    }

    /// Rank valid episodes using BM25, with deterministic UUID tie breaking.
    pub async fn keyword_search(&self, query: &str, limit: usize) -> Result<Vec<KeywordSearchResult>> {
        Ok(rank_episodes(self.get_all_episodes().await?, query, limit))
    }

    // ========== Memory Operations ==========

    /// Apply relevance decay to all episodes
    pub async fn apply_decay(&self) -> Result<()> {
        let decay_rate = MemoryType::Episodic.default_decay_rate();

        let all_episodes = self
            .storage
            .get_all_episodes(&self.config.agent_id)
            .await
            .map_err(|e| Error::Storage(format!("Failed to get episodes: {}", e)))?;

        for episode in all_episodes {
            self.storage.apply_relevance_change(&self.config.agent_id, episode.id,
                crate::relevance_accounting::RelevanceChange::DecayNow {
                    rate_per_hour: decay_rate,
                }).await?;
        }

        debug!(
            "Applied decay to episodes for agent {}",
            self.config.agent_id
        );

        Ok(())
    }

    /// Forget low-relevance episodes
    pub async fn forget(&self) -> Result<usize> {
        if !self.config.auto_forget {
            return Ok(0);
        }

        let all_episodes = self
            .storage
            .get_all_episodes(&self.config.agent_id)
            .await
            .map_err(|e| Error::Storage(format!("Failed to get episodes: {}", e)))?;

        let min_relevance = self.config.min_relevance;
        let to_forget: Vec<_> = all_episodes
            .into_iter()
            .filter(|e| e.relevance.should_forget(min_relevance))
            .collect();

        let count = to_forget.len();

        for episode in to_forget {
            self.storage
                .delete_episode(&self.config.agent_id, episode.id)
                .await
                .map_err(|e| Error::Storage(format!("Failed to delete episode: {}", e)))?;
        }

        if count > 0 {
            info!(
                "Forgot {} episodes for agent {}",
                count, self.config.agent_id
            );
        }

        Ok(count)
    }

    /// Clear all episodes
    pub async fn clear(&self) -> Result<()> {
        self.storage
            .delete_all_episodes(&self.config.agent_id)
            .await
            .map_err(|e| Error::Storage(format!("Failed to clear episodes: {}", e)))?;

        info!("Cleared all episodes for agent {}", self.config.agent_id);

        Ok(())
    }

    /// Get all episodes
    pub async fn get_all_episodes(&self) -> Result<Vec<Episode>> {
        let episodes = self
            .storage
            .get_all_episodes(&self.config.agent_id)
            .await
            .map_err(|e| Error::Storage(format!("Failed to get episodes: {}", e)))?;

        Ok(episodes.into_iter().filter(|e| e.is_valid()).collect())
    }

    /// Get memory statistics
    pub async fn get_statistics(&self) -> Result<MemoryStatistics> {
        let all_episodes = self
            .storage
            .get_all_episodes(&self.config.agent_id)
            .await
            .map_err(|e| Error::Storage(format!("Failed to get episodes: {}", e)))?;

        let valid_episodes: Vec<_> = all_episodes.into_iter().filter(|e| e.is_valid()).collect();
        let total_episodes = valid_episodes.len();

        let oldest_episode = valid_episodes
            .iter()
            .map(|e| e.event_time.as_millis())
            .min();

        let newest_episode = valid_episodes
            .iter()
            .map(|e| e.event_time.as_millis())
            .max();

        let avg_relevance = if total_episodes > 0 {
            valid_episodes.iter().map(|e| e.relevance.score).sum::<f64>() / total_episodes as f64
        } else {
            0.0
        };

        Ok(MemoryStatistics {
            total_episodes,
            oldest_episode,
            newest_episode,
            avg_relevance,
        })
    }

    /// Flush storage to disk
    pub async fn flush(&self) -> Result<()> {
        self.storage
            .flush()
            .await
            .map_err(|e| Error::Storage(format!("Failed to flush storage: {}", e)))
    }

    // ========== Semantic Search Methods ==========

    /// Generate embedding for text using the configured provider
    pub async fn generate_embedding(&self, text: &str) -> Result<Vec<f32>> {
        let provider = self.embedding_provider.as_ref().ok_or_else(|| {
            Error::MemoryOperation("Semantic search is not enabled".to_string())
        })?;

        provider
            .embed(text)
            .await
            .map_err(|e| Error::Internal(format!("Failed to generate embedding: {}", e)))
    }

    /// Index an episode in the vector index
    pub async fn index_episode(&self, episode: &Episode) -> Result<()> {
        let _mutation = self.index_mutation.lock().await;
        if episode.agent_id != self.config.agent_id || !episode.is_valid() {
            return Err(Error::ValidationError("Only valid episodes owned by this agent may be indexed".into()));
        }
        let index = self.vector_index.as_ref().ok_or_else(|| {
            Error::MemoryOperation("Semantic search is not enabled".to_string())
        })?;

        // Generate embedding for the episode content
        let text = format!(
            "{} {}",
            episode.content.primary,
            episode.content.secondary.as_deref().unwrap_or("")
        );
        let embedding = self.generate_embedding(&text).await?;

        // Add to HNSW index
        let mut index_guard = index.write().map_err(|_| {
            Error::Internal("Failed to acquire vector index lock".to_string())
        })?;

        index_guard.insert(episode.id.to_string(), embedding).map_err(|e| {
            Error::Internal(format!("Failed to insert into vector index: {}", e))
        })?;

        self.vector_sources.write().map_err(|_| {
            Error::Internal("Failed to acquire vector source lock".into())
        })?.insert(episode.id.to_string(), Arc::new(episode.clone()));

        debug!(
            "Indexed episode {} for agent {}",
            episode.id, self.config.agent_id
        );

        Ok(())
    }

    /// Remove an episode from the vector index
    pub async fn unindex_episode(&self, episode_id: EpisodeId) -> Result<bool> {
        let _mutation = self.index_mutation.lock().await;
        let index = self.vector_index.as_ref().ok_or_else(|| {
            Error::MemoryOperation("Semantic search is not enabled".to_string())
        })?;

        let mut index_guard = index.write().map_err(|_| {
            Error::Internal("Failed to acquire vector index lock".to_string())
        })?;

        let removed = index_guard.remove(&episode_id.to_string()).map_err(|e| {
            Error::Internal(format!("Failed to remove from vector index: {}", e))
        })?;
        self.vector_sources.write().map_err(|_| {
            Error::Internal("Failed to acquire vector source lock".into())
        })?.remove(&episode_id.to_string());
        Ok(removed)
    }

    /// Search for semantically similar episodes using a text query
    ///
    /// Returns episodes ranked by semantic similarity to the query.
    pub async fn semantic_search(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SemanticSearchResult>> {
        // Generate embedding for the query
        let query_embedding = self.generate_embedding(query).await?;

        // Search the HNSW index
        self.search_by_embedding(&query_embedding, limit).await
    }

    /// Search for similar episodes using a pre-computed embedding vector
    pub async fn search_by_embedding(
        &self,
        embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<SemanticSearchResult>> {
        Ok(self.search_by_embedding_report(embedding, limit).await?.results)
    }

    /// Search with explicit accounting for candidates rejected at resolution.
    pub async fn search_by_embedding_report(
        &self,
        embedding: &[f32],
        limit: usize,
    ) -> Result<NativeSemanticSearchReport> {
        let index = self.vector_index.as_ref().ok_or_else(|| {
            Error::MemoryOperation("Semantic search is not enabled".to_string())
        })?;

        // Capture vector/source pairs from the same index observation, then
        // release synchronous guards before resolving authoritative storage.
        let (indexed_records, search_results) = {
            let index_guard = index.read().map_err(|_| {
                Error::Internal("Failed to acquire vector index lock".into())
            })?;
            let ranked = index_guard.search(embedding, limit).map_err(|e| {
                Error::Internal(format!("Failed to search vector index: {}", e))
            })?;
            let sources = self.vector_sources.read().map_err(|_| {
                Error::Internal("Failed to acquire vector source lock".into())
            })?;
            let pairs = ranked.into_iter().map(|hit| {
                let source = sources.get(&hit.id).map(Arc::clone);
                (hit, source)
            }).collect::<Vec<_>>();
            (index_guard.len(), pairs)
        };

        let mut report = NativeSemanticSearchReport {
            results: Vec::new(), indexed_records,
            candidates_selected: search_results.len(), unbound_candidates: 0,
            missing_sources: 0, invalid_sources: 0, mismatched_sources: 0,
            exhaustive: false,
        };
        for (result, indexed_source) in search_results {
            let Some(indexed_source) = indexed_source else {
                report.unbound_candidates += 1;
                continue;
            };
            // Parse episode ID from the stored key (it's stored as a UUID string)
            let uuid = uuid::Uuid::parse_str(&result.id).map_err(|_| {
                Error::Internal(format!("Invalid episode ID in vector index: {}", result.id))
            })?;
            let episode_id = EpisodeId::from_uuid(uuid);

            // Fetch the full episode from storage
            if let Some(episode) = self
                .storage
                .get_episode(&self.config.agent_id, episode_id)
                .await
                .map_err(|e| Error::Storage(format!("Failed to get episode: {}", e)))?
            {
                if !episode.is_valid() || episode.agent_id != self.config.agent_id {
                    report.invalid_sources += 1;
                } else if !same_vector_source(&indexed_source, &episode) {
                    report.mismatched_sources += 1;
                } else {
                    // Convert distance to similarity score
                    // For cosine distance: distance = 1 - similarity, so similarity = 1 - distance
                    let score = 1.0 - result.distance;
                    report.results.push(SemanticSearchResult { episode, score });
                }
            } else {
                report.missing_sources += 1;
            }
        }

        Ok(report)
    }

    /// Find episodes similar to a given episode
    pub async fn find_similar_episodes(
        &self,
        episode_id: EpisodeId,
        limit: usize,
    ) -> Result<Vec<SemanticSearchResult>> {
        // Get the source episode
        let episode = self
            .get_episode(episode_id)
            .await?
            .ok_or_else(|| Error::MemoryOperation("Episode not found".to_string()))?;

        // Generate embedding for the episode content
        let text = format!(
            "{} {}",
            episode.content.primary,
            episode.content.secondary.as_deref().unwrap_or("")
        );
        let embedding = self.generate_embedding(&text).await?;

        // Search for similar episodes (limit + 1 to exclude the source episode)
        let mut results = self.search_by_embedding(&embedding, limit + 1).await?;

        // Remove the source episode from results
        results.retain(|r| r.episode.id != episode_id);
        results.truncate(limit);

        Ok(results)
    }

    /// Rebuild the vector index from all stored episodes
    pub async fn rebuild_vector_index(&self) -> Result<usize> {
        let index = self.vector_index.as_ref().ok_or_else(|| {
            Error::MemoryOperation("Semantic search is not enabled".to_string())
        })?;

        let _mutation = self.index_mutation.lock().await;
        let config = index.read().map_err(|_| {
            Error::Internal("Failed to acquire vector index lock".into())
        })?.config().clone();
        let mut candidate = HnswIndex::new(config);
        let mut candidate_sources = HashMap::new();
        let episodes = self.get_all_episodes().await?;
        let mut indexed_count = 0;
        for episode in episodes {
            if episode.agent_id != self.config.agent_id || !episode.is_valid() {
                return Err(Error::ValidationError("Rebuild source must belong to this agent and be valid".into()));
            }
            let text = format!("{} {}", episode.content.primary,
                episode.content.secondary.as_deref().unwrap_or(""));
            let embedding = self.generate_embedding(&text).await?;
            candidate.insert(episode.id.to_string(), embedding).map_err(|e| {
                Error::Internal(format!("Failed to prepare rebuilt vector index: {}", e))
            })?;
            candidate_sources.insert(episode.id.to_string(), Arc::new(episode));
            indexed_count += 1;
        }
        // No await between replacement and return: cancellation before this
        // point drops only the unpublished candidate and releases the gate.
        {
            let mut published = index.write().map_err(|_| {
                Error::Internal("Failed to acquire vector index lock".into())
            })?;
            let mut sources = self.vector_sources.write().map_err(|_| {
                Error::Internal("Failed to acquire vector source lock".into())
            })?;
            *published = candidate;
            *sources = candidate_sources;
        }

        info!(
            "Rebuilt vector index for agent {}: {} episodes indexed",
            self.config.agent_id, indexed_count
        );

        Ok(indexed_count)
    }

    /// Get the number of indexed episodes in the vector index
    pub fn vector_index_size(&self) -> Result<usize> {
        let index = self.vector_index.as_ref().ok_or_else(|| {
            Error::MemoryOperation("Semantic search is not enabled".to_string())
        })?;

        let index_guard = index.read().map_err(|_| {
            Error::Internal("Failed to acquire vector index lock".to_string())
        })?;

        Ok(index_guard.len())
    }

    // ========== Hybrid Search Methods ==========

    /// Perform hybrid search combining keyword and semantic search
    ///
    /// Uses Reciprocal Rank Fusion (RRF) to merge results from both search methods.
    ///
    /// # Arguments
    /// * `query` - The search query
    /// * `limit` - Maximum number of results to return
    /// * `semantic_weight` - Weight for semantic search (0.0 to 1.0, default 0.5)
    ///
    /// # Returns
    /// Combined search results ranked by hybrid score
    pub async fn hybrid_search(
        &self,
        query: &str,
        limit: usize,
        semantic_weight: Option<f32>,
    ) -> Result<Vec<HybridSearchResult>> {
        let mut weight = semantic_weight.unwrap_or(0.5);
        if !weight.is_finite() || !(0.0..=1.0).contains(&weight) {
            return Err(Error::ValidationError("Semantic weight must be finite and in [0, 1]".into()));
        }
        if limit == 0 || query.trim().is_empty() {
            return Ok(Vec::new());
        }
        if !self.has_semantic_search() { weight = 0.0; }
        let candidates = limit.saturating_mul(2);
        let keyword_results = if weight < 1.0 {
            self.keyword_search(query, candidates).await?.into_iter().map(|result| result.episode).collect()
        } else { Vec::new() };
        let semantic_results = if weight > 0.0 {
            self.semantic_search(query, candidates).await?
        } else { Vec::new() };

        // Apply Reciprocal Rank Fusion
        self.reciprocal_rank_fusion(
            keyword_results,
            semantic_results,
            weight,
            limit,
        )
    }

    /// Merge keyword and semantic search results using Reciprocal Rank Fusion (RRF)
    fn reciprocal_rank_fusion(
        &self,
        keyword_results: Vec<Episode>,
        semantic_results: Vec<SemanticSearchResult>,
        semantic_weight: f32,
        limit: usize,
    ) -> Result<Vec<HybridSearchResult>> {
        use std::collections::HashMap;

        let keyword_weight = 1.0 - semantic_weight;
        let k = 60.0; // RRF constant

        // Create a map to track scores by episode ID
        let mut score_map: HashMap<EpisodeId, (Option<f32>, Option<f32>, Episode)> = HashMap::new();

        // Add keyword results with RRF scores
        for (rank, episode) in keyword_results.into_iter().enumerate().filter(|_| keyword_weight > 0.0) {
            let rrf_score = 1.0 / (k + rank as f32 + 1.0);
            score_map.insert(episode.id, (Some(rrf_score), None, episode));
        }

        // Add semantic results with RRF scores
        for (rank, result) in semantic_results.into_iter().enumerate().filter(|_| semantic_weight > 0.0) {
            let rrf_score = 1.0 / (k + rank as f32 + 1.0);
            if let Some((_, semantic_score, _)) = score_map.get_mut(&result.episode.id) {
                *semantic_score = Some(rrf_score);
            } else {
                score_map.insert(result.episode.id, (None, Some(rrf_score), result.episode));
            }
        }

        // Calculate combined scores and create results
        let mut results: Vec<HybridSearchResult> = score_map
            .into_iter()
            .map(|(_, (keyword_score, semantic_score, episode))| {
                let kw_contribution = keyword_score.unwrap_or(0.0) * keyword_weight;
                let sem_contribution = semantic_score.unwrap_or(0.0) * semantic_weight;
                let combined_score = kw_contribution + sem_contribution;

                HybridSearchResult {
                    episode,
                    score: combined_score,
                    semantic_score,
                    keyword_score,
                }
            })
            .collect();

        // Sort by combined score (descending)
        results.sort_by(|a, b| b.score.total_cmp(&a.score)
            .then_with(|| a.episode.id.as_uuid().cmp(&b.episode.id.as_uuid())));

        // Limit results
        results.truncate(limit);

        Ok(results)
    }

    // ========== Private Helpers ==========

    async fn evict_low_relevance_episode(&self) -> Result<()> {
        let all_episodes = self
            .storage
            .get_all_episodes(&self.config.agent_id)
            .await
            .map_err(|e| Error::Storage(format!("Failed to get episodes: {}", e)))?;

        // Find the episode with lowest relevance
        let lowest = all_episodes
            .iter()
            .filter(|e| e.is_valid())
            .min_by(|a, b| {
                a.relevance
                    .score
                    .partial_cmp(&b.relevance.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|e| e.id);

        if let Some(id) = lowest {
            self.storage
                .delete_episode(&self.config.agent_id, id)
                .await
                .map_err(|e| Error::Storage(format!("Failed to delete episode: {}", e)))?;

            debug!(
                "Evicted low-relevance episode {} for agent {}",
                id, self.config.agent_id
            );
        }

        Ok(())
    }
}

impl Clone for PersistentAgentMemory {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            storage: Arc::clone(&self.storage),
            embedding_provider: self.embedding_provider.as_ref().map(Arc::clone),
            vector_index: self.vector_index.as_ref().map(Arc::clone),
            index_mutation: Arc::clone(&self.index_mutation),
            vector_sources: Arc::clone(&self.vector_sources),
            semantic_config: self.semantic_config.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embeddings::EmbeddingProviderType;

    #[test]
    fn lifecycle_sync_rejects_foreign_writes_before_eviction() {
        let memory = AgentMemory::new(MemoryConfig::new("owner").max_episodes(1));
        let own = Episode::observation("owner", "Keep");
        memory.store_episode(own.clone()).unwrap();
        assert!(memory.store_episode(Episode::observation("other", "Foreign")).is_err());
        assert!(memory.get_episode(own.id).unwrap().is_some());
    }

    #[test]
    fn lifecycle_invalidated_records_do_not_consume_live_capacity() {
        let memory = AgentMemory::new(MemoryConfig::new("owner").max_episodes(2));
        let invalid = Episode::observation("owner", "Old");
        let keep = Episode::observation("owner", "Keep");
        memory.store_episode(invalid.clone()).unwrap();
        memory.store_episode(keep.clone()).unwrap();
        memory.invalidate_episode(invalid.id).unwrap();
        memory.store_episode(Episode::observation("owner", "New")).unwrap();
        assert_eq!(memory.episode_count().unwrap(), 2);
        assert!(memory.get_episode(keep.id).unwrap().is_some());
    }

    #[tokio::test]
    async fn lifecycle_persistent_rejects_foreign_writes_before_eviction() {
        let memory = PersistentAgentMemory::in_memory(MemoryConfig::new("owner").max_episodes(1));
        let own = Episode::observation("owner", "Keep");
        memory.store_episode(own.clone()).await.unwrap();
        assert!(memory.store_episode(Episode::observation("other", "Foreign")).await.is_err());
        assert!(memory.get_episode(own.id).await.unwrap().is_some());
    }

    #[test]
    fn lifecycle_sync_invalidation_hides_regular_reads_without_recording_access() {
        let memory = AgentMemory::for_agent("owner");
        let own = Episode::observation("owner", "Obsolete");
        memory.store_episode(own.clone()).unwrap();
        memory.invalidate_episode(own.id).unwrap();
        assert!(memory.get_episode(own.id).unwrap().is_none());
        assert_eq!(memory.episodes.read().unwrap()[&own.id].relevance.access_count, 0);
    }

    #[tokio::test]
    async fn lifecycle_persistent_invalidation_hides_regular_reads_without_recording_access() {
        let memory = PersistentAgentMemory::in_memory(MemoryConfig::new("owner"));
        let own = Episode::observation("owner", "Obsolete");
        memory.store_episode(own.clone()).await.unwrap();
        memory.invalidate_episode(own.id).await.unwrap();
        assert!(memory.get_episode(own.id).await.unwrap().is_none());
        assert_eq!(memory.storage.get_episode("owner", own.id).await.unwrap().unwrap().relevance.access_count, 0);
    }

    #[test]
    fn lifecycle_sync_update_at_capacity_preserves_other_records_and_zero_rejects() {
        let memory = AgentMemory::new(MemoryConfig::new("owner").max_episodes(2));
        let mut low = Episode::observation("owner", "Low");
        low.relevance.score = 0.1;
        let high = Episode::observation("owner", "High");
        memory.store_episode(low.clone()).unwrap();
        memory.store_episode(high.clone()).unwrap();
        memory.store_episode(high).unwrap();
        assert!(memory.get_episode(low.id).unwrap().is_some());
        let zero = AgentMemory::new(MemoryConfig::new("owner").max_episodes(0));
        assert!(zero.store_episode(low).is_err());
    }

    #[tokio::test]
    async fn lifecycle_persistent_update_at_capacity_preserves_other_records_and_zero_rejects() {
        let memory = PersistentAgentMemory::in_memory(MemoryConfig::new("owner").max_episodes(2));
        let mut low = Episode::observation("owner", "Low");
        low.relevance.score = 0.1;
        let high = Episode::observation("owner", "High");
        memory.store_episode(low.clone()).await.unwrap();
        memory.store_episode(high.clone()).await.unwrap();
        memory.store_episode(high).await.unwrap();
        assert!(memory.get_episode(low.id).await.unwrap().is_some());
        let zero = PersistentAgentMemory::in_memory(MemoryConfig::new("owner").max_episodes(0));
        assert!(zero.store_episode(low).await.is_err());
    }

    #[tokio::test]
    async fn retrieval_rejects_invalid_weights() {
        let memory = PersistentAgentMemory::in_memory(MemoryConfig::new("a"));
        for weight in [f32::NAN, f32::INFINITY, -0.1, 1.1] {
            assert!(memory.hybrid_search("query", 10, Some(weight)).await.is_err());
        }
    }

    #[tokio::test]
    async fn retrieval_zero_limit_and_zero_weight_skip_embedding_provider() {
        let mut memory = PersistentAgentMemory::in_memory(MemoryConfig::new("a"))
            .with_mock_semantic_search(8).unwrap();
        memory.store_episode(Episode::observation("a", "query terms")).await.unwrap();
        // A zero-dimension mock fails to generate embeddings. Neither operation
        // should call it when there is no semantic retrieval contribution.
        memory.embedding_provider = Some(Arc::new(crate::embeddings::MockEmbeddingProvider::new(0)));
        assert!(memory.hybrid_search("query", 0, Some(0.5)).await.unwrap().is_empty());
        let results = memory.hybrid_search("query", 10, Some(0.0)).await.unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].score > 0.0);
    }

    #[tokio::test]
    async fn retrieval_keyword_fallback_has_positive_scores_for_semantic_weight_one() {
        let memory = PersistentAgentMemory::in_memory(MemoryConfig::new("a"));
        memory.store_episode(Episode::observation("a", "query")).await.unwrap();
        let results = memory.hybrid_search("query", 10, Some(1.0)).await.unwrap();
        assert!(results[0].score > 0.0);
        assert!(results[0].semantic_score.is_none());
    }

    #[test]
    fn retrieval_fusion_excludes_zero_weight_channel_and_breaks_ties_by_id() {
        let memory = PersistentAgentMemory::in_memory(MemoryConfig::new("a"));
        let mut one = Episode::observation("a", "one");
        one.id = EpisodeId::from_uuid(uuid::Uuid::from_u128(1));
        let mut two = Episode::observation("a", "two");
        two.id = EpisodeId::from_uuid(uuid::Uuid::from_u128(2));
        let semantic = vec![SemanticSearchResult { episode: one.clone(), score: 1.0 }];
        let results = memory.reciprocal_rank_fusion(vec![two.clone()], semantic.clone(), 1.0, 10).unwrap();
        assert_eq!(results.len(), 1);
        let tied = memory.reciprocal_rank_fusion(vec![two], semantic, 0.5, 10).unwrap();
        assert_eq!(tied[0].episode.id, one.id);
    }

    // ==================== Basic AgentMemory Tests ====================

    #[test]
    fn test_agent_memory_creation() {
        let memory = AgentMemory::for_agent("test-agent");
        assert_eq!(memory.agent_id(), "test-agent");
    }

    // ==================== PersistentAgentMemory Tests ====================

    #[tokio::test]
    async fn test_persistent_memory_in_memory_backend() {
        let config = MemoryConfig::new("test-agent");
        let memory = PersistentAgentMemory::in_memory(config);
        assert_eq!(memory.agent_id(), "test-agent");
    }

    #[tokio::test]
    async fn test_persistent_memory_store_and_get() {
        let config = MemoryConfig::new("test-agent");
        let memory = PersistentAgentMemory::in_memory(config);

        let episode = Episode::conversation("test-agent", "Hello", "Hi there!");
        let id = memory.store_episode(episode).await.unwrap();

        let retrieved = memory.get_episode(id).await.unwrap().unwrap();
        assert_eq!(retrieved.content.primary, "Hello");
    }

    #[tokio::test]
    async fn test_persistent_memory_search() {
        let config = MemoryConfig::new("test-agent");
        let memory = PersistentAgentMemory::in_memory(config);

        memory
            .store_episode(Episode::conversation(
                "test-agent",
                "Tell me about machine learning",
                "Machine learning is a subset of AI",
            ))
            .await
            .unwrap();
        memory
            .store_episode(Episode::conversation(
                "test-agent",
                "What's for dinner?",
                "Pizza",
            ))
            .await
            .unwrap();

        let results = memory.search_episodes("machine").await.unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].content.primary.contains("machine"));
    }

    // ==================== Semantic Search Tests ====================

    #[tokio::test]
    async fn test_semantic_search_config_creation() {
        // Test default config
        let config = SemanticSearchConfig::default();
        assert!(config.auto_embed);
        assert_eq!(config.embedding_config.provider, EmbeddingProviderType::Mock);

        // Test mock config
        let mock_config = SemanticSearchConfig::mock(384);
        assert_eq!(mock_config.embedding_config.dimensions, 384);
    }

    #[tokio::test]
    async fn test_enable_semantic_search() {
        let config = MemoryConfig::new("test-agent");
        let memory = PersistentAgentMemory::in_memory(config)
            .with_mock_semantic_search(384)
            .unwrap();

        assert!(memory.has_semantic_search());
        assert_eq!(memory.embedding_dimensions(), Some(384));
    }

    #[tokio::test]
    async fn test_semantic_search_not_enabled_error() {
        let config = MemoryConfig::new("test-agent");
        let memory = PersistentAgentMemory::in_memory(config);

        // Should not have semantic search enabled
        assert!(!memory.has_semantic_search());
        assert_eq!(memory.embedding_dimensions(), None);

        // Should return error when trying to do semantic search
        let result = memory.semantic_search("test query", 10).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_generate_embedding() {
        let config = MemoryConfig::new("test-agent");
        let memory = PersistentAgentMemory::in_memory(config)
            .with_mock_semantic_search(384)
            .unwrap();

        let embedding = memory.generate_embedding("Hello world").await.unwrap();
        assert_eq!(embedding.len(), 384);

        // Same text should produce same embedding (deterministic mock)
        let embedding2 = memory.generate_embedding("Hello world").await.unwrap();
        assert_eq!(embedding, embedding2);

        // Different text should produce different embedding
        let embedding3 = memory.generate_embedding("Goodbye world").await.unwrap();
        assert_ne!(embedding, embedding3);
    }

    #[tokio::test]
    async fn test_index_episode() {
        let config = MemoryConfig::new("test-agent");
        let memory = PersistentAgentMemory::in_memory(config)
            .with_mock_semantic_search(384)
            .unwrap();

        let episode = Episode::conversation("test-agent", "Hello", "Hi there!");
        memory.store_episode(episode.clone()).await.unwrap();

        // Index the episode
        memory.index_episode(&episode).await.unwrap();

        // Verify index size increased
        assert_eq!(memory.vector_index_size().unwrap(), 1);
    }

    #[tokio::test]
    async fn test_semantic_search_basic() {
        let config = MemoryConfig::new("test-agent");
        let memory = PersistentAgentMemory::in_memory(config)
            .with_mock_semantic_search(384)
            .unwrap();

        // Store and index episodes
        let ep1 = Episode::conversation(
            "test-agent",
            "Tell me about machine learning",
            "ML is a subset of artificial intelligence",
        );
        let ep2 = Episode::conversation(
            "test-agent",
            "What's for dinner tonight?",
            "I'm thinking pizza",
        );
        let ep3 = Episode::conversation(
            "test-agent",
            "How does neural network work?",
            "Neural networks are layers of nodes",
        );

        memory.store_episode(ep1.clone()).await.unwrap();
        memory.store_episode(ep2.clone()).await.unwrap();
        memory.store_episode(ep3.clone()).await.unwrap();

        // Index all episodes
        memory.index_episode(&ep1).await.unwrap();
        memory.index_episode(&ep2).await.unwrap();
        memory.index_episode(&ep3).await.unwrap();

        // Search for AI-related content
        let results = memory.semantic_search("artificial intelligence", 3).await.unwrap();
        assert!(!results.is_empty());
        assert!(results.len() <= 3);
    }

    #[tokio::test]
    async fn test_search_by_embedding() {
        let config = MemoryConfig::new("test-agent");
        let memory = PersistentAgentMemory::in_memory(config)
            .with_mock_semantic_search(384)
            .unwrap();

        // Store and index episode
        let episode = Episode::conversation("test-agent", "Hello world", "Greeting!");
        memory.store_episode(episode.clone()).await.unwrap();
        memory.index_episode(&episode).await.unwrap();

        // Generate embedding for search
        let query_embedding = memory.generate_embedding("Hello world").await.unwrap();

        // Search by embedding
        let results = memory.search_by_embedding(&query_embedding, 5).await.unwrap();
        assert!(!results.is_empty());
        // Verify we found the episode (score can be any value depending on distance metric)
        assert_eq!(results[0].episode.content.primary, "Hello world");
    }

    #[tokio::test]
    async fn test_find_similar_episodes() {
        let config = MemoryConfig::new("test-agent");
        let memory = PersistentAgentMemory::in_memory(config)
            .with_mock_semantic_search(384)
            .unwrap();

        // Store multiple episodes
        let ep1 = Episode::conversation("test-agent", "Python programming", "Python is great");
        let ep2 = Episode::conversation("test-agent", "Rust programming", "Rust is fast");
        let ep3 = Episode::conversation("test-agent", "Weather today", "It's sunny");

        let id1 = memory.store_episode(ep1.clone()).await.unwrap();
        memory.store_episode(ep2.clone()).await.unwrap();
        memory.store_episode(ep3.clone()).await.unwrap();

        // Index all episodes
        memory.index_episode(&ep1).await.unwrap();
        memory.index_episode(&ep2).await.unwrap();
        memory.index_episode(&ep3).await.unwrap();

        // Find episodes similar to the first one
        let similar = memory.find_similar_episodes(id1, 5).await.unwrap();

        // Should not include the source episode
        for result in &similar {
            assert_ne!(result.episode.id, id1);
        }
    }

    // ==================== Hybrid Search Tests ====================

    #[tokio::test]
    async fn test_hybrid_search_with_semantic() {
        let config = MemoryConfig::new("test-agent");
        let memory = PersistentAgentMemory::in_memory(config)
            .with_mock_semantic_search(384)
            .unwrap();

        // Store and index episodes
        let ep1 = Episode::conversation(
            "test-agent",
            "machine learning basics",
            "ML is a subset of AI",
        );
        let ep2 = Episode::conversation(
            "test-agent",
            "deep learning neural networks",
            "DL uses many layers",
        );
        let ep3 = Episode::conversation("test-agent", "cooking recipes", "Pizza is delicious");

        memory.store_episode(ep1.clone()).await.unwrap();
        memory.store_episode(ep2.clone()).await.unwrap();
        memory.store_episode(ep3.clone()).await.unwrap();

        // Index all episodes
        memory.index_episode(&ep1).await.unwrap();
        memory.index_episode(&ep2).await.unwrap();
        memory.index_episode(&ep3).await.unwrap();

        // Hybrid search with balanced weights
        let results = memory
            .hybrid_search("machine learning", 10, Some(0.5))
            .await
            .unwrap();

        assert!(!results.is_empty());

        // Results should have both keyword and semantic scores
        for result in &results {
            // At least one score should be present
            assert!(result.keyword_score.is_some() || result.semantic_score.is_some());
        }
    }

    #[tokio::test]
    async fn test_hybrid_search_without_semantic() {
        let config = MemoryConfig::new("test-agent");
        let memory = PersistentAgentMemory::in_memory(config);

        // Store episodes without semantic search
        let ep1 = Episode::conversation("test-agent", "machine learning", "ML is AI");
        let ep2 = Episode::conversation("test-agent", "cooking", "Food is good");

        memory.store_episode(ep1).await.unwrap();
        memory.store_episode(ep2).await.unwrap();

        // Hybrid search falls back to keyword-only
        let results = memory
            .hybrid_search("machine", 10, Some(0.5))
            .await
            .unwrap();

        // Should still return results from keyword search
        for result in &results {
            assert!(result.keyword_score.is_some());
            assert!(result.semantic_score.is_none()); // No semantic search enabled
        }
    }

    #[tokio::test]
    async fn test_hybrid_search_weight_variants() {
        let config = MemoryConfig::new("test-agent");
        let memory = PersistentAgentMemory::in_memory(config)
            .with_mock_semantic_search(384)
            .unwrap();

        let ep = Episode::conversation("test-agent", "test query", "response");
        memory.store_episode(ep.clone()).await.unwrap();
        memory.index_episode(&ep).await.unwrap();

        // Test keyword-only (semantic_weight = 0.0)
        let keyword_only = memory.hybrid_search("test", 10, Some(0.0)).await.unwrap();
        assert!(!keyword_only.is_empty());

        // Test semantic-only (semantic_weight = 1.0)
        let semantic_only = memory.hybrid_search("test", 10, Some(1.0)).await.unwrap();
        assert!(!semantic_only.is_empty());

        // Test default (semantic_weight = None -> 0.5)
        let default_weight = memory.hybrid_search("test", 10, None).await.unwrap();
        assert!(!default_weight.is_empty());
    }

    // ==================== Vector Index Management Tests ====================

    #[tokio::test]
    async fn test_rebuild_vector_index() {
        let config = MemoryConfig::new("test-agent");
        let memory = PersistentAgentMemory::in_memory(config)
            .with_mock_semantic_search(384)
            .unwrap();

        // Store episodes without indexing
        for i in 0..5 {
            memory
                .store_episode(Episode::conversation(
                    "test-agent",
                    &format!("Message {}", i),
                    &format!("Response {}", i),
                ))
                .await
                .unwrap();
        }

        // Index should be empty
        assert_eq!(memory.vector_index_size().unwrap(), 0);

        // Rebuild index
        let indexed = memory.rebuild_vector_index().await.unwrap();
        assert_eq!(indexed, 5);
        assert_eq!(memory.vector_index_size().unwrap(), 5);
    }

    #[tokio::test]
    async fn test_unindex_episode() {
        let config = MemoryConfig::new("test-agent");
        let memory = PersistentAgentMemory::in_memory(config)
            .with_mock_semantic_search(384)
            .unwrap();

        let episode = Episode::conversation("test-agent", "Test", "Response");
        memory.store_episode(episode.clone()).await.unwrap();
        memory.index_episode(&episode).await.unwrap();

        assert_eq!(memory.vector_index_size().unwrap(), 1);

        // Remove from index
        let removed = memory.unindex_episode(episode.id).await.unwrap();
        assert!(removed);
        assert_eq!(memory.vector_index_size().unwrap(), 0);
    }

    #[test]
    fn test_store_and_retrieve_episode() {
        let memory = AgentMemory::for_agent("test-agent");

        let episode = Episode::conversation("test-agent", "Hello", "Hi there!");
        let id = memory.store_episode(episode).unwrap();

        let retrieved = memory.get_episode(id).unwrap().unwrap();
        assert_eq!(retrieved.content.primary, "Hello");
    }

    #[test]
    fn test_get_episodes_by_type() {
        let memory = AgentMemory::for_agent("test-agent");

        memory
            .store_episode(Episode::conversation("test-agent", "Hello", "Hi"))
            .unwrap();
        memory
            .store_episode(Episode::conversation("test-agent", "Bye", "Goodbye"))
            .unwrap();
        memory
            .store_episode(Episode::observation("test-agent", "Event"))
            .unwrap();

        let conversations = memory
            .get_episodes_by_type(&EpisodeType::Conversation)
            .unwrap();
        assert_eq!(conversations.len(), 2);

        let observations = memory
            .get_episodes_by_type(&EpisodeType::Observation)
            .unwrap();
        assert_eq!(observations.len(), 1);
    }

    #[test]
    fn test_recent_episodes() {
        let memory = AgentMemory::for_agent("test-agent");

        for i in 0..5 {
            memory
                .store_episode(Episode::observation(
                    "test-agent",
                    &format!("Event {}", i),
                ))
                .unwrap();
        }

        let recent = memory.get_recent_episodes(3).unwrap();
        assert_eq!(recent.len(), 3);
    }

    #[test]
    fn test_search_episodes() {
        let memory = AgentMemory::for_agent("test-agent");

        memory
            .store_episode(Episode::conversation(
                "test-agent",
                "Tell me about weather",
                "It's sunny",
            ))
            .unwrap();
        memory
            .store_episode(Episode::conversation(
                "test-agent",
                "What's for lunch?",
                "Pizza",
            ))
            .unwrap();

        let results = memory.search_episodes("weather").unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].content.primary.contains("weather"));
    }

    #[test]
    fn test_invalidate_episode() {
        let memory = AgentMemory::for_agent("test-agent");

        let episode = Episode::observation("test-agent", "Event");
        let id = memory.store_episode(episode).unwrap();

        assert!(memory.invalidate_episode(id).unwrap());

        // Ordinary reads must not serve invalidated knowledge.
        assert!(memory.get_episode(id).unwrap().is_none());
        assert!(!memory.episodes.read().unwrap()[&id].is_valid());
    }

    #[test]
    fn test_episode_count() {
        let memory = AgentMemory::for_agent("test-agent");

        assert_eq!(memory.episode_count().unwrap(), 0);

        memory
            .store_episode(Episode::observation("test-agent", "Event 1"))
            .unwrap();
        memory
            .store_episode(Episode::observation("test-agent", "Event 2"))
            .unwrap();

        assert_eq!(memory.episode_count().unwrap(), 2);
    }

    #[test]
    fn test_clear() {
        let memory = AgentMemory::for_agent("test-agent");

        memory
            .store_episode(Episode::observation("test-agent", "Event"))
            .unwrap();
        assert_eq!(memory.episode_count().unwrap(), 1);

        memory.clear().unwrap();
        assert_eq!(memory.episode_count().unwrap(), 0);
    }
    struct RebuildFailureProvider;

    #[async_trait::async_trait]
    impl EmbeddingProvider for RebuildFailureProvider {
        fn dimensions(&self) -> usize { 2 }
        fn model_name(&self) -> &str { "controlled-rebuild-failure" }
        async fn embed(&self, _: &str) -> crate::embeddings::EmbeddingResult<Vec<f32>> {
            Err(crate::embeddings::EmbeddingError::Network("controlled failure".into()))
        }
        async fn embed_batch(&self, _: &[String]) -> crate::embeddings::EmbeddingResult<Vec<Vec<f32>>> {
            Err(crate::embeddings::EmbeddingError::Network("controlled failure".into()))
        }
    }

    #[tokio::test]
    async fn rebuild_failure_reports_error_and_preserves_live_index() {
        let mut memory = PersistentAgentMemory::in_memory(MemoryConfig::new("rebuild-agent"))
            .with_mock_semantic_search(2).unwrap();
        let episode = Episode::conversation("rebuild-agent", "retained source", "retained answer");
        memory.store_episode(episode.clone()).await.unwrap();
        memory.index_episode(&episode).await.unwrap();
        memory.embedding_provider = Some(Arc::new(RebuildFailureProvider));
        let result = memory.rebuild_vector_index().await;
        assert!(result.is_err(), "A failed rebuild must not report successful partial coverage");
        assert_eq!(memory.vector_index_size().unwrap(), 1);
    }

    struct PausedRebuildProvider {
        entered: Arc<tokio::sync::Notify>,
    }

    #[async_trait::async_trait]
    impl EmbeddingProvider for PausedRebuildProvider {
        fn dimensions(&self) -> usize { 2 }
        fn model_name(&self) -> &str { "controlled-paused-rebuild" }
        async fn embed(&self, _: &str) -> crate::embeddings::EmbeddingResult<Vec<f32>> {
            self.entered.notify_one();
            std::future::pending().await
        }
        async fn embed_batch(&self, _: &[String]) -> crate::embeddings::EmbeddingResult<Vec<Vec<f32>>> {
            unreachable!("Rebuild uses individual embedding requests")
        }
    }

    #[tokio::test]
    async fn cancelled_rebuild_preserves_published_index() {
        let mut memory = PersistentAgentMemory::in_memory(MemoryConfig::new("cancel-agent"))
            .with_mock_semantic_search(2).unwrap();
        let episode = Episode::conversation("cancel-agent", "retained source", "retained answer");
        memory.store_episode(episode.clone()).await.unwrap();
        memory.index_episode(&episode).await.unwrap();
        let entered = Arc::new(tokio::sync::Notify::new());
        memory.embedding_provider = Some(Arc::new(PausedRebuildProvider { entered: entered.clone() }));
        let previous_source = Arc::clone(&memory.vector_sources.read().unwrap()[&episode.id.to_string()]);
        let mut rebuild = Box::pin(memory.rebuild_vector_index());
        tokio::select! {
            result = &mut rebuild => panic!("Rebuild unexpectedly completed: {result:?}"),
            _ = entered.notified() => {},
        }
        drop(rebuild);
        assert!(Arc::ptr_eq(&previous_source, &memory.vector_sources.read().unwrap()[&episode.id.to_string()]));
        assert_eq!(memory.vector_index_size().unwrap(), 1, "Cancellation must retain the published index");
        assert!(tokio::time::timeout(std::time::Duration::from_secs(2),
            memory.unindex_episode(episode.id)).await.unwrap().unwrap());
    }

    struct ControlledRebuildProvider {
        entered: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Semaphore>,
    }

    #[async_trait::async_trait]
    impl EmbeddingProvider for ControlledRebuildProvider {
        fn dimensions(&self) -> usize { 2 }
        fn model_name(&self) -> &str { "controlled-rebuild" }
        async fn embed(&self, _: &str) -> crate::embeddings::EmbeddingResult<Vec<f32>> {
            self.entered.notify_one();
            self.release.acquire().await.unwrap().forget();
            Ok(vec![1.0, 0.0])
        }
        async fn embed_batch(&self, _: &[String]) -> crate::embeddings::EmbeddingResult<Vec<Vec<f32>>> {
            unreachable!()
        }
    }

    #[tokio::test]
    async fn rebuild_keeps_readers_available_and_orders_clone_removal_after_publish() {
        let mut memory = PersistentAgentMemory::in_memory(MemoryConfig::new("concurrent-agent"))
            .with_mock_semantic_search(2).unwrap();
        let episode = Episode::conversation("concurrent-agent", "retained source", "retained answer");
        memory.store_episode(episode.clone()).await.unwrap();
        memory.index_episode(&episode).await.unwrap();
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Semaphore::new(0));
        memory.embedding_provider = Some(Arc::new(ControlledRebuildProvider {
            entered: entered.clone(), release: release.clone(),
        }));
        let builder = memory.clone();
        let rebuild = tokio::spawn(async move { builder.rebuild_vector_index().await });
        tokio::time::timeout(std::time::Duration::from_secs(2), entered.notified()).await.unwrap();
        let reader = memory.clone();
        let results = tokio::spawn(async move { reader.search_by_embedding(&[1.0, 0.0], 1).await })
            .await.unwrap().unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].episode.id, episode.id);
        let remover = memory.clone();
        let mut removal = Box::pin(remover.unindex_episode(episode.id));
        assert!(tokio::time::timeout(std::time::Duration::from_millis(20), &mut removal).await.is_err());
        release.add_permits(1);
        assert_eq!(rebuild.await.unwrap().unwrap(), 1);
        assert!(removal.await.unwrap());
        assert_eq!(memory.vector_index_size().unwrap(), 0);
    }

    struct PartialInvalidRebuildProvider(std::sync::atomic::AtomicUsize);

    #[async_trait::async_trait]
    impl EmbeddingProvider for PartialInvalidRebuildProvider {
        fn dimensions(&self) -> usize { 2 }
        fn model_name(&self) -> &str { "controlled-partial-rebuild" }
        async fn embed(&self, _: &str) -> crate::embeddings::EmbeddingResult<Vec<f32>> {
            let call = self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(if call < 3 { vec![1.0, 0.0] } else { vec![1.0, 0.0, 0.0] })
        }
        async fn embed_batch(&self, _: &[String]) -> crate::embeddings::EmbeddingResult<Vec<Vec<f32>>> {
            unreachable!()
        }
    }

    #[tokio::test]
    async fn invalid_later_embedding_never_publishes_partial_rebuild() {
        let mut memory = PersistentAgentMemory::in_memory(MemoryConfig::new("partial-agent"))
            .with_mock_semantic_search(2).unwrap();
        for text in ["first source", "second source", "third source", "fourth source"] {
            let episode = Episode::conversation("partial-agent", text, "answer");
            memory.store_episode(episode.clone()).await.unwrap();
            memory.index_episode(&episode).await.unwrap();
        }
        let source_before = memory.vector_sources.read().unwrap().clone();
        let before = memory.vector_index.as_ref().unwrap().read().unwrap().to_bytes().unwrap();
        let provider = Arc::new(PartialInvalidRebuildProvider(std::sync::atomic::AtomicUsize::new(0)));
        memory.embedding_provider = Some(provider.clone());
        assert!(memory.rebuild_vector_index().await.is_err());
        assert_eq!(provider.0.load(std::sync::atomic::Ordering::SeqCst), 4);
        let after = memory.vector_index.as_ref().unwrap().read().unwrap().to_bytes().unwrap();
        assert_eq!(before, after);
        let source_after = memory.vector_sources.read().unwrap();
        assert_eq!(source_before.len(), source_after.len());
        for (id, source) in source_before {
            assert!(Arc::ptr_eq(&source, &source_after[&id]));
        }
    }

    #[tokio::test]
    async fn rebuild_serializes_with_clone_insert_and_preserves_its_result() {
        let mut memory = PersistentAgentMemory::in_memory(MemoryConfig::new("insert-agent"))
            .with_mock_semantic_search(2).unwrap();
        let old = Episode::conversation("insert-agent", "old source", "answer");
        memory.store_episode(old.clone()).await.unwrap();
        memory.index_episode(&old).await.unwrap();
        let inserter = memory.clone();
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Semaphore::new(0));
        memory.embedding_provider = Some(Arc::new(ControlledRebuildProvider {
            entered: entered.clone(), release: release.clone(),
        }));
        let builder = memory.clone();
        let rebuild = tokio::spawn(async move { builder.rebuild_vector_index().await });
        tokio::time::timeout(std::time::Duration::from_secs(2), entered.notified()).await.unwrap();
        let new = Episode::conversation("insert-agent", "new source", "answer");
        inserter.store_episode(new.clone()).await.unwrap();
        let mut insert = Box::pin(inserter.index_episode(&new));
        assert!(tokio::time::timeout(std::time::Duration::from_millis(20), &mut insert).await.is_err());
        release.add_permits(1);
        assert_eq!(rebuild.await.unwrap().unwrap(), 1);
        insert.await.unwrap();
        assert_eq!(memory.vector_index_size().unwrap(), 2);
        assert!(memory.vector_index.as_ref().unwrap().read().unwrap().get(&new.id.to_string()).is_some());
    }

    #[tokio::test]
    async fn competing_rebuilds_serialize_preparation_and_allow_empty_publication() {
        let mut memory = PersistentAgentMemory::in_memory(MemoryConfig::new("two-builders"))
            .with_mock_semantic_search(2).unwrap();
        let old = Episode::conversation("two-builders", "source", "answer");
        memory.store_episode(old.clone()).await.unwrap();
        memory.index_episode(&old).await.unwrap();
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Semaphore::new(0));
        memory.embedding_provider = Some(Arc::new(ControlledRebuildProvider {
            entered: entered.clone(), release: release.clone(),
        }));
        let builder = memory.clone();
        let first = tokio::spawn(async move { builder.rebuild_vector_index().await });
        tokio::time::timeout(std::time::Duration::from_secs(2), entered.notified()).await.unwrap();
        let second_builder = memory.clone();
        let mut second = Box::pin(second_builder.rebuild_vector_index());
        assert!(tokio::time::timeout(std::time::Duration::from_millis(20), &mut second).await.is_err());
        // Change the source while first preparation is paused. Its count refers
        // to its loaded inputs; the queued rebuild must load the newer source.
        memory.clear().await.unwrap();
        release.add_permits(1);
        assert_eq!(first.await.unwrap().unwrap(), 1);
        assert_eq!(second.await.unwrap(), 0);
        assert_eq!(memory.vector_index_size().unwrap(), 0);
    }

    #[tokio::test]
    async fn native_search_does_not_attach_old_vector_score_to_replaced_content() {
        let memory = PersistentAgentMemory::in_memory(MemoryConfig::new("source-agent"))
            .with_mock_semantic_search(8).unwrap();
        let mut episode = Episode::conversation("source-agent", "old source", "old answer");
        memory.store_episode(episode.clone()).await.unwrap();
        memory.index_episode(&episode).await.unwrap();
        let query = memory.generate_embedding("old source old answer").await.unwrap();
        assert_eq!(memory.search_by_embedding(&query, 1).await.unwrap().len(), 1);
        episode.content.primary = "unrelated replacement".into();
        memory.storage().update_episode("source-agent", &episode).await.unwrap();
        assert!(memory.search_by_embedding(&query, 1).await.unwrap().is_empty(),
            "A vector from old content must not rank the replacement as its source");
    }

    #[tokio::test]
    async fn native_index_rejects_foreign_agent_episode_before_insertion() {
        let memory = PersistentAgentMemory::in_memory(MemoryConfig::new("owner-agent"))
            .with_mock_semantic_search(8).unwrap();
        let foreign = Episode::conversation("other-agent", "foreign source", "answer");
        assert!(memory.index_episode(&foreign).await.is_err());
        assert_eq!(memory.vector_index_size().unwrap(), 0);
    }

    #[tokio::test]
    async fn native_source_binding_tracks_provenance_but_not_access_counters() {
        let memory = PersistentAgentMemory::in_memory(MemoryConfig::new("binding-agent"))
            .with_mock_semantic_search(8).unwrap();
        let original = Episode::conversation("binding-agent", "source", "answer");
        memory.store_episode(original.clone()).await.unwrap();
        memory.index_episode(&original).await.unwrap();
        let query = memory.generate_embedding("source answer").await.unwrap();
        memory.get_episode(original.id).await.unwrap();
        assert_eq!(memory.search_by_embedding(&query, 1).await.unwrap().len(), 1);
        for case in 0..5 {
            let mut changed = original.clone();
            match case {
                0 => changed.content.secondary = Some("new answer".into()),
                1 => changed.content.context = Some("new context".into()),
                2 => changed.set_metadata("origin", "another source"),
                3 => changed.consolidated = true,
                _ => changed.content.data = Some(serde_json::json!({"origin": "changed"})),
            }
            memory.storage().update_episode("binding-agent", &changed).await.unwrap();
            assert!(memory.search_by_embedding(&query, 1).await.unwrap().is_empty(), "case {case}");
            memory.index_episode(&changed).await.unwrap();
            assert_eq!(memory.search_by_embedding(&query, 1).await.unwrap().len(), 1);
            memory.storage().update_episode("binding-agent", &original).await.unwrap();
            memory.index_episode(&original).await.unwrap();
        }
        let mut invalid = original.clone();
        invalid.invalidate();
        assert!(memory.index_episode(&invalid).await.is_err());
        memory.storage().update_episode("binding-agent", &invalid).await.unwrap();
        assert!(memory.search_by_embedding(&query, 1).await.unwrap().is_empty());
        memory.storage().delete_episode("binding-agent", original.id).await.unwrap();
        assert!(memory.search_by_embedding(&query, 1).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn native_source_report_accounts_for_every_selected_candidate() {
        let memory = PersistentAgentMemory::in_memory(MemoryConfig::new("report-agent"))
            .with_mock_semantic_search(8).unwrap();
        let mut episodes = Vec::new();
        for _ in 0..5 {
            let ep = Episode::conversation("report-agent", "same source", "same answer");
            memory.store_episode(ep.clone()).await.unwrap();
            memory.index_episode(&ep).await.unwrap();
            episodes.push(ep);
        }
        memory.storage().delete_episode("report-agent", episodes[1].id).await.unwrap();
        episodes[2].invalidate();
        memory.storage().update_episode("report-agent", &episodes[2]).await.unwrap();
        episodes[3].content.primary = "changed".into();
        memory.storage().update_episode("report-agent", &episodes[3]).await.unwrap();
        memory.vector_sources.write().unwrap().remove(&episodes[4].id.to_string());
        let query = memory.generate_embedding("same source same answer").await.unwrap();
        let report = memory.search_by_embedding_report(&query, 5).await.unwrap();
        assert_eq!(report.indexed_records, 5);
        assert_eq!(report.candidates_selected, 5);
        assert_eq!(report.results.len(), 1);
        assert_eq!(report.results[0].episode.id, episodes[0].id);
        assert_eq!(report.unbound_candidates, 1);
        assert_eq!(report.missing_sources, 1);
        assert_eq!(report.invalid_sources, 1);
        assert_eq!(report.mismatched_sources, 1);
        assert!(!report.exhaustive);
        assert_eq!(report.candidates_selected, report.results.len() + report.unbound_candidates
            + report.missing_sources + report.invalid_sources + report.mismatched_sources);
    }

    #[tokio::test]
    async fn native_rebuild_binds_loaded_source_without_claiming_publication_snapshot() {
        let mut memory = PersistentAgentMemory::in_memory(MemoryConfig::new("racing-source"))
            .with_mock_semantic_search(2).unwrap();
        let mut episode = Episode::conversation("racing-source", "original", "answer");
        memory.store_episode(episode.clone()).await.unwrap();
        memory.index_episode(&episode).await.unwrap();
        let updater = memory.clone();
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Semaphore::new(0));
        memory.embedding_provider = Some(Arc::new(ControlledRebuildProvider {
            entered: entered.clone(), release: release.clone(),
        }));
        let builder = memory.clone();
        let rebuild = tokio::spawn(async move { builder.rebuild_vector_index().await });
        tokio::time::timeout(std::time::Duration::from_secs(2), entered.notified()).await.unwrap();
        episode.content.primary = "changed during preparation".into();
        updater.storage().update_episode("racing-source", &episode).await.unwrap();
        release.add_permits(1);
        assert_eq!(rebuild.await.unwrap().unwrap(), 1);
        let report = memory.search_by_embedding_report(&[1.0, 0.0], 1).await.unwrap();
        assert_eq!(report.candidates_selected, 1);
        assert_eq!(report.mismatched_sources, 1);
        assert_eq!(report.unbound_candidates, 0);
        assert!(report.results.is_empty());
        updater.index_episode(&episode).await.unwrap();
        let report = memory.search_by_embedding_report(&[1.0, 0.0], 1).await.unwrap();
        assert_eq!(report.results.len(), 1);
        assert_eq!(report.mismatched_sources, 0);
    }

}
