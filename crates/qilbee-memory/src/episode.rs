//! Episodic memory implementation

use crate::types::Relevance;
use qilbee_core::temporal::{BiTemporal, EventTime, TransactionTime};
use qilbee_core::Property;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// An episode in agent memory
///
/// Represents a discrete interaction or event that the agent should remember.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Episode {
    /// Unique identifier
    pub id: EpisodeId,

    /// Agent this episode belongs to
    pub agent_id: String,

    /// Type of episode
    pub episode_type: EpisodeType,

    /// When this episode occurred
    pub event_time: EventTime,

    /// When this was stored
    pub transaction_time: TransactionTime,

    /// The content of the episode
    pub content: EpisodeContent,

    /// Additional metadata
    pub metadata: Property,

    /// Relevance tracking
    pub relevance: Relevance,

    /// Whether this episode has been consolidated
    pub consolidated: bool,

    /// When this episode was invalidated (if ever)
    pub invalidated_at: Option<TransactionTime>,
}

/// Episode identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EpisodeId(Uuid);

impl EpisodeId {
    /// Create a new episode ID
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Create from UUID
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Get as UUID
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl Default for EpisodeId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for EpisodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Type of episode
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EpisodeType {
    /// User-agent conversation
    Conversation,
    /// Task execution
    TaskExecution,
    /// Observation of external event
    Observation,
    /// Internal agent decision
    Decision,
    /// Error or exception
    Error,
    /// Custom episode type
    Custom(String),
}

/// Content of an episode
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodeContent {
    /// Primary content (e.g., user message)
    pub primary: String,

    /// Secondary content (e.g., agent response)
    pub secondary: Option<String>,

    /// Context at time of episode
    pub context: Option<String>,

    /// Structured data
    pub data: Option<serde_json::Value>,

    /// Embedding vector (for similarity search)
    pub embedding: Option<Vec<f32>>,
}

impl EpisodeContent {
    /// Create new episode content
    pub fn new(primary: &str) -> Self {
        Self {
            primary: primary.to_string(),
            secondary: None,
            context: None,
            data: None,
            embedding: None,
        }
    }

    /// Builder: add secondary content
    pub fn with_secondary(mut self, secondary: &str) -> Self {
        self.secondary = Some(secondary.to_string());
        self
    }

    /// Builder: add context
    pub fn with_context(mut self, context: &str) -> Self {
        self.context = Some(context.to_string());
        self
    }

    /// Builder: add structured data
    pub fn with_data(mut self, data: serde_json::Value) -> Self {
        self.data = Some(data);
        self
    }

    /// Builder: add embedding
    pub fn with_embedding(mut self, embedding: Vec<f32>) -> Self {
        self.embedding = Some(embedding);
        self
    }
}

impl Episode {
    /// Create a new episode
    pub fn new(agent_id: &str, episode_type: EpisodeType, content: EpisodeContent) -> Self {
        Self {
            id: EpisodeId::new(),
            agent_id: agent_id.to_string(),
            episode_type,
            event_time: EventTime::now(),
            transaction_time: TransactionTime::now(),
            content,
            metadata: Property::new(),
            relevance: Relevance::new(),
            consolidated: false,
            invalidated_at: None,
        }
    }

    /// Create with a specific event time
    pub fn with_event_time(
        agent_id: &str,
        episode_type: EpisodeType,
        content: EpisodeContent,
        event_time: EventTime,
    ) -> Self {
        Self {
            id: EpisodeId::new(),
            agent_id: agent_id.to_string(),
            episode_type,
            event_time,
            transaction_time: TransactionTime::now(),
            content,
            metadata: Property::new(),
            relevance: Relevance::new(),
            consolidated: false,
            invalidated_at: None,
        }
    }

    /// Create a conversation episode
    pub fn conversation(agent_id: &str, user_message: &str, agent_response: &str) -> Self {
        let content = EpisodeContent::new(user_message).with_secondary(agent_response);
        Self::new(agent_id, EpisodeType::Conversation, content)
    }

    /// Create a task execution episode
    pub fn task_execution(agent_id: &str, task: &str, result: &str) -> Self {
        let content = EpisodeContent::new(task).with_secondary(result);
        Self::new(agent_id, EpisodeType::TaskExecution, content)
    }

    /// Create an observation episode
    pub fn observation(agent_id: &str, observation: &str) -> Self {
        let content = EpisodeContent::new(observation);
        Self::new(agent_id, EpisodeType::Observation, content)
    }

    /// Check if this episode is still valid
    pub fn is_valid(&self) -> bool {
        self.invalidated_at.is_none()
    }

    /// Invalidate this episode
    pub fn invalidate(&mut self) {
        self.invalidated_at = Some(TransactionTime::now());
    }

    /// Mark as accessed
    pub fn access(&mut self) {
        self.relevance.access();
    }

    /// Mark as consolidated
    pub fn mark_consolidated(&mut self) {
        self.consolidated = true;
    }

    /// Add metadata
    pub fn set_metadata<K: Into<String>, V: Into<qilbee_core::PropertyValue>>(
        &mut self,
        key: K,
        value: V,
    ) {
        self.metadata.set(key, value);
    }

    /// Get metadata
    pub fn get_metadata(&self, key: &str) -> Option<&qilbee_core::PropertyValue> {
        self.metadata.get(key)
    }
}

/// Builder for episodes
pub struct EpisodeBuilder {
    agent_id: String,
    episode_type: EpisodeType,
    content: EpisodeContent,
    event_time: Option<EventTime>,
    metadata: Property,
}

impl EpisodeBuilder {
    /// Create a new episode builder
    pub fn new(agent_id: &str) -> Self {
        Self {
            agent_id: agent_id.to_string(),
            episode_type: EpisodeType::Observation,
            content: EpisodeContent::new(""),
            event_time: None,
            metadata: Property::new(),
        }
    }

    /// Set episode type
    pub fn episode_type(mut self, t: EpisodeType) -> Self {
        self.episode_type = t;
        self
    }

    /// Set content
    pub fn content(mut self, content: EpisodeContent) -> Self {
        self.content = content;
        self
    }

    /// Set event time
    pub fn event_time(mut self, time: EventTime) -> Self {
        self.event_time = Some(time);
        self
    }

    /// Add metadata
    pub fn metadata<K: Into<String>, V: Into<qilbee_core::PropertyValue>>(
        mut self,
        key: K,
        value: V,
    ) -> Self {
        self.metadata.set(key, value);
        self
    }

    /// Build the episode
    pub fn build(self) -> Episode {
        let mut episode = if let Some(time) = self.event_time {
            Episode::with_event_time(&self.agent_id, self.episode_type, self.content, time)
        } else {
            Episode::new(&self.agent_id, self.episode_type, self.content)
        };

        episode.metadata = self.metadata;
        episode
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_episode_creation() {
        let content = EpisodeContent::new("Hello, how can I help?");
        let episode = Episode::new("agent-1", EpisodeType::Conversation, content);

        assert_eq!(episode.agent_id, "agent-1");
        assert!(episode.is_valid());
        assert!(!episode.consolidated);
    }

    #[test]
    fn test_conversation_episode() {
        let episode = Episode::conversation("agent-1", "What's the weather?", "It's sunny today.");

        assert_eq!(episode.content.primary, "What's the weather?");
        assert_eq!(
            episode.content.secondary,
            Some("It's sunny today.".to_string())
        );
        assert_eq!(episode.episode_type, EpisodeType::Conversation);
    }

    #[test]
    fn test_episode_invalidation() {
        let mut episode = Episode::observation("agent-1", "User logged in");

        assert!(episode.is_valid());
        episode.invalidate();
        assert!(!episode.is_valid());
    }

    #[test]
    fn test_episode_access() {
        let mut episode = Episode::observation("agent-1", "Event");
        let initial_count = episode.relevance.access_count;

        episode.access();
        assert_eq!(episode.relevance.access_count, initial_count + 1);
    }

    #[test]
    fn test_episode_builder() {
        let episode = EpisodeBuilder::new("agent-1")
            .episode_type(EpisodeType::Decision)
            .content(EpisodeContent::new("Made a decision"))
            .metadata("importance", "high")
            .build();

        assert_eq!(episode.episode_type, EpisodeType::Decision);
        assert!(episode.get_metadata("importance").is_some());
    }

    #[test]
    fn test_episode_content_builder() {
        let content = EpisodeContent::new("Primary")
            .with_secondary("Secondary")
            .with_context("Context");

        assert_eq!(content.primary, "Primary");
        assert_eq!(content.secondary, Some("Secondary".to_string()));
        assert_eq!(content.context, Some("Context".to_string()));
    }
}
