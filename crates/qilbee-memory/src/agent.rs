//! Agent memory manager

use crate::embeddings::{
    create_provider, similarity, EmbeddingConfig, EmbeddingProvider, SimilarityMetric,
};
use crate::episode::{Episode, EpisodeId, EpisodeType};
use crate::storage::{InMemoryStorage, MemoryStorage, MemoryStorageConfig, RocksDbMemoryStorage};
use crate::types::{MemoryConfig, MemoryType, Relevance};
use crate::vector_index::{HnswConfig, HnswIndex};
use qilbee_core::temporal::{EventTime, TemporalRange};
use qilbee_core::{Error, Result};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tracing::{debug, info, warn};

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
        if episodes.len() >= self.config.max_episodes {
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
            episode.access();
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
            episode.relevance.decay(decay_rate);
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
            semantic_config: None,
        }
    }

    /// Enable semantic search with the given configuration
    pub fn with_semantic_search(mut self, semantic_config: SemanticSearchConfig) -> Result<Self> {
        let provider = create_provider(semantic_config.embedding_config.clone())
            .map_err(|e| Error::Internal(format!("Failed to create embedding provider: {}", e)))?;

        let index = HnswIndex::new(semantic_config.hnsw_config.clone());

        info!(
            "Enabled semantic search for agent '{}' with {} dimensions",
            self.config.agent_id,
            provider.dimensions()
        );

        self.embedding_provider = Some(provider);
        self.vector_index = Some(Arc::new(RwLock::new(index)));
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

        if count >= self.config.max_episodes {
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
        let mut episode = self
            .storage
            .get_episode(&self.config.agent_id, id)
            .await
            .map_err(|e| Error::Storage(format!("Failed to get episode: {}", e)))?;

        if let Some(ref mut ep) = episode {
            ep.access();
            self.storage
                .update_episode(&self.config.agent_id, ep)
                .await
                .map_err(|e| Error::Storage(format!("Failed to update episode access time: {}", e)))?;
        }

        Ok(episode)
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

    /// Invalidate an episode
    pub async fn invalidate_episode(&self, id: EpisodeId) -> Result<bool> {
        let episode = self
            .storage
            .get_episode(&self.config.agent_id, id)
            .await
            .map_err(|e| Error::Storage(format!("Failed to get episode: {}", e)))?;

        if let Some(mut ep) = episode {
            ep.invalidate();
            self.storage
                .update_episode(&self.config.agent_id, &ep)
                .await
                .map_err(|e| Error::Storage(format!("Failed to update episode: {}", e)))?;

            debug!(
                "Invalidated episode {} for agent {}",
                id, self.config.agent_id
            );
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Get episode count
    pub async fn episode_count(&self) -> Result<usize> {
        self.storage
            .episode_count(&self.config.agent_id)
            .await
            .map_err(|e| Error::Storage(format!("Failed to get episode count: {}", e)))
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

        for mut episode in all_episodes {
            episode.relevance.decay(decay_rate);
            self.storage
                .update_episode(&self.config.agent_id, &episode)
                .await
                .map_err(|e| Error::Storage(format!("Failed to update episode: {}", e)))?;
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

        debug!(
            "Indexed episode {} for agent {}",
            episode.id, self.config.agent_id
        );

        Ok(())
    }

    /// Remove an episode from the vector index
    pub async fn unindex_episode(&self, episode_id: EpisodeId) -> Result<bool> {
        let index = self.vector_index.as_ref().ok_or_else(|| {
            Error::MemoryOperation("Semantic search is not enabled".to_string())
        })?;

        let mut index_guard = index.write().map_err(|_| {
            Error::Internal("Failed to acquire vector index lock".to_string())
        })?;

        index_guard.remove(&episode_id.to_string()).map_err(|e| {
            Error::Internal(format!("Failed to remove from vector index: {}", e))
        })
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
        let index = self.vector_index.as_ref().ok_or_else(|| {
            Error::MemoryOperation("Semantic search is not enabled".to_string())
        })?;

        // Search the HNSW index
        let index_guard = index.read().map_err(|_| {
            Error::Internal("Failed to acquire vector index lock".to_string())
        })?;

        let search_results = index_guard.search(embedding, limit).map_err(|e| {
            Error::Internal(format!("Failed to search vector index: {}", e))
        })?;

        // Convert search results to SemanticSearchResult
        let mut results = Vec::new();
        for result in search_results {
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
                if episode.is_valid() {
                    // Convert distance to similarity score
                    // For cosine distance: distance = 1 - similarity, so similarity = 1 - distance
                    let score = 1.0 - result.distance;
                    results.push(SemanticSearchResult { episode, score });
                }
            }
        }

        Ok(results)
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

        // Clear existing index
        {
            let mut index_guard = index.write().map_err(|_| {
                Error::Internal("Failed to acquire vector index lock".to_string())
            })?;
            index_guard.clear().map_err(|e| {
                Error::Internal(format!("Failed to clear vector index: {}", e))
            })?;
        }

        // Get all episodes
        let episodes = self.get_all_episodes().await?;
        let mut indexed_count = 0;

        // Index each episode
        for episode in episodes {
            if let Err(e) = self.index_episode(&episode).await {
                warn!(
                    "Failed to index episode {} during rebuild: {}",
                    episode.id, e
                );
            } else {
                indexed_count += 1;
            }
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
        let weight = semantic_weight.unwrap_or(0.5).clamp(0.0, 1.0);
        let keyword_weight = 1.0 - weight;

        // Perform keyword search
        let keyword_results = self.search_episodes(query).await?;

        // If semantic search is not enabled, return keyword results only
        if !self.has_semantic_search() {
            return Ok(keyword_results
                .into_iter()
                .take(limit)
                .enumerate()
                .map(|(rank, episode)| {
                    let keyword_score = 1.0 / (rank as f32 + 1.0);
                    HybridSearchResult {
                        episode,
                        score: keyword_score * keyword_weight,
                        semantic_score: None,
                        keyword_score: Some(keyword_score),
                    }
                })
                .collect());
        }

        // Perform semantic search
        let semantic_results = self.semantic_search(query, limit * 2).await?;

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
        for (rank, episode) in keyword_results.into_iter().enumerate() {
            let rrf_score = 1.0 / (k + rank as f32 + 1.0);
            score_map.insert(episode.id, (Some(rrf_score), None, episode));
        }

        // Add semantic results with RRF scores
        for (rank, result) in semantic_results.into_iter().enumerate() {
            let rrf_score = 1.0 / (k + rank as f32 + 1.0);
            if let Some((keyword_score, semantic_score, _)) = score_map.get_mut(&result.episode.id) {
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
        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

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
            semantic_config: self.semantic_config.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embeddings::EmbeddingProviderType;

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

        // Should still be retrievable but invalid
        let retrieved = memory.get_episode(id).unwrap().unwrap();
        assert!(!retrieved.is_valid());
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
}
