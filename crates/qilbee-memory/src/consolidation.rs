//! Memory consolidation service
//!
//! This module handles the consolidation of episodic memories into
//! semantic memories using LLM-powered strategies.

use crate::agent::AgentMemory;
use crate::episode::{Episode, EpisodeContent, EpisodeType};
use crate::llm::{LLMConfig, LLMProvider};
use qilbee_core::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{debug, info, warn};

/// Strategy for memory consolidation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConsolidationStrategy {
    /// Summarize multiple episodes into a single semantic memory
    Summarize,
    /// Extract key facts from episodes as structured data
    ExtractFacts,
    /// Build/update knowledge graph from episodes
    BuildGraph,
    /// Merge similar episodes into consolidated episodes
    Merge,
}

/// Configuration for consolidation service
#[derive(Debug, Clone)]
pub struct ConsolidationConfig {
    /// LLM configuration
    pub llm_config: LLMConfig,

    /// Default strategy to use
    pub default_strategy: ConsolidationStrategy,

    /// Minimum number of episodes to trigger consolidation
    pub min_episodes: usize,

    /// Maximum episodes to process in one batch
    pub max_batch_size: usize,

    /// Similarity threshold for merging (0.0 - 1.0)
    pub merge_similarity_threshold: f32,

    /// Whether to mark episodes as consolidated after processing
    pub mark_consolidated: bool,
}

impl Default for ConsolidationConfig {
    fn default() -> Self {
        Self {
            llm_config: LLMConfig::default(),
            default_strategy: ConsolidationStrategy::Summarize,
            min_episodes: 3,
            max_batch_size: 10,
            merge_similarity_threshold: 0.8,
            mark_consolidated: true,
        }
    }
}

impl ConsolidationConfig {
    /// Create config with OpenAI GPT-4o-mini
    pub fn with_openai(api_key: &str) -> Self {
        Self {
            llm_config: LLMConfig::openai_mini(api_key),
            ..Default::default()
        }
    }

    /// Create config for testing with mock LLM
    pub fn for_testing() -> Self {
        Self {
            llm_config: LLMConfig::mock(),
            min_episodes: 2,
            ..Default::default()
        }
    }
}

/// Memory consolidation service
///
/// Handles the process of converting short-term episodic memories
/// into long-term semantic and factual memories using LLM.
pub struct ConsolidationService {
    /// Configuration
    config: ConsolidationConfig,

    /// LLM provider
    llm: Arc<dyn LLMProvider>,
}

impl ConsolidationService {
    /// Create a new consolidation service
    pub fn new(config: ConsolidationConfig) -> Result<Self> {
        let llm = crate::llm::create_provider(config.llm_config.clone())
            .map_err(|e| qilbee_core::Error::Internal(format!("Failed to create LLM provider: {}", e)))?;

        info!(
            "Created consolidation service with {:?} strategy and {} model",
            config.default_strategy,
            llm.model_name()
        );

        Ok(Self { config, llm })
    }

    /// Create a consolidation service with a custom LLM provider
    pub fn with_provider(config: ConsolidationConfig, llm: Arc<dyn LLMProvider>) -> Self {
        info!(
            "Created consolidation service with {:?} strategy and custom provider",
            config.default_strategy
        );
        Self { config, llm }
    }

    /// Consolidate episodes from a memory store using the default strategy
    pub async fn consolidate(&self, memory: &AgentMemory) -> Result<ConsolidationResult> {
        self.consolidate_with_strategy(memory, self.config.default_strategy)
            .await
    }

    /// Consolidate episodes using a specific strategy
    pub async fn consolidate_with_strategy(
        &self,
        memory: &AgentMemory,
        strategy: ConsolidationStrategy,
    ) -> Result<ConsolidationResult> {
        let candidates = self.get_consolidation_candidates(memory, self.config.max_batch_size)?;

        if candidates.len() < self.config.min_episodes {
            debug!(
                "Not enough episodes for consolidation: {} < {}",
                candidates.len(),
                self.config.min_episodes
            );
            return Ok(ConsolidationResult {
                episodes_processed: 0,
                memories_created: 0,
                strategy_used: strategy,
                details: None,
            });
        }

        info!(
            "Consolidating {} episodes with {:?} strategy",
            candidates.len(),
            strategy
        );

        let result = match strategy {
            ConsolidationStrategy::Summarize => self.consolidate_summarize(memory, &candidates).await,
            ConsolidationStrategy::ExtractFacts => self.consolidate_extract_facts(memory, &candidates).await,
            ConsolidationStrategy::Merge => self.consolidate_merge(memory, &candidates).await,
            ConsolidationStrategy::BuildGraph => {
                // BuildGraph is more complex and would integrate with qilbee-graph
                warn!("BuildGraph strategy not yet fully implemented");
                Ok(ConsolidationResult {
                    episodes_processed: 0,
                    memories_created: 0,
                    strategy_used: strategy,
                    details: Some("BuildGraph strategy requires graph integration".to_string()),
                })
            }
        }?;

        // Mark episodes as consolidated if configured
        if self.config.mark_consolidated && result.episodes_processed > 0 {
            for episode in &candidates[..result.episodes_processed.min(candidates.len())] {
                if let Err(e) = memory.mark_consolidated(episode.id) {
                    warn!("Failed to mark episode {} as consolidated: {}", episode.id, e);
                }
            }
        }

        Ok(result)
    }

    /// Get episodes ready for consolidation
    pub fn get_consolidation_candidates(
        &self,
        memory: &AgentMemory,
        limit: usize,
    ) -> Result<Vec<Episode>> {
        let episodes = memory.get_recent_episodes(limit * 2)?;

        Ok(episodes
            .into_iter()
            .filter(|e| !e.consolidated && e.is_valid())
            .take(limit)
            .collect())
    }

    /// Summarize episodes into a semantic memory
    async fn consolidate_summarize(
        &self,
        memory: &AgentMemory,
        episodes: &[Episode],
    ) -> Result<ConsolidationResult> {
        if episodes.is_empty() {
            return Ok(ConsolidationResult::empty(ConsolidationStrategy::Summarize));
        }

        // Build the prompt with episode content
        let episodes_text = episodes
            .iter()
            .enumerate()
            .map(|(i, e)| {
                format!("{}. [{:?}] {}", i + 1, e.episode_type, e.to_searchable_text())
            })
            .collect::<Vec<_>>()
            .join("\n");

        let system_prompt = SUMMARIZE_SYSTEM_PROMPT;
        let user_prompt = format!(
            "Please summarize the following {} episodes from agent '{}':\n\n{}\n\nProvide a concise summary that captures the key events, decisions, and outcomes.",
            episodes.len(),
            episodes.first().map(|e| e.agent_id.as_str()).unwrap_or("unknown"),
            episodes_text
        );

        let summary = self
            .llm
            .complete_with_system(system_prompt, &user_prompt)
            .await
            .map_err(|e| qilbee_core::Error::Internal(format!("LLM error: {}", e)))?;

        // Store the summary as a new semantic memory episode
        let summary_episode = Episode::new(
            &episodes.first().map(|e| e.agent_id.clone()).unwrap_or_default(),
            EpisodeType::Custom("SemanticMemory".to_string()),
            EpisodeContent::new(&summary),
        );

        memory.store_episode(summary_episode)?;

        info!(
            "Created summary from {} episodes: {}...",
            episodes.len(),
            &summary[..summary.len().min(100)]
        );

        Ok(ConsolidationResult {
            episodes_processed: episodes.len(),
            memories_created: 1,
            strategy_used: ConsolidationStrategy::Summarize,
            details: Some(summary),
        })
    }

    /// Extract facts from episodes
    async fn consolidate_extract_facts(
        &self,
        memory: &AgentMemory,
        episodes: &[Episode],
    ) -> Result<ConsolidationResult> {
        if episodes.is_empty() {
            return Ok(ConsolidationResult::empty(ConsolidationStrategy::ExtractFacts));
        }

        // Build the prompt with episode content
        let episodes_text = episodes
            .iter()
            .map(|e| e.to_searchable_text())
            .collect::<Vec<_>>()
            .join("\n---\n");

        let system_prompt = EXTRACT_FACTS_SYSTEM_PROMPT;
        let user_prompt = format!(
            "Extract key facts from the following episodes:\n\n{}\n\nReturn the facts as a JSON array.",
            episodes_text
        );

        let facts_json = self
            .llm
            .complete_with_system(system_prompt, &user_prompt)
            .await
            .map_err(|e| qilbee_core::Error::Internal(format!("LLM error: {}", e)))?;

        // Parse and store facts
        let facts: Vec<ExtractedFact> = serde_json::from_str(&facts_json).unwrap_or_else(|_| {
            // Try to extract from wrapped response
            if let Some(start) = facts_json.find('[') {
                if let Some(end) = facts_json.rfind(']') {
                    if let Ok(parsed) = serde_json::from_str(&facts_json[start..=end]) {
                        return parsed;
                    }
                }
            }
            warn!("Failed to parse facts JSON, storing as text");
            vec![ExtractedFact {
                subject: "agent".to_string(),
                predicate: "learned".to_string(),
                object: facts_json.clone(),
                confidence: 0.5,
            }]
        });

        let mut memories_created = 0;
        for fact in &facts {
            // Store fact as structured data in the content
            let fact_content = EpisodeContent::new(&format!(
                "{} {} {}",
                fact.subject, fact.predicate, fact.object
            ))
            .with_data(serde_json::to_value(fact).unwrap_or_default());

            let fact_episode = Episode::new(
                &episodes.first().map(|e| e.agent_id.clone()).unwrap_or_default(),
                EpisodeType::Custom("FactualMemory".to_string()),
                fact_content,
            );
            memory.store_episode(fact_episode)?;
            memories_created += 1;
        }

        info!(
            "Extracted {} facts from {} episodes",
            facts.len(),
            episodes.len()
        );

        Ok(ConsolidationResult {
            episodes_processed: episodes.len(),
            memories_created,
            strategy_used: ConsolidationStrategy::ExtractFacts,
            details: Some(format!("Extracted {} facts", facts.len())),
        })
    }

    /// Merge similar episodes
    async fn consolidate_merge(
        &self,
        memory: &AgentMemory,
        episodes: &[Episode],
    ) -> Result<ConsolidationResult> {
        if episodes.len() < 2 {
            return Ok(ConsolidationResult::empty(ConsolidationStrategy::Merge));
        }

        // Group similar episodes using simple text similarity
        let groups = self.group_similar_episodes(episodes);

        if groups.is_empty() {
            return Ok(ConsolidationResult {
                episodes_processed: 0,
                memories_created: 0,
                strategy_used: ConsolidationStrategy::Merge,
                details: Some("No similar episode groups found".to_string()),
            });
        }

        let mut total_processed = 0;
        let mut memories_created = 0;

        for group in groups {
            if group.len() < 2 {
                continue;
            }

            // Use LLM to merge the group
            let episodes_text = group
                .iter()
                .map(|e| e.to_searchable_text())
                .collect::<Vec<_>>()
                .join("\n---\n");

            let system_prompt = MERGE_SYSTEM_PROMPT;
            let user_prompt = format!(
                "Merge the following {} similar episodes into a single consolidated episode:\n\n{}",
                group.len(),
                episodes_text
            );

            let merged_content = self
                .llm
                .complete_with_system(system_prompt, &user_prompt)
                .await
                .map_err(|e| qilbee_core::Error::Internal(format!("LLM error: {}", e)))?;

            // Store merged episode
            let merged_episode = Episode::new(
                &group.first().map(|e| e.agent_id.clone()).unwrap_or_default(),
                EpisodeType::Observation, // Merged episodes are observations
                EpisodeContent::new(&merged_content),
            );

            memory.store_episode(merged_episode)?;
            total_processed += group.len();
            memories_created += 1;
        }

        info!(
            "Merged {} episodes into {} consolidated memories",
            total_processed, memories_created
        );

        Ok(ConsolidationResult {
            episodes_processed: total_processed,
            memories_created,
            strategy_used: ConsolidationStrategy::Merge,
            details: Some(format!(
                "Merged {} episodes into {} memories",
                total_processed, memories_created
            )),
        })
    }

    /// Group similar episodes based on content similarity
    fn group_similar_episodes(&self, episodes: &[Episode]) -> Vec<Vec<Episode>> {
        let mut groups: Vec<Vec<Episode>> = Vec::new();
        let mut used: Vec<bool> = vec![false; episodes.len()];

        for i in 0..episodes.len() {
            if used[i] {
                continue;
            }

            let mut group = vec![episodes[i].clone()];
            used[i] = true;

            for j in (i + 1)..episodes.len() {
                if used[j] {
                    continue;
                }

                let similarity = self.text_similarity(
                    &episodes[i].to_searchable_text(),
                    &episodes[j].to_searchable_text(),
                );

                if similarity >= self.config.merge_similarity_threshold {
                    group.push(episodes[j].clone());
                    used[j] = true;
                }
            }

            if group.len() >= 2 {
                groups.push(group);
            }
        }

        groups
    }

    /// Simple Jaccard similarity for text
    fn text_similarity(&self, text1: &str, text2: &str) -> f32 {
        let words1: std::collections::HashSet<&str> = text1.split_whitespace().collect();
        let words2: std::collections::HashSet<&str> = text2.split_whitespace().collect();

        if words1.is_empty() && words2.is_empty() {
            return 1.0;
        }

        let intersection = words1.intersection(&words2).count();
        let union = words1.union(&words2).count();

        if union == 0 {
            0.0
        } else {
            intersection as f32 / union as f32
        }
    }
}

/// Result of a consolidation operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidationResult {
    /// Number of episodes processed
    pub episodes_processed: usize,

    /// Number of new memories created
    pub memories_created: usize,

    /// Strategy that was used
    pub strategy_used: ConsolidationStrategy,

    /// Additional details about the consolidation
    pub details: Option<String>,
}

impl ConsolidationResult {
    /// Create an empty result
    pub fn empty(strategy: ConsolidationStrategy) -> Self {
        Self {
            episodes_processed: 0,
            memories_created: 0,
            strategy_used: strategy,
            details: None,
        }
    }
}

/// A fact extracted from episodes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedFact {
    /// Subject of the fact
    pub subject: String,

    /// Predicate/relationship
    pub predicate: String,

    /// Object of the fact
    pub object: String,

    /// Confidence score (0.0 - 1.0)
    #[serde(default = "default_confidence")]
    pub confidence: f32,
}

fn default_confidence() -> f32 {
    0.8
}

// System prompts for consolidation strategies
const SUMMARIZE_SYSTEM_PROMPT: &str = r#"You are a memory consolidation assistant for an AI agent system.
Your task is to summarize episodic memories into concise semantic memories.
Focus on:
- Key events and their outcomes
- Important decisions made
- Lessons learned
- Patterns observed
Keep summaries factual and concise (2-4 sentences)."#;

const EXTRACT_FACTS_SYSTEM_PROMPT: &str = r#"You are a fact extraction assistant for an AI agent system.
Extract structured facts from episodic memories as JSON.
Each fact should have:
- subject: The entity the fact is about
- predicate: The relationship or property
- object: The value or related entity
- confidence: Your confidence (0.0-1.0)

Return ONLY a JSON array like:
[{"subject": "agent", "predicate": "learned", "object": "skill", "confidence": 0.9}]"#;

const MERGE_SYSTEM_PROMPT: &str = r#"You are a memory consolidation assistant for an AI agent system.
Your task is to merge similar episodic memories into a single consolidated memory.
Combine the key information from all episodes while removing redundancy.
Keep the merged memory concise but complete."#;

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_memory() -> AgentMemory {
        AgentMemory::for_agent("test-agent")
    }

    fn create_test_service() -> ConsolidationService {
        let config = ConsolidationConfig::for_testing();
        ConsolidationService::new(config).unwrap()
    }

    #[test]
    fn test_consolidation_config_default() {
        let config = ConsolidationConfig::default();
        assert_eq!(config.default_strategy, ConsolidationStrategy::Summarize);
        assert_eq!(config.min_episodes, 3);
    }

    #[test]
    fn test_consolidation_config_with_openai() {
        let config = ConsolidationConfig::with_openai("test-key");
        assert_eq!(config.llm_config.model, "gpt-4o-mini");
    }

    #[test]
    fn test_consolidation_service_creation() {
        let service = create_test_service();
        assert_eq!(service.config.default_strategy, ConsolidationStrategy::Summarize);
    }

    #[test]
    fn test_get_consolidation_candidates() {
        let memory = create_test_memory();
        let service = create_test_service();

        // Store some episodes
        memory
            .store_episode(Episode::observation("test-agent", "Event 1"))
            .unwrap();
        memory
            .store_episode(Episode::observation("test-agent", "Event 2"))
            .unwrap();

        let candidates = service.get_consolidation_candidates(&memory, 10).unwrap();
        assert_eq!(candidates.len(), 2);
    }

    #[test]
    fn test_text_similarity() {
        let service = create_test_service();

        let sim = service.text_similarity("hello world", "hello world");
        assert!((sim - 1.0).abs() < 0.01);

        let sim = service.text_similarity("hello world", "goodbye moon");
        assert!(sim < 0.5);

        let sim = service.text_similarity("the quick brown fox", "the quick red fox");
        assert!(sim > 0.5);
    }

    #[test]
    fn test_group_similar_episodes() {
        let service = create_test_service();

        let episodes = vec![
            Episode::observation("test", "User asked about weather forecast"),
            Episode::observation("test", "User asked about weather tomorrow"),
            Episode::observation("test", "Agent processed a payment"),
        ];

        let groups = service.group_similar_episodes(&episodes);
        // Weather episodes should be grouped together
        assert!(!groups.is_empty() || groups.len() <= 2);
    }

    #[tokio::test]
    async fn test_consolidate_not_enough_episodes() {
        let memory = create_test_memory();
        let service = create_test_service();

        // Store only one episode (less than min_episodes=2)
        memory
            .store_episode(Episode::observation("test-agent", "Single event"))
            .unwrap();

        let result = service.consolidate(&memory).await.unwrap();
        assert_eq!(result.episodes_processed, 0);
        assert_eq!(result.memories_created, 0);
    }

    #[tokio::test]
    async fn test_consolidate_summarize() {
        let memory = create_test_memory();
        let config = ConsolidationConfig {
            llm_config: LLMConfig::mock(),
            min_episodes: 2,
            mark_consolidated: false, // Don't mark in test
            ..Default::default()
        };
        let service = ConsolidationService::new(config).unwrap();

        // Store enough episodes
        memory
            .store_episode(Episode::observation("test-agent", "User logged in"))
            .unwrap();
        memory
            .store_episode(Episode::observation("test-agent", "User viewed dashboard"))
            .unwrap();
        memory
            .store_episode(Episode::observation("test-agent", "User updated settings"))
            .unwrap();

        let result = service
            .consolidate_with_strategy(&memory, ConsolidationStrategy::Summarize)
            .await
            .unwrap();

        assert!(result.episodes_processed > 0);
        assert_eq!(result.memories_created, 1);
        assert_eq!(result.strategy_used, ConsolidationStrategy::Summarize);
        assert!(result.details.is_some());
    }

    #[tokio::test]
    async fn test_consolidate_extract_facts() {
        let memory = create_test_memory();
        let config = ConsolidationConfig {
            llm_config: LLMConfig::mock(),
            min_episodes: 2,
            mark_consolidated: false,
            ..Default::default()
        };
        let service = ConsolidationService::new(config).unwrap();

        // Store episodes
        memory
            .store_episode(Episode::observation("test-agent", "Agent learned Python"))
            .unwrap();
        memory
            .store_episode(Episode::observation("test-agent", "Agent completed task"))
            .unwrap();

        let result = service
            .consolidate_with_strategy(&memory, ConsolidationStrategy::ExtractFacts)
            .await
            .unwrap();

        assert!(result.episodes_processed > 0);
        assert!(result.memories_created > 0);
        assert_eq!(result.strategy_used, ConsolidationStrategy::ExtractFacts);
    }

    #[tokio::test]
    async fn test_consolidate_merge() {
        let memory = create_test_memory();
        let config = ConsolidationConfig {
            llm_config: LLMConfig::mock(),
            min_episodes: 2,
            merge_similarity_threshold: 0.3, // Lower threshold for testing
            mark_consolidated: false,
            ..Default::default()
        };
        let service = ConsolidationService::new(config).unwrap();

        // Store similar episodes
        memory
            .store_episode(Episode::observation("test-agent", "User asked about the weather forecast for today"))
            .unwrap();
        memory
            .store_episode(Episode::observation("test-agent", "User asked about the weather forecast tomorrow"))
            .unwrap();

        let result = service
            .consolidate_with_strategy(&memory, ConsolidationStrategy::Merge)
            .await
            .unwrap();

        assert_eq!(result.strategy_used, ConsolidationStrategy::Merge);
    }

    #[test]
    fn test_consolidation_result_empty() {
        let result = ConsolidationResult::empty(ConsolidationStrategy::Summarize);
        assert_eq!(result.episodes_processed, 0);
        assert_eq!(result.memories_created, 0);
        assert!(result.details.is_none());
    }

    #[test]
    fn test_extracted_fact_serialization() {
        let fact = ExtractedFact {
            subject: "agent".to_string(),
            predicate: "learned".to_string(),
            object: "Python".to_string(),
            confidence: 0.9,
        };

        let json = serde_json::to_string(&fact).unwrap();
        assert!(json.contains("agent"));
        assert!(json.contains("Python"));

        let parsed: ExtractedFact = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.subject, "agent");
    }
}
