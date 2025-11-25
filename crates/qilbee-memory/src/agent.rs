//! Agent memory manager

use crate::episode::{Episode, EpisodeId, EpisodeType};
use crate::types::{MemoryConfig, MemoryType, Relevance};
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::episode::EpisodeContent;

    #[test]
    fn test_agent_memory_creation() {
        let memory = AgentMemory::for_agent("test-agent");
        assert_eq!(memory.agent_id(), "test-agent");
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
