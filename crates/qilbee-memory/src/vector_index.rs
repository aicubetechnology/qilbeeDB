//! HNSW (Hierarchical Navigable Small World) Vector Index
//!
//! This module implements an HNSW index for efficient approximate nearest neighbor (ANN) search.
//! HNSW provides logarithmic search complexity with high recall rates.
//!
//! # Features
//!
//! - Configurable index parameters (M, ef_construction, ef_search)
//! - Multiple similarity metrics (cosine, dot product, euclidean)
//! - Persistence support via RocksDB
//! - Thread-safe concurrent access
//!
//! # References
//!
//! - Malkov, Y. A., & Yashunin, D. A. (2018). Efficient and robust approximate nearest neighbor
//!   search using Hierarchical Navigable Small World graphs.

use crate::embeddings::SimilarityMetric;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::sync::{Arc, RwLock};
use thiserror::Error;

/// Errors that can occur during HNSW operations
#[derive(Error, Debug)]
pub enum HnswError {
    #[error("Vector dimension mismatch: expected {expected}, got {got}")]
    DimensionMismatch { expected: usize, got: usize },

    #[error("Index is empty")]
    EmptyIndex,

    #[error("Node not found: {0}")]
    NodeNotFound(String),

    #[error("Invalid parameter: {0}")]
    InvalidParameter(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Lock error: {0}")]
    LockError(String),
}

/// Result type for HNSW operations
pub type HnswResult<T> = Result<T, HnswError>;

/// Configuration for the HNSW index
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HnswConfig {
    /// Maximum number of connections per node per layer (default: 16)
    /// Higher values improve recall but increase memory and build time
    pub m: usize,

    /// Size of dynamic candidate list during construction (default: 200)
    /// Higher values improve recall during construction but slow down build
    pub ef_construction: usize,

    /// Size of dynamic candidate list during search (default: 50)
    /// Higher values improve recall but slow down search
    pub ef_search: usize,

    /// Maximum number of layers in the graph
    /// Computed as ln(N) / ln(M) where N is expected dataset size
    pub max_level: usize,

    /// Vector dimension (set when first vector is inserted)
    pub dimension: Option<usize>,

    /// Similarity metric to use
    pub metric: SimilarityMetric,

    /// Normalization multiplier for level generation
    /// Default: 1 / ln(M)
    pub ml: f32,
}

impl Default for HnswConfig {
    fn default() -> Self {
        Self {
            m: 16,
            ef_construction: 200,
            ef_search: 50,
            max_level: 16,
            dimension: None,
            metric: SimilarityMetric::Cosine,
            ml: 1.0 / (16.0_f32).ln(),
        }
    }
}

impl HnswConfig {
    fn validate(&self) -> HnswResult<()> {
        if self.m < 2
            || self.m.checked_mul(2).is_none()
            || self.ef_search == 0
            || self.ef_construction == 0
            || self.max_level >= usize::BITS as usize
            || !self.ml.is_finite()
            || self.ml <= 0.0
            || self.dimension == Some(0)
        {
            return Err(HnswError::InvalidParameter(
                "Invalid HNSW construction/search configuration".into(),
            ));
        }
        Ok(())
    }

    /// Create configuration for small datasets (< 10,000 vectors)
    pub fn small() -> Self {
        Self {
            m: 8,
            ef_construction: 100,
            ef_search: 30,
            max_level: 10,
            dimension: None,
            metric: SimilarityMetric::Cosine,
            ml: 1.0 / (8.0_f32).ln(),
        }
    }

    /// Create configuration for medium datasets (10,000 - 100,000 vectors)
    pub fn medium() -> Self {
        Self {
            m: 16,
            ef_construction: 200,
            ef_search: 50,
            max_level: 16,
            dimension: None,
            metric: SimilarityMetric::Cosine,
            ml: 1.0 / (16.0_f32).ln(),
        }
    }

    /// Create configuration for large datasets (> 100,000 vectors)
    pub fn large() -> Self {
        Self {
            m: 32,
            ef_construction: 400,
            ef_search: 100,
            max_level: 20,
            dimension: None,
            metric: SimilarityMetric::Cosine,
            ml: 1.0 / (32.0_f32).ln(),
        }
    }

    /// Set the similarity metric
    pub fn with_metric(mut self, metric: SimilarityMetric) -> Self {
        self.metric = metric;
        self
    }

    /// Set the dimension
    pub fn with_dimension(mut self, dimension: usize) -> Self {
        self.dimension = Some(dimension);
        self
    }

    /// Set ef_search for query time
    pub fn with_ef_search(mut self, ef_search: usize) -> Self {
        self.ef_search = ef_search;
        self
    }
}

/// A node in the HNSW graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HnswNode {
    /// Unique identifier for this node
    pub id: String,

    /// The vector data
    pub vector: Vec<f32>,

    /// Maximum layer this node appears in
    pub level: usize,

    /// Neighbors at each layer (layer -> set of neighbor IDs)
    pub neighbors: Vec<Vec<String>>,
}

impl HnswNode {
    fn new(id: String, vector: Vec<f32>, level: usize, max_level: usize) -> Self {
        let mut neighbors = Vec::with_capacity(level + 1);
        for _ in 0..=level.min(max_level) {
            neighbors.push(Vec::new());
        }
        Self {
            id,
            vector,
            level,
            neighbors,
        }
    }
}

/// A candidate during search (distance, node_id)
#[derive(Debug, Clone)]
struct Candidate {
    distance: f32,
    id: String,
}

impl PartialEq for Candidate {
    fn eq(&self, other: &Self) -> bool {
        self.distance == other.distance && self.id == other.id
    }
}

impl Eq for Candidate {}

impl PartialOrd for Candidate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Candidate {
    fn cmp(&self, other: &Self) -> Ordering {
        // For min-heap: reverse comparison so smaller distances are at the top
        other
            .distance
            .partial_cmp(&self.distance)
            .unwrap_or(Ordering::Equal)
    }
}

/// Max-heap wrapper for candidates (furthest first)
#[derive(Debug, Clone)]
struct MaxCandidate {
    distance: f32,
    id: String,
}

impl PartialEq for MaxCandidate {
    fn eq(&self, other: &Self) -> bool {
        self.distance == other.distance && self.id == other.id
    }
}

impl Eq for MaxCandidate {}

impl PartialOrd for MaxCandidate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for MaxCandidate {
    fn cmp(&self, other: &Self) -> Ordering {
        // For max-heap: normal comparison so larger distances are at the top
        self.distance
            .partial_cmp(&other.distance)
            .unwrap_or(Ordering::Equal)
    }
}

/// Search result containing the node ID and distance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    /// The node ID
    pub id: String,

    /// Distance/similarity to the query (interpretation depends on metric)
    pub distance: f32,
}

/// HNSW Index for approximate nearest neighbor search
#[derive(Debug)]
pub struct HnswIndex {
    /// Index configuration
    config: HnswConfig,

    /// All nodes in the index (id -> node)
    nodes: Arc<RwLock<HashMap<String, HnswNode>>>,

    /// Entry point node ID (highest level node)
    entry_point: Arc<RwLock<Option<String>>>,

    /// Current maximum level in the index
    current_max_level: Arc<RwLock<usize>>,
}

impl HnswIndex {
    /// Create a new HNSW index with the given configuration
    pub fn new(config: HnswConfig) -> Self {
        Self {
            config,
            nodes: Arc::new(RwLock::new(HashMap::new())),
            entry_point: Arc::new(RwLock::new(None)),
            current_max_level: Arc::new(RwLock::new(0)),
        }
    }

    /// Validate configuration before constructing a new index.
    pub fn try_new(config: HnswConfig) -> HnswResult<Self> {
        config.validate()?;
        Ok(Self::new(config))
    }

    /// Create a new HNSW index with default configuration
    pub fn with_defaults() -> Self {
        Self::new(HnswConfig::default())
    }

    /// Get the number of nodes in the index
    pub fn len(&self) -> usize {
        self.nodes.read().unwrap().len()
    }

    /// Check if the index is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get the configuration
    pub fn config(&self) -> &HnswConfig {
        &self.config
    }

    /// Calculate distance between two vectors based on the configured metric
    fn distance(&self, a: &[f32], b: &[f32]) -> f32 {
        match self.config.metric {
            SimilarityMetric::Cosine => {
                // Convert similarity to distance (1 - similarity)
                // Finite f32 products and their norms can overflow in f32.
                // Accumulate in f64; cosine itself is bounded and representable.
                let dot: f64 = a
                    .iter()
                    .zip(b)
                    .map(|(&x, &y)| f64::from(x) * f64::from(y))
                    .sum();
                let norm_a: f64 = a.iter().map(|&x| f64::from(x).powi(2)).sum();
                let norm_b: f64 = b.iter().map(|&x| f64::from(x).powi(2)).sum();
                (1.0 - (dot / (norm_a.sqrt() * norm_b.sqrt())).clamp(-1.0, 1.0)) as f32
            }
            SimilarityMetric::DotProduct => {
                // For dot product, higher is better, so negate
                -a.iter()
                    .zip(b)
                    .map(|(&x, &y)| f64::from(x) * f64::from(y))
                    .sum::<f64>() as f32
            }
            SimilarityMetric::Euclidean => a
                .iter()
                .zip(b)
                .map(|(&x, &y)| (f64::from(x) - f64::from(y)).powi(2))
                .sum::<f64>()
                .sqrt() as f32,
        }
    }

    fn validate_vector(&self, vector: &[f32]) -> HnswResult<()> {
        self.config.validate()?;
        if vector.is_empty() || vector.iter().any(|value| !value.is_finite()) {
            return Err(HnswError::InvalidParameter(
                "Vectors must be nonempty and finite".into(),
            ));
        }
        if self.config.metric == SimilarityMetric::Cosine
            && vector.iter().all(|&value| value == 0.0)
        {
            return Err(HnswError::InvalidParameter(
                "Cosine requires a nonzero vector".into(),
            ));
        }
        let norm_squared: f64 = vector.iter().map(|&x| f64::from(x).powi(2)).sum();
        let limit = match self.config.metric {
            SimilarityMetric::Cosine => f64::INFINITY,
            SimilarityMetric::DotProduct => f64::from(f32::MAX),
            SimilarityMetric::Euclidean => (f64::from(f32::MAX) / 2.0).powi(2),
        };
        if norm_squared > limit {
            return Err(HnswError::InvalidParameter(
                "Vector norm exceeds the finite f32 distance range for this metric".into(),
            ));
        }
        Ok(())
    }

    /// Generate a random level for a new node
    fn random_level(&self) -> usize {
        let mut level = 0;
        let threshold = self.config.ml;

        while rand_float() < threshold && level < self.config.max_level {
            level += 1;
        }

        level
    }

    /// Insert a vector into the index
    pub fn insert(&mut self, id: String, vector: Vec<f32>) -> HnswResult<()> {
        self.validate_vector(vector.as_slice())?;
        // Validate dimension
        if let Some(dim) = self.config.dimension {
            if vector.len() != dim {
                return Err(HnswError::DimensionMismatch {
                    expected: dim,
                    got: vector.len(),
                });
            }
        } else {
            // Set dimension from first vector
            self.config.dimension = Some(vector.len());
        }

        // Replacements preserve the existing topology while refreshing neighbors.
        let node_level = self
            .nodes
            .read()
            .map_err(|e| HnswError::LockError(e.to_string()))?
            .get(&id)
            .map(|node| node.level)
            .unwrap_or_else(|| self.random_level());

        // Check if this is the first node
        let entry_point = {
            let entry = self
                .entry_point
                .read()
                .map_err(|e| HnswError::LockError(format!("Failed to acquire read lock: {}", e)))?;
            entry.clone()
        };

        if entry_point.is_none() {
            // First node - just insert it
            let new_node = HnswNode::new(id.clone(), vector, node_level, self.config.max_level);

            {
                let mut nodes = self.nodes.write().map_err(|e| {
                    HnswError::LockError(format!("Failed to acquire write lock: {}", e))
                })?;
                nodes.insert(id.clone(), new_node);
            }

            {
                let mut entry = self.entry_point.write().map_err(|e| {
                    HnswError::LockError(format!("Failed to acquire write lock: {}", e))
                })?;
                *entry = Some(id);
            }

            {
                let mut max_level = self.current_max_level.write().map_err(|e| {
                    HnswError::LockError(format!("Failed to acquire write lock: {}", e))
                })?;
                *max_level = node_level;
            }

            return Ok(());
        }

        let entry_id = entry_point.unwrap();

        // Get current max level
        let current_max = {
            let max_level = self
                .current_max_level
                .read()
                .map_err(|e| HnswError::LockError(format!("Failed to acquire read lock: {}", e)))?;
            *max_level
        };

        // Create the new node
        let new_node = HnswNode::new(
            id.clone(),
            vector.clone(),
            node_level,
            self.config.max_level,
        );

        // Insert the node
        {
            let mut nodes = self.nodes.write().map_err(|e| {
                HnswError::LockError(format!("Failed to acquire write lock: {}", e))
            })?;
            if let Some(existing) = nodes.get_mut(&id) {
                existing.vector = vector.clone();
            } else {
                nodes.insert(id.clone(), new_node);
            }
        }

        // Phase 1: Search for entry point at layers above node_level
        let mut ep = entry_id.clone();
        for level in (node_level + 1..=current_max).rev() {
            let neighbors = self.search_layer(&vector, &ep, 1, level)?;
            if let Some(nearest) = neighbors.first() {
                ep = nearest.id.clone();
            }
        }

        // Phase 2: Insert at layers 0 to node_level
        for level in (0..=node_level.min(current_max)).rev() {
            let neighbors = self.search_layer(&vector, &ep, self.config.ef_construction, level)?;

            // Select M best neighbors
            let m = if level == 0 {
                self.config.m * 2
            } else {
                self.config.m
            };
            let selected: Vec<String> = neighbors
                .iter()
                .filter(|c| c.id != id)
                .take(m)
                .map(|c| c.id.clone())
                .collect();

            // Add bidirectional connections
            self.add_connections(&id, &selected, level)?;

            // Update entry point for next layer
            if let Some(nearest) = neighbors.first() {
                ep = nearest.id.clone();
            }
        }

        // Update entry point if new node has higher level
        if node_level > current_max {
            {
                let mut entry = self.entry_point.write().map_err(|e| {
                    HnswError::LockError(format!("Failed to acquire write lock: {}", e))
                })?;
                *entry = Some(id);
            }
            {
                let mut max_level = self.current_max_level.write().map_err(|e| {
                    HnswError::LockError(format!("Failed to acquire write lock: {}", e))
                })?;
                *max_level = node_level;
            }
        }

        Ok(())
    }

    /// Search for the k nearest neighbors of a query vector
    pub fn search(&self, query: &[f32], k: usize) -> HnswResult<Vec<SearchResult>> {
        self.validate_vector(query)?;
        // Validate dimension
        if let Some(dim) = self.config.dimension {
            if query.len() != dim {
                return Err(HnswError::DimensionMismatch {
                    expected: dim,
                    got: query.len(),
                });
            }
        }

        // Get entry point
        let entry_point = {
            let entry = self
                .entry_point
                .read()
                .map_err(|e| HnswError::LockError(format!("Failed to acquire read lock: {}", e)))?;
            entry.clone()
        };

        let Some(entry_id) = entry_point else {
            return Ok(Vec::new());
        };

        // Get current max level
        let current_max = {
            let max_level = self
                .current_max_level
                .read()
                .map_err(|e| HnswError::LockError(format!("Failed to acquire read lock: {}", e)))?;
            *max_level
        };

        // Phase 1: Greedy search from top to layer 1
        let mut ep = entry_id;
        for level in (1..=current_max).rev() {
            let neighbors = self.search_layer(query, &ep, 1, level)?;
            if let Some(nearest) = neighbors.first() {
                ep = nearest.id.clone();
            }
        }

        // Phase 2: Search at layer 0 with ef_search
        let candidates = self.search_layer(query, &ep, self.config.ef_search.max(k), 0)?;

        // Return top k results
        Ok(candidates
            .into_iter()
            .take(k)
            .map(|c| SearchResult {
                id: c.id,
                distance: c.distance,
            })
            .collect())
    }

    /// Search a single layer starting from an entry point
    fn search_layer(
        &self,
        query: &[f32],
        entry_point: &str,
        ef: usize,
        level: usize,
    ) -> HnswResult<Vec<Candidate>> {
        let nodes = self
            .nodes
            .read()
            .map_err(|e| HnswError::LockError(format!("Failed to acquire read lock: {}", e)))?;

        let entry_node = nodes
            .get(entry_point)
            .ok_or_else(|| HnswError::NodeNotFound(entry_point.to_string()))?;

        let entry_dist = self.distance(query, &entry_node.vector);

        let mut visited: HashSet<String> = HashSet::new();
        visited.insert(entry_point.to_string());

        // Min-heap for candidates (closest first)
        let mut candidates: BinaryHeap<Candidate> = BinaryHeap::new();
        candidates.push(Candidate {
            distance: entry_dist,
            id: entry_point.to_string(),
        });

        // Max-heap for results (furthest first, to maintain ef best results)
        let mut results: BinaryHeap<MaxCandidate> = BinaryHeap::new();
        results.push(MaxCandidate {
            distance: entry_dist,
            id: entry_point.to_string(),
        });

        while let Some(current) = candidates.pop() {
            // Get the furthest result
            let furthest_dist = results.peek().map(|r| r.distance).unwrap_or(f32::INFINITY);

            // If current candidate is further than our furthest result, stop
            if current.distance > furthest_dist && results.len() >= ef {
                break;
            }

            // Get neighbors of current node at this level
            let current_node = match nodes.get(&current.id) {
                Some(node) => node,
                None => continue,
            };

            if level >= current_node.neighbors.len() {
                continue;
            }

            for neighbor_id in &current_node.neighbors[level] {
                if visited.contains(neighbor_id) {
                    continue;
                }
                visited.insert(neighbor_id.clone());

                let neighbor_node = match nodes.get(neighbor_id) {
                    Some(node) => node,
                    None => continue,
                };

                let dist = self.distance(query, &neighbor_node.vector);
                let furthest_dist = results.peek().map(|r| r.distance).unwrap_or(f32::INFINITY);

                if dist < furthest_dist || results.len() < ef {
                    candidates.push(Candidate {
                        distance: dist,
                        id: neighbor_id.clone(),
                    });
                    results.push(MaxCandidate {
                        distance: dist,
                        id: neighbor_id.clone(),
                    });

                    // Keep only ef results
                    while results.len() > ef {
                        results.pop();
                    }
                }
            }
        }

        // Convert results to sorted vector
        let mut result_vec: Vec<Candidate> = results
            .into_iter()
            .map(|mc| Candidate {
                distance: mc.distance,
                id: mc.id,
            })
            .collect();

        result_vec.sort_by(|a, b| {
            a.distance
                .partial_cmp(&b.distance)
                .unwrap_or(Ordering::Equal)
        });

        Ok(result_vec)
    }

    /// Add bidirectional connections between a node and its neighbors
    fn add_connections(
        &mut self,
        node_id: &str,
        neighbors: &[String],
        level: usize,
    ) -> HnswResult<()> {
        let mut nodes = self
            .nodes
            .write()
            .map_err(|e| HnswError::LockError(format!("Failed to acquire write lock: {}", e)))?;

        // Add neighbors to node
        if let Some(node) = nodes.get_mut(node_id) {
            if level < node.neighbors.len() {
                for neighbor_id in neighbors {
                    if !node.neighbors[level].contains(neighbor_id) {
                        node.neighbors[level].push(neighbor_id.clone());
                    }
                }
            }
        }

        // Get node's vector for distance calculation
        let node_vector = nodes.get(node_id).map(|n| n.vector.clone());
        let Some(node_vec) = node_vector else {
            return Ok(());
        };

        // Add reverse connections
        let m = if level == 0 {
            self.config.m * 2
        } else {
            self.config.m
        };

        for neighbor_id in neighbors {
            if let Some(neighbor) = nodes.get_mut(neighbor_id) {
                if level < neighbor.neighbors.len() {
                    // Add connection if not present
                    if !neighbor.neighbors[level].contains(&node_id.to_string()) {
                        neighbor.neighbors[level].push(node_id.to_string());
                    }

                    // Prune if too many connections
                    if neighbor.neighbors[level].len() > m {
                        let neighbor_vec = neighbor.vector.clone();
                        let connections = neighbor.neighbors[level].clone();

                        // Calculate distances and sort
                        let mut connection_distances: Vec<(String, f32)> = connections
                            .iter()
                            .filter_map(|conn_id| {
                                nodes.get(conn_id).map(|conn_node| {
                                    (
                                        conn_id.clone(),
                                        self.distance(&neighbor_vec, &conn_node.vector),
                                    )
                                })
                            })
                            .collect();

                        connection_distances
                            .sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(Ordering::Equal));

                        // Keep only m best connections
                        if let Some(neighbor) = nodes.get_mut(neighbor_id) {
                            neighbor.neighbors[level] = connection_distances
                                .into_iter()
                                .take(m)
                                .map(|(id, _)| id)
                                .collect();
                        }
                    }
                }
            }
        }

        // Prune node's connections if needed
        if let Some(node) = nodes.get_mut(node_id) {
            if level < node.neighbors.len() && node.neighbors[level].len() > m {
                let connections = node.neighbors[level].clone();

                let mut connection_distances: Vec<(String, f32)> = connections
                    .iter()
                    .filter_map(|conn_id| {
                        nodes.get(conn_id).map(|conn_node| {
                            (conn_id.clone(), self.distance(&node_vec, &conn_node.vector))
                        })
                    })
                    .collect();

                connection_distances
                    .sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(Ordering::Equal));

                if let Some(node) = nodes.get_mut(node_id) {
                    node.neighbors[level] = connection_distances
                        .into_iter()
                        .take(m)
                        .map(|(id, _)| id)
                        .collect();
                }
            }
        }

        Ok(())
    }

    /// Remove a node from the index
    pub fn remove(&mut self, id: &str) -> HnswResult<bool> {
        let mut nodes = self
            .nodes
            .write()
            .map_err(|e| HnswError::LockError(format!("Failed to acquire write lock: {}", e)))?;

        // Check if node exists
        let Some(_node) = nodes.remove(id) else {
            return Ok(false);
        };

        // Pruning can leave directed links, so outgoing neighbors are not a
        // complete inventory of references to the removed node.
        for neighbor in nodes.values_mut() {
            for layer in &mut neighbor.neighbors {
                layer.retain(|target| target != id);
            }
        }

        drop(nodes);

        // Update entry point if needed
        let mut entry = self
            .entry_point
            .write()
            .map_err(|e| HnswError::LockError(format!("Failed to acquire write lock: {}", e)))?;

        if entry.as_ref() == Some(&id.to_string()) {
            let nodes = self
                .nodes
                .read()
                .map_err(|e| HnswError::LockError(format!("Failed to acquire read lock: {}", e)))?;

            // Find new entry point (highest level node)
            let new_entry = nodes.values().max_by_key(|n| n.level).map(|n| n.id.clone());

            *entry = new_entry;

            // Update max level
            if let Some(ref new_entry_id) = *entry {
                if let Some(new_entry_node) = nodes.get(new_entry_id) {
                    let mut max_level = self.current_max_level.write().map_err(|e| {
                        HnswError::LockError(format!("Failed to acquire write lock: {}", e))
                    })?;
                    *max_level = new_entry_node.level;
                }
            } else {
                *self
                    .current_max_level
                    .write()
                    .map_err(|e| HnswError::LockError(e.to_string()))? = 0;
            }
        }

        Ok(true)
    }

    /// Clear all nodes from the index
    pub fn clear(&mut self) -> HnswResult<()> {
        let mut nodes = self
            .nodes
            .write()
            .map_err(|e| HnswError::LockError(format!("Failed to acquire write lock: {}", e)))?;
        nodes.clear();
        drop(nodes);

        let mut entry = self
            .entry_point
            .write()
            .map_err(|e| HnswError::LockError(format!("Failed to acquire write lock: {}", e)))?;
        *entry = None;

        let mut max_level = self
            .current_max_level
            .write()
            .map_err(|e| HnswError::LockError(format!("Failed to acquire write lock: {}", e)))?;
        *max_level = 0;

        Ok(())
    }

    /// Get a node by ID
    pub fn get(&self, id: &str) -> Option<Vec<f32>> {
        let nodes = self.nodes.read().ok()?;
        nodes.get(id).map(|n| n.vector.clone())
    }

    /// Check if a node exists
    pub fn contains(&self, id: &str) -> bool {
        self.nodes
            .read()
            .map(|nodes| nodes.contains_key(id))
            .unwrap_or(false)
    }

    /// Get all node IDs
    pub fn node_ids(&self) -> Vec<String> {
        self.nodes
            .read()
            .map(|nodes| nodes.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Serialize the index to bytes
    pub fn to_bytes(&self) -> HnswResult<Vec<u8>> {
        let nodes = self
            .nodes
            .read()
            .map_err(|e| HnswError::LockError(format!("Failed to acquire read lock: {}", e)))?;

        let entry_point = self
            .entry_point
            .read()
            .map_err(|e| HnswError::LockError(format!("Failed to acquire read lock: {}", e)))?;

        let current_max_level = self
            .current_max_level
            .read()
            .map_err(|e| HnswError::LockError(format!("Failed to acquire read lock: {}", e)))?;

        let data = SerializedHnswIndex {
            config: self.config.clone(),
            nodes: nodes.clone(),
            entry_point: entry_point.clone(),
            current_max_level: *current_max_level,
        };

        data.validate()?;
        bincode::serialize(&data).map_err(|e| HnswError::SerializationError(e.to_string()))
    }

    /// Deserialize an index from bytes
    pub fn from_bytes(bytes: &[u8]) -> HnswResult<Self> {
        use bincode::Options;
        let data: SerializedHnswIndex = bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .with_limit(bytes.len() as u64)
            .reject_trailing_bytes()
            .deserialize(bytes)
            .map_err(|e| HnswError::SerializationError(e.to_string()))?;
        data.validate()?;
        Ok(Self {
            config: data.config,
            nodes: Arc::new(RwLock::new(data.nodes)),
            entry_point: Arc::new(RwLock::new(data.entry_point)),
            current_max_level: Arc::new(RwLock::new(data.current_max_level)),
        })
    }
}

/// Serializable version of HnswIndex
#[derive(Debug, Serialize, Deserialize)]
struct SerializedHnswIndex {
    config: HnswConfig,
    nodes: HashMap<String, HnswNode>,
    entry_point: Option<String>,
    current_max_level: usize,
}

impl SerializedHnswIndex {
    fn validate(&self) -> HnswResult<()> {
        self.config.validate()?;
        let invalid = |message: &str| HnswError::SerializationError(message.into());
        if self.nodes.is_empty() {
            if self.entry_point.is_some() || self.current_max_level != 0 {
                return Err(invalid("Empty snapshot has a live entry point or level"));
            }
            return Ok(());
        }
        let dimension = self
            .config
            .dimension
            .ok_or_else(|| invalid("Nonempty snapshot lacks dimensions"))?;
        let entry = self
            .entry_point
            .as_ref()
            .and_then(|id| self.nodes.get(id))
            .ok_or_else(|| invalid("Snapshot entry point does not exist"))?;
        let highest = self.nodes.values().map(|node| node.level).max().unwrap();
        if self.current_max_level != highest
            || entry.level != highest
            || highest > self.config.max_level
        {
            return Err(invalid("Snapshot maximum level disagrees with its nodes"));
        }
        let validator = HnswIndex::new(self.config.clone());
        for (id, node) in &self.nodes {
            if id != &node.id
                || node.vector.len() != dimension
                || node.level > self.config.max_level
                || node.neighbors.len() != node.level + 1
            {
                return Err(invalid(
                    "Snapshot node identity, dimension or layer shape is invalid",
                ));
            }
            validator.validate_vector(&node.vector)?;
            for (level, neighbors) in node.neighbors.iter().enumerate() {
                for neighbor in neighbors {
                    if !self
                        .nodes
                        .get(neighbor)
                        .is_some_and(|target| target.level >= level)
                    {
                        return Err(invalid(
                            "Snapshot neighbor does not exist at the referenced layer",
                        ));
                    }
                }
            }
        }
        Ok(())
    }
}

/// Simple pseudo-random number generator for level selection
/// Uses a simple LCG (Linear Congruential Generator)
fn rand_float() -> f32 {
    use std::cell::Cell;
    use std::time::SystemTime;

    thread_local! {
        static STATE: Cell<u64> = Cell::new(
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(12345)
        );
    }

    STATE.with(|state| {
        // LCG parameters (same as glibc)
        let next = state.get().wrapping_mul(1103515245).wrapping_add(12345);
        state.set(next);
        // Extract bits and convert to float in [0, 1)
        ((next >> 16) & 0x7FFF) as f32 / 32768.0
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contract_requested_count_is_not_silently_capped_by_ef_search() {
        let mut index = HnswIndex::new(HnswConfig {
            ef_search: 1,
            max_level: 0,
            ..Default::default()
        });
        for (id, vector) in [
            ("a", vec![1.0, 0.0]),
            ("b", vec![0.8, 0.2]),
            ("c", vec![0.0, 1.0]),
        ] {
            index.insert(id.into(), vector).unwrap();
        }
        assert_eq!(index.search(&[1.0, 0.0], 3).unwrap().len(), 3);
    }

    #[test]
    fn contract_nonfinite_insert_is_rejected_without_claiming_dimensions() {
        let mut index = HnswIndex::with_defaults();
        assert!(index.insert("invalid".into(), vec![f32::NAN, 1.0]).is_err());
        assert!(index.is_empty());
        assert_eq!(index.config().dimension, None);
    }

    #[test]
    fn contract_large_finite_cosine_vectors_have_finite_self_distance() {
        let mut index = HnswIndex::with_defaults();
        let vector = vec![f32::MAX; 3072];
        index.insert("large".into(), vector.clone()).unwrap();
        let result = index.search(&vector, 1).unwrap();
        assert!(result[0].distance.is_finite());
        assert!(result[0].distance.abs() < 1e-6);
    }

    #[test]
    fn contract_nonempty_snapshot_cannot_lose_its_entry_point() {
        let mut index = HnswIndex::with_defaults();
        index.insert("a".into(), vec![1.0, 0.0]).unwrap();
        let mut persisted: SerializedHnswIndex =
            bincode::deserialize(&index.to_bytes().unwrap()).unwrap();
        persisted.entry_point = None;
        assert!(HnswIndex::from_bytes(&bincode::serialize(&persisted).unwrap()).is_err());
    }

    fn create_test_vectors() -> Vec<(String, Vec<f32>)> {
        vec![
            ("v1".to_string(), vec![1.0, 0.0, 0.0]),
            ("v2".to_string(), vec![0.9, 0.1, 0.0]),
            ("v3".to_string(), vec![0.8, 0.2, 0.0]),
            ("v4".to_string(), vec![0.0, 1.0, 0.0]),
            ("v5".to_string(), vec![0.0, 0.9, 0.1]),
            ("v6".to_string(), vec![0.0, 0.0, 1.0]),
            ("v7".to_string(), vec![0.5, 0.5, 0.0]),
            ("v8".to_string(), vec![0.5, 0.0, 0.5]),
        ]
    }

    #[test]
    fn test_create_index() {
        let index = HnswIndex::with_defaults();
        assert!(index.is_empty());
        assert_eq!(index.len(), 0);
    }

    #[test]
    fn test_insert_single() {
        let mut index = HnswIndex::new(HnswConfig::small());
        let result = index.insert("v1".to_string(), vec![1.0, 0.0, 0.0]);
        assert!(result.is_ok());
        assert_eq!(index.len(), 1);
        assert!(index.contains("v1"));
    }

    #[test]
    fn test_insert_multiple() {
        let mut index = HnswIndex::new(HnswConfig::small());
        let vectors = create_test_vectors();

        for (id, vec) in vectors {
            let result = index.insert(id, vec);
            assert!(result.is_ok());
        }

        assert_eq!(index.len(), 8);
    }

    #[test]
    fn test_dimension_mismatch() {
        let mut index = HnswIndex::new(HnswConfig::small());
        index.insert("v1".to_string(), vec![1.0, 0.0, 0.0]).unwrap();

        let result = index.insert("v2".to_string(), vec![1.0, 0.0]); // Wrong dimension
        assert!(matches!(result, Err(HnswError::DimensionMismatch { .. })));
    }

    #[test]
    fn test_search_basic() {
        let mut index = HnswIndex::new(HnswConfig::small());
        let vectors = create_test_vectors();

        for (id, vec) in vectors {
            index.insert(id, vec).unwrap();
        }

        // Search for vectors similar to [1, 0, 0]
        let query = vec![1.0, 0.0, 0.0];
        let results = index.search(&query, 3).unwrap();

        assert_eq!(results.len(), 3);
        // The closest should be v1 (exact match)
        assert_eq!(results[0].id, "v1");
        assert!(results[0].distance < 0.01); // Very close to 0
    }

    #[test]
    fn test_search_empty() {
        let index = HnswIndex::with_defaults();
        let results = index.search(&[1.0, 0.0, 0.0], 5).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_remove() {
        let mut index = HnswIndex::new(HnswConfig::small());
        index.insert("v1".to_string(), vec![1.0, 0.0, 0.0]).unwrap();
        index.insert("v2".to_string(), vec![0.0, 1.0, 0.0]).unwrap();

        assert_eq!(index.len(), 2);
        assert!(index.contains("v1"));

        let removed = index.remove("v1").unwrap();
        assert!(removed);
        assert_eq!(index.len(), 1);
        assert!(!index.contains("v1"));
        assert!(index.contains("v2"));
    }

    #[test]
    fn test_get() {
        let mut index = HnswIndex::new(HnswConfig::small());
        let vec = vec![1.0, 0.0, 0.0];
        index.insert("v1".to_string(), vec.clone()).unwrap();

        let retrieved = index.get("v1").unwrap();
        assert_eq!(retrieved, vec);

        assert!(index.get("nonexistent").is_none());
    }

    #[test]
    fn test_serialization() {
        let mut index = HnswIndex::new(HnswConfig::small());
        let vectors = create_test_vectors();

        for (id, vec) in vectors {
            index.insert(id, vec).unwrap();
        }

        // Serialize
        let bytes = index.to_bytes().unwrap();

        // Deserialize
        let restored = HnswIndex::from_bytes(&bytes).unwrap();

        assert_eq!(restored.len(), index.len());

        // Verify search still works
        let query = vec![1.0, 0.0, 0.0];
        let results = restored.search(&query, 3).unwrap();
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].id, "v1");
    }

    #[test]
    fn test_different_metrics() {
        // Test with cosine similarity (default)
        let mut cosine_index = HnswIndex::new(HnswConfig::small());
        cosine_index
            .insert("v1".to_string(), vec![1.0, 0.0, 0.0])
            .unwrap();
        cosine_index
            .insert("v2".to_string(), vec![0.7071, 0.7071, 0.0])
            .unwrap();

        let results = cosine_index.search(&[1.0, 0.0, 0.0], 2).unwrap();
        assert_eq!(results[0].id, "v1");

        // Test with euclidean distance
        let mut euclidean_index =
            HnswIndex::new(HnswConfig::small().with_metric(SimilarityMetric::Euclidean));
        euclidean_index
            .insert("v1".to_string(), vec![1.0, 0.0, 0.0])
            .unwrap();
        euclidean_index
            .insert("v2".to_string(), vec![0.9, 0.0, 0.0])
            .unwrap();

        let results = euclidean_index.search(&[1.0, 0.0, 0.0], 2).unwrap();
        assert_eq!(results[0].id, "v1");
    }

    #[test]
    fn test_config_presets() {
        let small = HnswConfig::small();
        assert_eq!(small.m, 8);

        let medium = HnswConfig::medium();
        assert_eq!(medium.m, 16);

        let large = HnswConfig::large();
        assert_eq!(large.m, 32);
    }

    #[test]
    fn test_node_ids() {
        let mut index = HnswIndex::new(HnswConfig::small());
        index.insert("a".to_string(), vec![1.0, 0.0]).unwrap();
        index.insert("b".to_string(), vec![0.0, 1.0]).unwrap();
        index.insert("c".to_string(), vec![0.5, 0.5]).unwrap();

        let ids = index.node_ids();
        assert_eq!(ids.len(), 3);
        assert!(ids.contains(&"a".to_string()));
        assert!(ids.contains(&"b".to_string()));
        assert!(ids.contains(&"c".to_string()));
    }

    #[test]
    fn test_search_quality() {
        // Create a larger test set
        let mut index = HnswIndex::new(HnswConfig::medium());

        // Create 100 random-ish vectors
        for i in 0..100 {
            let angle = (i as f32) * std::f32::consts::PI / 50.0;
            let vec = vec![angle.cos(), angle.sin(), 0.0];
            index.insert(format!("v{}", i), vec).unwrap();
        }

        // Search for a specific vector
        let query = vec![1.0, 0.0, 0.0]; // angle = 0
        let results = index.search(&query, 5).unwrap();

        // v0 should be the closest (angle = 0)
        assert_eq!(results[0].id, "v0");

        // Top results should have small distances
        for result in results.iter().take(5) {
            assert!(result.distance < 0.1);
        }
    }
    #[test]
    fn replacement_preserves_entry_topology_and_snapshot() {
        let mut index = HnswIndex::new(HnswConfig::small());
        for (id, vector) in [
            ("a", vec![1.0, 0.0]),
            ("b", vec![0.0, 1.0]),
            ("c", vec![-1.0, 0.0]),
        ] {
            index.insert(id.into(), vector).unwrap();
        }
        let entry = index.entry_point.read().unwrap().clone().unwrap();
        let old_level = index.nodes.read().unwrap()[&entry].level;
        for _ in 0..20 {
            index.insert(entry.clone(), vec![0.5, 0.5]).unwrap();
            assert_eq!(index.nodes.read().unwrap()[&entry].level, old_level);
            let restored = HnswIndex::from_bytes(&index.to_bytes().unwrap()).unwrap();
            assert_eq!(restored.search(&[0.5, 0.5], 3).unwrap().len(), 3);
            for node in restored.nodes.read().unwrap().values() {
                assert!(node.neighbors.iter().flatten().all(|id| id != &node.id));
            }
        }
        assert!(index.insert(entry.clone(), vec![f32::NAN, 0.0]).is_err());
        assert_eq!(index.nodes.read().unwrap()[&entry].vector, vec![0.5, 0.5]);
    }

    #[test]
    fn extreme_cosine_dimensions_remain_finite() {
        for dimension in [3072, 32768] {
            for value in [f32::MAX, f32::from_bits(1)] {
                let mut index = HnswIndex::new(HnswConfig::small());
                index.insert("x".into(), vec![value; dimension]).unwrap();
                let opposite = vec![-value; dimension];
                assert_eq!(index.search(&opposite, 1).unwrap()[0].distance, 2.0);
                HnswIndex::from_bytes(&index.to_bytes().unwrap()).unwrap();
            }
        }
    }

    #[test]
    fn rejects_structurally_corrupt_snapshots() {
        let mut index = HnswIndex::new(HnswConfig::small());
        index.insert("a".into(), vec![1.0, 0.0]).unwrap();
        index.insert("b".into(), vec![0.0, 1.0]).unwrap();
        let bytes = index.to_bytes().unwrap();
        for case in 0..7 {
            let mut data: SerializedHnswIndex = bincode::deserialize(&bytes).unwrap();
            match case {
                0 => data.config.dimension = Some(3),
                1 => data.nodes.get_mut("a").unwrap().neighbors[0].push("missing".into()),
                2 => data.nodes.get_mut("a").unwrap().neighbors.clear(),
                3 => data.current_max_level += 1,
                4 => data.nodes.get_mut("a").unwrap().vector[0] = f32::INFINITY,
                5 => data.config.ef_search = 0,
                _ => data.nodes.get_mut("a").unwrap().id = "other".into(),
            }
            assert!(
                HnswIndex::from_bytes(&bincode::serialize(&data).unwrap()).is_err(),
                "case {case}"
            );
        }
        let mut trailing = bytes;
        trailing.push(0);
        assert!(HnswIndex::from_bytes(&trailing).is_err());
    }

    #[test]
    fn removal_cleans_asymmetric_incoming_links_and_empty_snapshot() {
        let mut index = HnswIndex::new(HnswConfig::small());
        index.insert("a".into(), vec![1.0, 0.0]).unwrap();
        index.insert("b".into(), vec![0.0, 1.0]).unwrap();
        {
            let mut nodes = index.nodes.write().unwrap();
            nodes.get_mut("a").unwrap().neighbors[0].clear();
            nodes.get_mut("b").unwrap().neighbors[0] = vec!["a".into()];
        }
        index.remove("a").unwrap();
        let restored = HnswIndex::from_bytes(&index.to_bytes().unwrap()).unwrap();
        assert_eq!(restored.search(&[0.0, 1.0], 1).unwrap()[0].id, "b");
        index.remove("b").unwrap();
        assert!(HnswIndex::from_bytes(&index.to_bytes().unwrap())
            .unwrap()
            .is_empty());
    }

    #[test]
    fn unrepresentable_metric_norms_fail_before_dimension_is_claimed() {
        for metric in [SimilarityMetric::DotProduct, SimilarityMetric::Euclidean] {
            let mut index = HnswIndex::new(HnswConfig::small().with_metric(metric));
            assert!(index.insert("invalid".into(), vec![f32::MAX; 2]).is_err());
            assert!(index.config.dimension.is_none());
            assert!(index.is_empty());
            index.insert("valid".into(), vec![1.0e15, 1.0e15]).unwrap();
            assert!(index.search(&[-1.0e15, -1.0e15], 1).unwrap()[0]
                .distance
                .is_finite());
            assert!(index.search(&[f32::MAX; 2], 1).is_err());
        }
    }

    #[test]
    fn reads_original_fixed_width_snapshot_layout() {
        // Original bincode layout, assembled without current Serialize impls.
        fn integer(bytes: &mut Vec<u8>, value: u64) {
            bytes.extend(value.to_le_bytes());
        }
        fn text(bytes: &mut Vec<u8>, value: &str) {
            integer(bytes, value.len() as u64);
            bytes.extend(value.as_bytes());
        }
        let mut bytes = Vec::new();
        for value in [8, 100, 30, 10] {
            integer(&mut bytes, value);
        }
        bytes.push(1); // Some dimension
        integer(&mut bytes, 2);
        bytes.extend(0u32.to_le_bytes()); // Cosine enum
        bytes.extend((1.0 / 8.0_f32.ln()).to_le_bytes());
        integer(&mut bytes, 1); // map size
        text(&mut bytes, "legacy");
        text(&mut bytes, "legacy");
        integer(&mut bytes, 2);
        bytes.extend(1.0_f32.to_le_bytes());
        bytes.extend(0.0_f32.to_le_bytes());
        integer(&mut bytes, 0); // node level
        integer(&mut bytes, 1); // one layer
        integer(&mut bytes, 0); // no neighbors
        bytes.push(1);
        text(&mut bytes, "legacy"); // entry point
        integer(&mut bytes, 0); // maximum level
        let restored = HnswIndex::from_bytes(&bytes).unwrap();
        assert_eq!(restored.search(&[1.0, 0.0], 1).unwrap()[0].id, "legacy");
        assert_eq!(restored.to_bytes().unwrap(), bytes);
    }
    #[test]
    fn invalid_configuration_is_rejected_by_constructor_and_operations() {
        for case in 0..8 {
            let mut config = HnswConfig::small();
            match case {
                0 => config.m = 1,
                1 => config.m = usize::MAX,
                2 => config.ef_search = 0,
                3 => config.ef_construction = 0,
                4 => config.max_level = usize::MAX,
                5 => config.ml = f32::NAN,
                6 => config.ml = 0.0,
                _ => config.dimension = Some(0),
            }
            assert!(HnswIndex::try_new(config.clone()).is_err());
            let mut unchecked = HnswIndex::new(config);
            assert!(unchecked.insert("a".into(), vec![1.0]).is_err());
            assert!(unchecked.search(&[1.0], 1).is_err());
            assert!(unchecked.to_bytes().is_err());
            assert!(unchecked.is_empty());
        }
    }
}
