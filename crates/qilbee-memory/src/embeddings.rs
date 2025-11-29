//! Embedding generation and management for semantic search
//!
//! This module provides abstraction for embedding providers and vector operations
//! to enable semantic search over agent memories.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;
use tracing::{debug, info, warn};

/// Errors that can occur during embedding operations
#[derive(Debug, Error)]
pub enum EmbeddingError {
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

    /// Dimension mismatch
    #[error("Embedding dimension mismatch: expected {expected}, got {got}")]
    DimensionMismatch { expected: usize, got: usize },

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Network error
    #[error("Network error: {0}")]
    Network(String),
}

/// Result type for embedding operations
pub type EmbeddingResult<T> = Result<T, EmbeddingError>;

/// Configuration for embedding providers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingConfig {
    /// Provider type
    pub provider: EmbeddingProviderType,

    /// Model name (e.g., "text-embedding-3-small")
    pub model: String,

    /// Embedding dimensions
    pub dimensions: usize,

    /// API key (for cloud providers)
    pub api_key: Option<String>,

    /// API base URL (for custom endpoints)
    pub api_base_url: Option<String>,

    /// Maximum batch size for embedding requests
    pub max_batch_size: usize,

    /// Timeout in seconds for API requests
    pub timeout_secs: u64,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            provider: EmbeddingProviderType::Mock,
            model: "mock-embedding".to_string(),
            dimensions: 384,
            api_key: None,
            api_base_url: None,
            max_batch_size: 100,
            timeout_secs: 30,
        }
    }
}

impl EmbeddingConfig {
    /// Create config for OpenAI text-embedding-3-small
    pub fn openai_small(api_key: &str) -> Self {
        Self {
            provider: EmbeddingProviderType::OpenAI,
            model: "text-embedding-3-small".to_string(),
            dimensions: 1536,
            api_key: Some(api_key.to_string()),
            api_base_url: Some("https://api.openai.com/v1".to_string()),
            max_batch_size: 100,
            timeout_secs: 30,
        }
    }

    /// Create config for OpenAI text-embedding-3-large
    pub fn openai_large(api_key: &str) -> Self {
        Self {
            provider: EmbeddingProviderType::OpenAI,
            model: "text-embedding-3-large".to_string(),
            dimensions: 3072,
            api_key: Some(api_key.to_string()),
            api_base_url: Some("https://api.openai.com/v1".to_string()),
            max_batch_size: 100,
            timeout_secs: 30,
        }
    }

    /// Create config for mock embeddings (testing)
    pub fn mock(dimensions: usize) -> Self {
        Self {
            provider: EmbeddingProviderType::Mock,
            model: "mock-embedding".to_string(),
            dimensions,
            api_key: None,
            api_base_url: None,
            max_batch_size: 100,
            timeout_secs: 30,
        }
    }

    /// Create config for local embedding server (e.g., Ollama, local transformer)
    pub fn local(base_url: &str, model: &str, dimensions: usize) -> Self {
        Self {
            provider: EmbeddingProviderType::Local,
            model: model.to_string(),
            dimensions,
            api_key: None,
            api_base_url: Some(base_url.to_string()),
            max_batch_size: 32,
            timeout_secs: 60,
        }
    }
}

/// Supported embedding provider types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EmbeddingProviderType {
    /// OpenAI embeddings API
    OpenAI,
    /// Local embedding server (e.g., Ollama)
    Local,
    /// Mock provider for testing
    Mock,
}

/// Trait for embedding providers
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    /// Get the embedding dimension for this provider
    fn dimensions(&self) -> usize;

    /// Get the model name
    fn model_name(&self) -> &str;

    /// Generate embedding for a single text
    async fn embed(&self, text: &str) -> EmbeddingResult<Vec<f32>>;

    /// Generate embeddings for multiple texts (batch)
    async fn embed_batch(&self, texts: &[String]) -> EmbeddingResult<Vec<Vec<f32>>>;
}

/// Mock embedding provider for testing
///
/// Generates deterministic embeddings based on text hash for testing purposes.
pub struct MockEmbeddingProvider {
    dimensions: usize,
}

impl MockEmbeddingProvider {
    /// Create a new mock provider
    pub fn new(dimensions: usize) -> Self {
        info!("Created mock embedding provider with {} dimensions", dimensions);
        Self { dimensions }
    }

    /// Generate a deterministic embedding from text
    fn hash_to_embedding(&self, text: &str) -> Vec<f32> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        text.hash(&mut hasher);
        let hash = hasher.finish();

        // Generate deterministic values based on hash
        let mut embedding = Vec::with_capacity(self.dimensions);
        let mut current = hash;

        for _ in 0..self.dimensions {
            // Use simple LCG to generate pseudo-random values
            current = current.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            // Normalize to [-1, 1] range
            let value = ((current as f64) / (u64::MAX as f64) * 2.0 - 1.0) as f32;
            embedding.push(value);
        }

        // Normalize the vector
        normalize_vector(&mut embedding);
        embedding
    }
}

#[async_trait]
impl EmbeddingProvider for MockEmbeddingProvider {
    fn dimensions(&self) -> usize {
        self.dimensions
    }

    fn model_name(&self) -> &str {
        "mock-embedding"
    }

    async fn embed(&self, text: &str) -> EmbeddingResult<Vec<f32>> {
        if text.is_empty() {
            return Err(EmbeddingError::InvalidInput("Empty text".to_string()));
        }
        debug!("Mock embedding for text of length {}", text.len());
        Ok(self.hash_to_embedding(text))
    }

    async fn embed_batch(&self, texts: &[String]) -> EmbeddingResult<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }

        let embeddings: Vec<Vec<f32>> = texts
            .iter()
            .filter(|t| !t.is_empty())
            .map(|t| self.hash_to_embedding(t))
            .collect();

        debug!("Mock batch embedding for {} texts", embeddings.len());
        Ok(embeddings)
    }
}

/// OpenAI embedding provider
///
/// Uses the OpenAI Embeddings API to generate embeddings.
#[cfg(feature = "openai")]
pub struct OpenAIEmbeddingProvider {
    config: EmbeddingConfig,
    client: reqwest::Client,
}

#[cfg(feature = "openai")]
impl OpenAIEmbeddingProvider {
    /// Create a new OpenAI provider
    pub fn new(config: EmbeddingConfig) -> EmbeddingResult<Self> {
        if config.api_key.is_none() {
            return Err(EmbeddingError::NotConfigured(
                "OpenAI API key required".to_string(),
            ));
        }

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout_secs))
            .build()
            .map_err(|e| EmbeddingError::Network(e.to_string()))?;

        info!(
            "Created OpenAI embedding provider with model {}",
            config.model
        );

        Ok(Self { config, client })
    }
}

#[cfg(feature = "openai")]
#[async_trait]
impl EmbeddingProvider for OpenAIEmbeddingProvider {
    fn dimensions(&self) -> usize {
        self.config.dimensions
    }

    fn model_name(&self) -> &str {
        &self.config.model
    }

    async fn embed(&self, text: &str) -> EmbeddingResult<Vec<f32>> {
        let embeddings = self.embed_batch(&[text.to_string()]).await?;
        embeddings
            .into_iter()
            .next()
            .ok_or_else(|| EmbeddingError::ApiError("Empty response".to_string()))
    }

    async fn embed_batch(&self, texts: &[String]) -> EmbeddingResult<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }

        let api_key = self
            .config
            .api_key
            .as_ref()
            .ok_or_else(|| EmbeddingError::NotConfigured("API key missing".to_string()))?;

        let base_url = self
            .config
            .api_base_url
            .as_ref()
            .map(|s| s.as_str())
            .unwrap_or("https://api.openai.com/v1");

        let url = format!("{}/embeddings", base_url);

        #[derive(Serialize)]
        struct EmbeddingRequest<'a> {
            model: &'a str,
            input: &'a [String],
        }

        #[derive(Deserialize)]
        struct EmbeddingResponse {
            data: Vec<EmbeddingData>,
        }

        #[derive(Deserialize)]
        struct EmbeddingData {
            embedding: Vec<f32>,
            index: usize,
        }

        let request = EmbeddingRequest {
            model: &self.config.model,
            input: texts,
        };

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| EmbeddingError::Network(e.to_string()))?;

        if response.status() == 429 {
            return Err(EmbeddingError::RateLimitExceeded);
        }

        if !response.status().is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(EmbeddingError::ApiError(error_text));
        }

        let embedding_response: EmbeddingResponse = response
            .json()
            .await
            .map_err(|e| EmbeddingError::Serialization(e.to_string()))?;

        // Sort by index to ensure correct order
        let mut data = embedding_response.data;
        data.sort_by_key(|d| d.index);

        Ok(data.into_iter().map(|d| d.embedding).collect())
    }
}

/// Create an embedding provider from configuration
pub fn create_provider(config: EmbeddingConfig) -> EmbeddingResult<Arc<dyn EmbeddingProvider>> {
    match config.provider {
        EmbeddingProviderType::Mock => {
            Ok(Arc::new(MockEmbeddingProvider::new(config.dimensions)))
        }
        #[cfg(feature = "openai")]
        EmbeddingProviderType::OpenAI => {
            Ok(Arc::new(OpenAIEmbeddingProvider::new(config)?))
        }
        #[cfg(not(feature = "openai"))]
        EmbeddingProviderType::OpenAI => {
            Err(EmbeddingError::NotConfigured(
                "OpenAI feature not enabled. Compile with --features openai".to_string(),
            ))
        }
        EmbeddingProviderType::Local => {
            // For now, return mock provider; actual local provider would need implementation
            warn!("Local provider not fully implemented, using mock");
            Ok(Arc::new(MockEmbeddingProvider::new(config.dimensions)))
        }
    }
}

// ============== Vector Similarity Functions ==============

/// Normalize a vector in-place
pub fn normalize_vector(v: &mut [f32]) {
    let magnitude: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if magnitude > 0.0 {
        for x in v.iter_mut() {
            *x /= magnitude;
        }
    }
}

/// Calculate cosine similarity between two vectors
///
/// Returns a value between -1 and 1, where:
/// - 1 means identical direction
/// - 0 means orthogonal (unrelated)
/// - -1 means opposite direction
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }

    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let mag_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let mag_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if mag_a == 0.0 || mag_b == 0.0 {
        return 0.0;
    }

    dot / (mag_a * mag_b)
}

/// Calculate dot product between two vectors
///
/// For normalized vectors, this is equivalent to cosine similarity.
pub fn dot_product(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }

    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

/// Calculate Euclidean distance between two vectors
///
/// Lower values indicate more similar vectors.
pub fn euclidean_distance(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return f32::MAX;
    }

    a.iter()
        .zip(b.iter())
        .map(|(x, y)| (x - y).powi(2))
        .sum::<f32>()
        .sqrt()
}

/// Calculate squared Euclidean distance (faster, avoids sqrt)
pub fn euclidean_distance_squared(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return f32::MAX;
    }

    a.iter()
        .zip(b.iter())
        .map(|(x, y)| (x - y).powi(2))
        .sum()
}

/// Similarity metric types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SimilarityMetric {
    /// Cosine similarity (default, good for text)
    Cosine,
    /// Dot product (for normalized vectors)
    DotProduct,
    /// Euclidean distance (converted to similarity)
    Euclidean,
}

impl Default for SimilarityMetric {
    fn default() -> Self {
        SimilarityMetric::Cosine
    }
}

/// Calculate similarity between two vectors using the specified metric
///
/// Returns a value where higher = more similar.
pub fn similarity(a: &[f32], b: &[f32], metric: SimilarityMetric) -> f32 {
    match metric {
        SimilarityMetric::Cosine => cosine_similarity(a, b),
        SimilarityMetric::DotProduct => dot_product(a, b),
        SimilarityMetric::Euclidean => {
            // Convert distance to similarity (inverse, bounded)
            let dist = euclidean_distance(a, b);
            1.0 / (1.0 + dist)
        }
    }
}

/// Search result with score
#[derive(Debug, Clone)]
pub struct SimilarityResult<T> {
    /// The matched item
    pub item: T,
    /// Similarity score (higher is more similar)
    pub score: f32,
}

impl<T> SimilarityResult<T> {
    pub fn new(item: T, score: f32) -> Self {
        Self { item, score }
    }
}

/// Find top-k most similar items from a collection
///
/// Returns items with their similarity scores, sorted by score descending.
/// The function requires items to have an embedding, and will filter out items
/// where the `get_embedding` callback returns None.
pub fn find_top_k<T, F>(
    query: &[f32],
    items: impl IntoIterator<Item = T>,
    k: usize,
    metric: SimilarityMetric,
    get_embedding: F,
) -> Vec<SimilarityResult<T>>
where
    F: Fn(&T) -> Option<Vec<f32>>,
{
    let mut results: Vec<SimilarityResult<T>> = items
        .into_iter()
        .filter_map(|item| {
            get_embedding(&item).map(|emb| {
                let score = similarity(query, &emb, metric);
                SimilarityResult::new(item, score)
            })
        })
        .collect();

    // Sort by score descending
    results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

    // Take top k
    results.truncate(k);
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity_identical() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 0.0001);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        assert!(cosine_similarity(&a, &b).abs() < 0.0001);
    }

    #[test]
    fn test_cosine_similarity_opposite() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![-1.0, 0.0, 0.0];
        assert!((cosine_similarity(&a, &b) + 1.0).abs() < 0.0001);
    }

    #[test]
    fn test_dot_product() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![4.0, 5.0, 6.0];
        // 1*4 + 2*5 + 3*6 = 4 + 10 + 18 = 32
        assert!((dot_product(&a, &b) - 32.0).abs() < 0.0001);
    }

    #[test]
    fn test_euclidean_distance() {
        let a = vec![0.0, 0.0, 0.0];
        let b = vec![3.0, 4.0, 0.0];
        // sqrt(9 + 16) = 5
        assert!((euclidean_distance(&a, &b) - 5.0).abs() < 0.0001);
    }

    #[test]
    fn test_normalize_vector() {
        let mut v = vec![3.0, 4.0];
        normalize_vector(&mut v);
        assert!((v[0] - 0.6).abs() < 0.0001);
        assert!((v[1] - 0.8).abs() < 0.0001);
    }

    #[tokio::test]
    async fn test_mock_provider() {
        let provider = MockEmbeddingProvider::new(384);

        let embedding = provider.embed("Hello world").await.unwrap();
        assert_eq!(embedding.len(), 384);

        // Same text should produce same embedding
        let embedding2 = provider.embed("Hello world").await.unwrap();
        assert_eq!(embedding, embedding2);

        // Different text should produce different embedding
        let embedding3 = provider.embed("Goodbye world").await.unwrap();
        assert_ne!(embedding, embedding3);
    }

    #[tokio::test]
    async fn test_mock_provider_batch() {
        let provider = MockEmbeddingProvider::new(384);

        let texts = vec![
            "First text".to_string(),
            "Second text".to_string(),
            "Third text".to_string(),
        ];

        let embeddings = provider.embed_batch(&texts).await.unwrap();
        assert_eq!(embeddings.len(), 3);

        for emb in &embeddings {
            assert_eq!(emb.len(), 384);
        }
    }

    #[test]
    fn test_find_top_k() {
        let query = vec![1.0, 0.0, 0.0];

        let items = vec![
            ("a", vec![1.0, 0.0, 0.0]),  // identical
            ("b", vec![0.9, 0.1, 0.0]),  // very similar
            ("c", vec![0.0, 1.0, 0.0]),  // orthogonal
            ("d", vec![-1.0, 0.0, 0.0]), // opposite
        ];

        let results = find_top_k(
            &query,
            items.iter().cloned(),
            2,
            SimilarityMetric::Cosine,
            |item| Some(item.1.clone()),
        );

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].item.0, "a");
        assert_eq!(results[1].item.0, "b");
    }

    #[test]
    fn test_embedding_config_defaults() {
        let config = EmbeddingConfig::default();
        assert_eq!(config.provider, EmbeddingProviderType::Mock);
        assert_eq!(config.dimensions, 384);
    }

    #[test]
    fn test_embedding_config_openai() {
        let config = EmbeddingConfig::openai_small("test-key");
        assert_eq!(config.provider, EmbeddingProviderType::OpenAI);
        assert_eq!(config.dimensions, 1536);
        assert_eq!(config.model, "text-embedding-3-small");
    }
}
