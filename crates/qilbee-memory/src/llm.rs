//! LLM (Large Language Model) provider for memory consolidation
//!
//! This module provides abstraction for LLM providers to enable
//! memory consolidation strategies like summarization and fact extraction.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;
use tracing::{debug, info, warn};

/// Errors that can occur during LLM operations
#[derive(Debug, Error)]
pub enum LLMError {
    /// API request failed
    #[error("API request failed: {0}")]
    ApiError(String),

    /// Rate limit exceeded
    #[error("Rate limit exceeded")]
    RateLimitExceeded,

    /// Invalid input
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    /// Provider not configured
    #[error("Provider not configured: {0}")]
    NotConfigured(String),

    /// Network error
    #[error("Network error: {0}")]
    Network(String),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Response parsing error
    #[error("Response parsing error: {0}")]
    ParseError(String),
}

/// Result type for LLM operations
pub type LLMResult<T> = Result<T, LLMError>;

/// Configuration for LLM providers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMConfig {
    /// Provider type
    pub provider: LLMProviderType,

    /// Model name (e.g., "gpt-4o-mini", "gpt-4o")
    pub model: String,

    /// API key (for cloud providers)
    pub api_key: Option<String>,

    /// API base URL (for custom endpoints)
    pub api_base_url: Option<String>,

    /// Maximum tokens in response
    pub max_tokens: u32,

    /// Temperature for response generation (0.0 - 2.0)
    pub temperature: f32,

    /// Timeout in seconds for API requests
    pub timeout_secs: u64,
}

impl Default for LLMConfig {
    fn default() -> Self {
        Self {
            provider: LLMProviderType::Mock,
            model: "mock-llm".to_string(),
            api_key: None,
            api_base_url: None,
            max_tokens: 1024,
            temperature: 0.3,
            timeout_secs: 60,
        }
    }
}

impl LLMConfig {
    /// Create config for OpenAI GPT-4o-mini (fast, cost-effective)
    pub fn openai_mini(api_key: &str) -> Self {
        Self {
            provider: LLMProviderType::OpenAI,
            model: "gpt-4o-mini".to_string(),
            api_key: Some(api_key.to_string()),
            api_base_url: Some("https://api.openai.com/v1".to_string()),
            max_tokens: 1024,
            temperature: 0.3,
            timeout_secs: 60,
        }
    }

    /// Create config for OpenAI GPT-4o (higher quality)
    pub fn openai_4o(api_key: &str) -> Self {
        Self {
            provider: LLMProviderType::OpenAI,
            model: "gpt-4o".to_string(),
            api_key: Some(api_key.to_string()),
            api_base_url: Some("https://api.openai.com/v1".to_string()),
            max_tokens: 2048,
            temperature: 0.3,
            timeout_secs: 120,
        }
    }

    /// Create config for mock LLM (testing)
    pub fn mock() -> Self {
        Self {
            provider: LLMProviderType::Mock,
            model: "mock-llm".to_string(),
            api_key: None,
            api_base_url: None,
            max_tokens: 1024,
            temperature: 0.3,
            timeout_secs: 10,
        }
    }
}

/// Supported LLM provider types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LLMProviderType {
    /// OpenAI Chat API
    OpenAI,
    /// Mock provider for testing
    Mock,
}

/// Message role in a chat conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    System,
    User,
    Assistant,
}

/// A message in a chat conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: MessageRole,
    pub content: String,
}

impl ChatMessage {
    /// Create a system message
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::System,
            content: content.into(),
        }
    }

    /// Create a user message
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::User,
            content: content.into(),
        }
    }

    /// Create an assistant message
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Assistant,
            content: content.into(),
        }
    }
}

/// Response from an LLM completion
#[derive(Debug, Clone)]
pub struct LLMResponse {
    /// The generated content
    pub content: String,

    /// Token usage information
    pub usage: Option<TokenUsage>,

    /// Model used for generation
    pub model: String,
}

/// Token usage information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// Trait for LLM providers
#[async_trait]
pub trait LLMProvider: Send + Sync {
    /// Get the model name
    fn model_name(&self) -> &str;

    /// Generate a chat completion
    async fn chat(&self, messages: &[ChatMessage]) -> LLMResult<LLMResponse>;

    /// Generate a completion for a single prompt (convenience method)
    async fn complete(&self, prompt: &str) -> LLMResult<String> {
        let messages = vec![ChatMessage::user(prompt)];
        let response = self.chat(&messages).await?;
        Ok(response.content)
    }

    /// Generate a completion with a system prompt
    async fn complete_with_system(
        &self,
        system_prompt: &str,
        user_prompt: &str,
    ) -> LLMResult<String> {
        let messages = vec![
            ChatMessage::system(system_prompt),
            ChatMessage::user(user_prompt),
        ];
        let response = self.chat(&messages).await?;
        Ok(response.content)
    }
}

/// Mock LLM provider for testing
///
/// Returns deterministic responses based on input for testing purposes.
pub struct MockLLMProvider {
    model: String,
}

impl MockLLMProvider {
    /// Create a new mock provider
    pub fn new() -> Self {
        info!("Created mock LLM provider");
        Self {
            model: "mock-llm".to_string(),
        }
    }
}

impl Default for MockLLMProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl LLMProvider for MockLLMProvider {
    fn model_name(&self) -> &str {
        &self.model
    }

    async fn chat(&self, messages: &[ChatMessage]) -> LLMResult<LLMResponse> {
        if messages.is_empty() {
            return Err(LLMError::InvalidInput("No messages provided".to_string()));
        }

        // Find the last user message
        let empty_string = String::new();
        let user_content = messages
            .iter()
            .filter(|m| matches!(m.role, MessageRole::User))
            .last()
            .map(|m| m.content.as_str())
            .unwrap_or(&empty_string);

        let user_content_len = user_content.len();

        // Generate mock response based on content
        let content = if user_content.to_lowercase().contains("summarize") {
            "This is a mock summary of the provided content. Key points include: 1) Important event occurred, 2) Action was taken, 3) Result was achieved.".to_string()
        } else if user_content.to_lowercase().contains("extract") || user_content.to_lowercase().contains("fact") {
            r#"[{"subject": "agent", "predicate": "performed", "object": "action", "confidence": 0.9}, {"subject": "event", "predicate": "occurred_at", "object": "time", "confidence": 0.8}]"#.to_string()
        } else if user_content.to_lowercase().contains("merge") {
            "Merged content: The episodes describe related events that can be consolidated into a single narrative about the agent's activities.".to_string()
        } else {
            format!("Mock response to: {}", &user_content[..user_content_len.min(50)])
        };

        debug!("Mock LLM response generated");

        let content_len = content.len();
        Ok(LLMResponse {
            content,
            usage: Some(TokenUsage {
                prompt_tokens: user_content_len as u32 / 4,
                completion_tokens: content_len as u32 / 4,
                total_tokens: (user_content_len + content_len) as u32 / 4,
            }),
            model: self.model.clone(),
        })
    }
}

/// OpenAI LLM provider
///
/// Uses the OpenAI Chat Completions API.
#[cfg(feature = "openai")]
pub struct OpenAILLMProvider {
    config: LLMConfig,
    client: reqwest::Client,
}

#[cfg(feature = "openai")]
impl OpenAILLMProvider {
    /// Create a new OpenAI provider
    pub fn new(config: LLMConfig) -> LLMResult<Self> {
        if config.api_key.is_none() {
            return Err(LLMError::NotConfigured(
                "OpenAI API key required".to_string(),
            ));
        }

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout_secs))
            .build()
            .map_err(|e| LLMError::Network(e.to_string()))?;

        info!("Created OpenAI LLM provider with model {}", config.model);

        Ok(Self { config, client })
    }
}

#[cfg(feature = "openai")]
#[async_trait]
impl LLMProvider for OpenAILLMProvider {
    fn model_name(&self) -> &str {
        &self.config.model
    }

    async fn chat(&self, messages: &[ChatMessage]) -> LLMResult<LLMResponse> {
        if messages.is_empty() {
            return Err(LLMError::InvalidInput("No messages provided".to_string()));
        }

        let api_key = self
            .config
            .api_key
            .as_ref()
            .ok_or_else(|| LLMError::NotConfigured("API key missing".to_string()))?;

        let base_url = self
            .config
            .api_base_url
            .as_ref()
            .map(|s| s.as_str())
            .unwrap_or("https://api.openai.com/v1");

        let url = format!("{}/chat/completions", base_url);

        #[derive(Serialize)]
        struct ChatRequest<'a> {
            model: &'a str,
            messages: &'a [ChatMessage],
            max_tokens: u32,
            temperature: f32,
        }

        #[derive(Deserialize)]
        struct ChatResponse {
            choices: Vec<Choice>,
            usage: Option<ApiUsage>,
            model: String,
        }

        #[derive(Deserialize)]
        struct Choice {
            message: ResponseMessage,
        }

        #[derive(Deserialize)]
        struct ResponseMessage {
            content: String,
        }

        #[derive(Deserialize)]
        struct ApiUsage {
            prompt_tokens: u32,
            completion_tokens: u32,
            total_tokens: u32,
        }

        let request = ChatRequest {
            model: &self.config.model,
            messages,
            max_tokens: self.config.max_tokens,
            temperature: self.config.temperature,
        };

        debug!("Sending chat request to OpenAI with {} messages", messages.len());

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| LLMError::Network(e.to_string()))?;

        if response.status() == 429 {
            return Err(LLMError::RateLimitExceeded);
        }

        if !response.status().is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(LLMError::ApiError(error_text));
        }

        let chat_response: ChatResponse = response
            .json()
            .await
            .map_err(|e| LLMError::Serialization(e.to_string()))?;

        let content = chat_response
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .ok_or_else(|| LLMError::ParseError("No choices in response".to_string()))?;

        Ok(LLMResponse {
            content,
            usage: chat_response.usage.map(|u| TokenUsage {
                prompt_tokens: u.prompt_tokens,
                completion_tokens: u.completion_tokens,
                total_tokens: u.total_tokens,
            }),
            model: chat_response.model,
        })
    }
}

/// Create an LLM provider from configuration
pub fn create_provider(config: LLMConfig) -> LLMResult<Arc<dyn LLMProvider>> {
    match config.provider {
        LLMProviderType::Mock => Ok(Arc::new(MockLLMProvider::new())),
        #[cfg(feature = "openai")]
        LLMProviderType::OpenAI => Ok(Arc::new(OpenAILLMProvider::new(config)?)),
        #[cfg(not(feature = "openai"))]
        LLMProviderType::OpenAI => Err(LLMError::NotConfigured(
            "OpenAI feature not enabled. Compile with --features openai".to_string(),
        )),
    }
}

/// Runtime-configurable LLM service
///
/// Allows changing LLM configuration at runtime without server restart.
pub struct LLMService {
    provider: tokio::sync::RwLock<Arc<dyn LLMProvider>>,
    config: tokio::sync::RwLock<LLMConfig>,
}

impl LLMService {
    /// Create a new LLM service with the given configuration
    pub fn new(config: LLMConfig) -> LLMResult<Self> {
        let provider = create_provider(config.clone())?;
        info!("Created LLM service with {} provider", config.model);
        Ok(Self {
            provider: tokio::sync::RwLock::new(provider),
            config: tokio::sync::RwLock::new(config),
        })
    }

    /// Create a new LLM service with mock provider (for testing)
    pub fn mock() -> Self {
        let config = LLMConfig::mock();
        Self {
            provider: tokio::sync::RwLock::new(Arc::new(MockLLMProvider::new())),
            config: tokio::sync::RwLock::new(config),
        }
    }

    /// Update the LLM configuration at runtime
    pub async fn update_config(&self, new_config: LLMConfig) -> LLMResult<()> {
        let new_provider = create_provider(new_config.clone())?;

        let mut config = self.config.write().await;
        let mut provider = self.provider.write().await;

        *config = new_config.clone();
        *provider = new_provider;

        info!("Updated LLM service to use {} provider", new_config.model);
        Ok(())
    }

    /// Get the current configuration
    pub async fn get_config(&self) -> LLMConfig {
        self.config.read().await.clone()
    }

    /// Get the current model name
    pub async fn model_name(&self) -> String {
        self.provider.read().await.model_name().to_string()
    }

    /// Check if the service is using a real LLM (not mock)
    pub async fn is_configured(&self) -> bool {
        let config = self.config.read().await;
        config.provider != LLMProviderType::Mock && config.api_key.is_some()
    }

    /// Generate a chat completion
    pub async fn chat(&self, messages: &[ChatMessage]) -> LLMResult<LLMResponse> {
        let provider = self.provider.read().await;
        provider.chat(messages).await
    }

    /// Generate a completion for a single prompt
    pub async fn complete(&self, prompt: &str) -> LLMResult<String> {
        let provider = self.provider.read().await;
        provider.complete(prompt).await
    }

    /// Generate a completion with a system prompt
    pub async fn complete_with_system(
        &self,
        system_prompt: &str,
        user_prompt: &str,
    ) -> LLMResult<String> {
        let provider = self.provider.read().await;
        provider.complete_with_system(system_prompt, user_prompt).await
    }
}

/// Configuration status response for API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMStatus {
    /// Whether the service is configured with a real provider
    pub configured: bool,

    /// Provider type
    pub provider: LLMProviderType,

    /// Model name
    pub model: String,

    /// Max tokens setting
    pub max_tokens: u32,

    /// Temperature setting
    pub temperature: f32,

    /// Whether API key is set (not the actual key)
    pub has_api_key: bool,
}

impl LLMStatus {
    /// Create status from config
    pub fn from_config(config: &LLMConfig) -> Self {
        Self {
            configured: config.provider != LLMProviderType::Mock && config.api_key.is_some(),
            provider: config.provider,
            model: config.model.clone(),
            max_tokens: config.max_tokens,
            temperature: config.temperature,
            has_api_key: config.api_key.is_some(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_llm_config_default() {
        let config = LLMConfig::default();
        assert_eq!(config.provider, LLMProviderType::Mock);
        assert_eq!(config.model, "mock-llm");
    }

    #[test]
    fn test_llm_config_openai_mini() {
        let config = LLMConfig::openai_mini("test-key");
        assert_eq!(config.provider, LLMProviderType::OpenAI);
        assert_eq!(config.model, "gpt-4o-mini");
        assert_eq!(config.api_key, Some("test-key".to_string()));
    }

    #[test]
    fn test_chat_message_constructors() {
        let system = ChatMessage::system("You are helpful");
        assert!(matches!(system.role, MessageRole::System));
        assert_eq!(system.content, "You are helpful");

        let user = ChatMessage::user("Hello");
        assert!(matches!(user.role, MessageRole::User));

        let assistant = ChatMessage::assistant("Hi there");
        assert!(matches!(assistant.role, MessageRole::Assistant));
    }

    #[tokio::test]
    async fn test_mock_llm_provider() {
        let provider = MockLLMProvider::new();
        assert_eq!(provider.model_name(), "mock-llm");

        let messages = vec![ChatMessage::user("Please summarize this content")];
        let response = provider.chat(&messages).await.unwrap();

        assert!(response.content.contains("summary"));
        assert!(response.usage.is_some());
    }

    #[tokio::test]
    async fn test_mock_llm_complete() {
        let provider = MockLLMProvider::new();
        let response = provider.complete("Hello world").await.unwrap();
        assert!(!response.is_empty());
    }

    #[tokio::test]
    async fn test_mock_llm_complete_with_system() {
        let provider = MockLLMProvider::new();
        let response = provider
            .complete_with_system("You are a helpful assistant", "Please summarize")
            .await
            .unwrap();
        assert!(response.contains("summary"));
    }

    #[tokio::test]
    async fn test_mock_llm_extract_facts() {
        let provider = MockLLMProvider::new();
        let messages = vec![ChatMessage::user("Extract facts from this text")];
        let response = provider.chat(&messages).await.unwrap();
        // The mock returns a JSON array for fact extraction
        assert!(response.content.contains("subject"));
        assert!(response.content.contains("predicate"));
    }

    #[test]
    fn test_create_mock_provider() {
        let config = LLMConfig::mock();
        let provider = create_provider(config).unwrap();
        assert_eq!(provider.model_name(), "mock-llm");
    }

    #[tokio::test]
    async fn test_llm_service_creation() {
        let service = LLMService::mock();
        assert_eq!(service.model_name().await, "mock-llm");
        assert!(!service.is_configured().await);
    }

    #[tokio::test]
    async fn test_llm_service_complete() {
        let service = LLMService::mock();
        let response = service.complete("Hello world").await.unwrap();
        assert!(!response.is_empty());
    }

    #[tokio::test]
    async fn test_llm_service_update_config() {
        let service = LLMService::mock();
        assert_eq!(service.model_name().await, "mock-llm");

        // Update to a different mock config (same provider but different settings)
        let new_config = LLMConfig {
            provider: LLMProviderType::Mock,
            model: "mock-llm-v2".to_string(),
            ..Default::default()
        };

        // Note: This will still use MockLLMProvider because provider type is Mock
        service.update_config(new_config).await.unwrap();

        let config = service.get_config().await;
        assert_eq!(config.model, "mock-llm-v2");
    }

    #[test]
    fn test_llm_status_from_config() {
        let config = LLMConfig::mock();
        let status = LLMStatus::from_config(&config);
        assert!(!status.configured);
        assert!(!status.has_api_key);
        assert_eq!(status.provider, LLMProviderType::Mock);

        let config_with_key = LLMConfig::openai_mini("test-key");
        let status = LLMStatus::from_config(&config_with_key);
        assert!(status.configured);
        assert!(status.has_api_key);
        assert_eq!(status.provider, LLMProviderType::OpenAI);
    }
}
