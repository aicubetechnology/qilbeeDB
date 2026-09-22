//! Memory types and configuration

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Type of memory
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MemoryType {
    /// Specific events and interactions
    Episodic,
    /// General knowledge and concepts
    Semantic,
    /// How-to knowledge and workflows
    Procedural,
    /// User preferences and persistent facts
    Factual,
}

impl MemoryType {
    /// Get default retention period for this memory type
    pub fn default_retention(&self) -> Duration {
        match self {
            MemoryType::Episodic => Duration::from_secs(30 * 24 * 60 * 60), // 30 days
            MemoryType::Semantic => Duration::from_secs(365 * 24 * 60 * 60), // 1 year
            MemoryType::Procedural => Duration::from_secs(365 * 24 * 60 * 60), // 1 year
            MemoryType::Factual => Duration::from_secs(u64::MAX), // Forever
        }
    }

    /// Get default relevance decay rate
    pub fn default_decay_rate(&self) -> f64 {
        match self {
            MemoryType::Episodic => 0.1,    // Fast decay
            MemoryType::Semantic => 0.01,   // Slow decay
            MemoryType::Procedural => 0.01, // Slow decay
            MemoryType::Factual => 0.0,     // No decay
        }
    }
}

/// Configuration for agent memory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    /// Agent identifier
    pub agent_id: String,

    /// Enable episodic memory
    pub enable_episodic: bool,

    /// Enable semantic memory
    pub enable_semantic: bool,

    /// Enable procedural memory
    pub enable_procedural: bool,

    /// Enable factual memory
    pub enable_factual: bool,

    /// Retention period for episodic memories
    pub episodic_retention: Duration,

    /// Minimum relevance score to keep memory
    pub min_relevance: f64,

    /// Enable automatic consolidation
    pub auto_consolidate: bool,

    /// Consolidation interval
    pub consolidation_interval: Duration,

    /// Enable automatic forgetting
    pub auto_forget: bool,

    /// Forgetting interval
    pub forget_interval: Duration,

    /// Maximum number of episodes to keep
    pub max_episodes: usize,

    /// Number of episodes that triggers consolidation
    pub consolidation_threshold: usize,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            agent_id: String::new(),
            enable_episodic: true,
            enable_semantic: true,
            enable_procedural: true,
            enable_factual: true,
            episodic_retention: Duration::from_secs(30 * 24 * 60 * 60), // 30 days
            min_relevance: 0.1,
            auto_consolidate: true,
            consolidation_interval: Duration::from_secs(24 * 60 * 60), // 1 day
            auto_forget: true,
            forget_interval: Duration::from_secs(24 * 60 * 60), // 1 day
            max_episodes: 10000,
            consolidation_threshold: 5000, // Consolidate at 50% capacity
        }
    }
}

impl MemoryConfig {
    /// Create a new configuration for an agent
    pub fn new(agent_id: &str) -> Self {
        Self {
            agent_id: agent_id.to_string(),
            ..Default::default()
        }
    }

    /// Builder: set episodic retention
    pub fn episodic_retention(mut self, duration: Duration) -> Self {
        self.episodic_retention = duration;
        self
    }

    /// Builder: set minimum relevance
    pub fn min_relevance(mut self, relevance: f64) -> Self {
        self.min_relevance = relevance;
        self
    }

    /// Builder: disable automatic consolidation
    pub fn no_auto_consolidate(mut self) -> Self {
        self.auto_consolidate = false;
        self
    }

    /// Builder: disable automatic forgetting
    pub fn no_auto_forget(mut self) -> Self {
        self.auto_forget = false;
        self
    }

    /// Builder: set max episodes
    pub fn max_episodes(mut self, max: usize) -> Self {
        self.max_episodes = max;
        self
    }
}

/// Relevance with explicit temporal accounting. Native struct construction now
/// requires accounting state; deserialize old stored episodes via the storage adapter.
pub use crate::relevance_accounting::AccountedRelevance as Relevance;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_maintenance_does_not_charge_the_same_elapsed_day_twice() {
        let mut relevance = Relevance::new_at(0);
        relevance.decay_at(86_400_000, 0.1).unwrap();
        let after_first = relevance;
        relevance.decay_at(86_400_000, 0.1).unwrap();
        assert_eq!(relevance, after_first);
    }


    #[test]
    fn test_memory_type_defaults() {
        assert!(MemoryType::Episodic.default_retention() < MemoryType::Semantic.default_retention());
        assert!(MemoryType::Episodic.default_decay_rate() > MemoryType::Semantic.default_decay_rate());
    }

    #[test]
    fn test_memory_config_builder() {
        let config = MemoryConfig::new("agent-1")
            .min_relevance(0.2)
            .max_episodes(5000)
            .no_auto_forget();

        assert_eq!(config.agent_id, "agent-1");
        assert_eq!(config.min_relevance, 0.2);
        assert_eq!(config.max_episodes, 5000);
        assert!(!config.auto_forget);
    }

    #[test]
    fn test_relevance_access() {
        let mut rel = Relevance::new();
        let initial_score = rel.score;

        rel.access().unwrap();

        assert!(rel.access_count == 1);
        assert!(rel.score >= initial_score);
    }

    #[test]
    fn test_relevance_should_forget() {
        let mut rel = Relevance::new();
        rel.score = 0.05;

        assert!(rel.should_forget(0.1));
        assert!(!rel.should_forget(0.01));
    }
}
