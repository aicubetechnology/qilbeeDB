//! Memory storage abstraction and implementations
//!
//! Provides persistent storage for agent memories using RocksDB.

use crate::episode::{Episode, EpisodeId};
use crate::relevance_accounting::{RelevanceChange, RelevanceUpdate};
use async_trait::async_trait;
use qilbee_core::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tracing::{debug, info, warn};

mod legacy_episode;
use legacy_episode::LegacyEpisode;

/// Configuration for memory storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryStorageConfig {
    /// Path to the storage directory
    pub path: String,

    /// Enable write-ahead logging for durability
    pub enable_wal: bool,

    /// Sync writes to disk immediately (slower but more durable)
    pub sync_writes: bool,

    /// Maximum write buffer size in bytes
    pub write_buffer_size: usize,

    /// Enable compression for stored data
    pub enable_compression: bool,

    /// Cache size for frequently accessed episodes (in bytes)
    pub cache_size: usize,
}

impl Default for MemoryStorageConfig {
    fn default() -> Self {
        Self {
            path: "data/memory".to_string(),
            enable_wal: true,
            sync_writes: false,
            write_buffer_size: 64 * 1024 * 1024, // 64MB
            enable_compression: true,
            cache_size: 128 * 1024 * 1024, // 128MB
        }
    }
}

impl MemoryStorageConfig {
    /// Create config for testing with temporary directory
    pub fn for_testing(path: &Path) -> Self {
        Self {
            path: path.to_string_lossy().to_string(),
            enable_wal: true,
            sync_writes: false,
            write_buffer_size: 4 * 1024 * 1024, // 4MB for tests
            enable_compression: false,
            cache_size: 16 * 1024 * 1024, // 16MB for tests
        }
    }
}

/// Trait for memory storage backends
///
/// This abstraction allows for different storage implementations:
/// - In-memory (for testing)
/// - RocksDB (for production)
/// - Future: distributed storage
#[async_trait]
pub trait MemoryStorage: Send + Sync {
    /// Store an episode
    async fn store_episode(&self, agent_id: &str, episode: &Episode) -> Result<()>;

    /// Get an episode by ID
    async fn get_episode(&self, agent_id: &str, episode_id: EpisodeId) -> Result<Option<Episode>>;

    /// Get all episodes for an agent
    async fn get_all_episodes(&self, agent_id: &str) -> Result<Vec<Episode>>;

    /// Get episodes in a time range
    async fn get_episodes_in_range(
        &self,
        agent_id: &str,
        start_time_millis: i64,
        end_time_millis: i64,
    ) -> Result<Vec<Episode>>;

    /// Delete an episode
    async fn delete_episode(&self, agent_id: &str, episode_id: EpisodeId) -> Result<bool>;

    /// Delete all episodes for an agent
    async fn delete_all_episodes(&self, agent_id: &str) -> Result<usize>;

    /// Get episode count for an agent
    async fn episode_count(&self, agent_id: &str) -> Result<usize>;

    /// Update an episode (for relevance decay, access tracking, etc.)
    async fn update_episode(&self, agent_id: &str, episode: &Episode) -> Result<()>;

    /// Mutate relevance on the current record atomically. Missing, foreign or
    /// invalidated records return None; unsupported backends never fall back to
    /// an unguarded read/replace cycle.
    async fn apply_relevance_change(&self, _agent_id: &str, _episode_id: EpisodeId,
        _change: RelevanceChange) -> Result<Option<RelevanceUpdate>> {
        Err(Error::MemoryOperation("Backend does not support atomic relevance updates".into()))
    }

    /// Invalidate the current episode without replacing concurrently updated fields.
    /// Unsupported backends fail rather than performing a read/replace cycle.
    async fn invalidate_current_episode(&self, _agent_id: &str, _episode_id: EpisodeId) -> Result<bool> {
        Err(Error::MemoryOperation("Backend does not support atomic episode invalidation".into()))
    }

    /// Flush any pending writes to disk
    async fn flush(&self) -> Result<()>;

    /// Close the storage (for clean shutdown)
    async fn close(&self) -> Result<()>;
}

/// Column family names for memory storage
mod cf {
    /// Episodes indexed by agent_id + timestamp + episode_id
    pub const EPISODES: &str = "memory_episodes";

    /// Episode lookup by ID: episode_id -> agent_id + timestamp
    pub const EPISODE_INDEX: &str = "memory_episode_index";

    /// Agent metadata (episode counts, etc.)
    pub const AGENT_META: &str = "memory_agent_meta";
}

/// Key prefixes for memory storage
mod prefix {
    pub const EPISODE: u8 = 0x01;
    pub const EPISODE_INDEX: u8 = 0x02;
    pub const AGENT_META: u8 = 0x03;
}

/// RocksDB-backed memory storage implementation
pub struct RocksDbMemoryStorage {
    db: Arc<rocksdb::DB>,
    config: MemoryStorageConfig,
    // Serialize index read/modify/write operations, including timestamp moves.
    mutation_lock: Mutex<()>,
    // A restarted or restored storage process cannot accept a previous worker lease.
    consolidation_incarnation: uuid::Uuid,
}

impl RocksDbMemoryStorage {
    /// Open or create a RocksDB-backed memory storage
    pub fn open(config: MemoryStorageConfig) -> Result<Self> {
        info!("Opening memory storage at {}", config.path);

        let mut db_opts = rocksdb::Options::default();
        db_opts.create_if_missing(true);
        db_opts.create_missing_column_families(true);
        db_opts.set_write_buffer_size(config.write_buffer_size);

        // Configure WAL
        if config.enable_wal {
            db_opts.set_wal_dir(&config.path);
        } else {
            db_opts.set_manual_wal_flush(true);
        }

        // Configure compression
        if config.enable_compression {
            db_opts.set_compression_type(rocksdb::DBCompressionType::Lz4);
        }

        // Create column family descriptors
        let cf_names = [cf::EPISODES, cf::EPISODE_INDEX, cf::AGENT_META];
        let cf_descriptors: Vec<rocksdb::ColumnFamilyDescriptor> = cf_names
            .iter()
            .map(|name| {
                let mut cf_opts = rocksdb::Options::default();
                if config.enable_compression {
                    cf_opts.set_compression_type(rocksdb::DBCompressionType::Lz4);
                }
                rocksdb::ColumnFamilyDescriptor::new(*name, cf_opts)
            })
            .collect();

        let db = rocksdb::DB::open_cf_descriptors(&db_opts, &config.path, cf_descriptors)
            .map_err(|e| Error::Storage(format!("Failed to open memory storage: {}", e)))?;

        info!("Memory storage opened successfully");

        let storage = Self {
            db: Arc::new(db),
            config,
            mutation_lock: Mutex::new(()),
            consolidation_incarnation: uuid::Uuid::new_v4(),
        };
        storage.initialize_candidates()?;
        storage.initialize_relation_adjacency()?;
        Ok(storage)
    }

    fn validate_agent(agent_id: &str) -> Result<()> {
        if agent_id.is_empty() || agent_id.len() > u16::MAX as usize {
            return Err(Error::ValidationError(
                "Agent ID must contain 1..=65535 bytes".into(),
            ));
        }
        Ok(())
    }

    fn write_options(&self) -> rocksdb::WriteOptions {
        let mut options = rocksdb::WriteOptions::default();
        options.set_sync(self.config.sync_writes && self.config.enable_wal);
        options.disable_wal(!self.config.enable_wal);
        options
    }

    fn index_location(value: &[u8]) -> Result<(&str, i64)> {
        let invalid = || Error::DataCorruption("Invalid episode index value".into());
        if value.len() < 2 {
            return Err(invalid());
        }
        let len = u16::from_be_bytes([value[0], value[1]]) as usize;
        if value.len() != 2 + len + 8 {
            return Err(invalid());
        }
        let agent = std::str::from_utf8(&value[2..2 + len]).map_err(|_| invalid())?;
        let timestamp = i64::from_be_bytes(value[2 + len..].try_into().map_err(|_| invalid())?);
        Ok((agent, timestamp))
    }

    // Bincode cannot deserialize serde_json::Value. Version the envelope and
    // store that field as JSON text while retaining the binary episode schema.
    // QMEP v2 persists accounting checkpoints. Original raw/v1 layouts are decoded
    // explicitly; their prior accounting remains unknown until adoption.
    fn encode_episode(episode: &Episode) -> Result<Vec<u8>> {
        let mut binary_episode = episode.clone();
        let data = binary_episode
            .content
            .data
            .take()
            .map(|value| serde_json::to_string(&value))
            .transpose()
            .map_err(|e| Error::Serialization(e.to_string()))?;
        let mut value = b"QMEP\0\x02".to_vec();
        value.extend(
            bincode::serialize(&(binary_episode, data))
                .map_err(|e| Error::Serialization(e.to_string()))?,
        );
        Ok(value)
    }

    fn decode_episode(value: &[u8]) -> Result<Episode> {
        if let Some(payload) = value.strip_prefix(b"QMEP\0\x02") {
            let (mut episode, data): (Episode, Option<String>) =
                bincode::deserialize(payload).map_err(|e| Error::Deserialization(e.to_string()))?;
            episode.content.data = data
                .map(|json| serde_json::from_str(&json))
                .transpose()
                .map_err(|e| Error::Deserialization(e.to_string()))?;
            Ok(episode)
        } else if let Some(payload) = value.strip_prefix(b"QMEP\0\x01") {
            let (old, data): (LegacyEpisode, Option<String>) =
                bincode::deserialize(payload).map_err(|e| Error::Deserialization(e.to_string()))?;
            let mut episode: Episode = old.into();
            episode.content.data = data.map(|json| serde_json::from_str(&json)).transpose()
                .map_err(|e| Error::Deserialization(e.to_string()))?;
            Ok(episode)
        } else if value.starts_with(b"QMEP") {
            Err(Error::Deserialization(
                "Unsupported episode record version".into(),
            ))
        } else {
            bincode::deserialize::<LegacyEpisode>(value)
                .map(Into::into).map_err(|e| Error::Deserialization(e.to_string()))
        }
    }

    /// Build episode key: agent_id + timestamp + episode_id
    fn episode_key(agent_id: &str, event_time_millis: i64, episode_id: EpisodeId) -> Vec<u8> {
        let mut key = Vec::with_capacity(1 + 2 + agent_id.len() + 8 + 16);
        key.push(prefix::EPISODE);
        // Length-prefixed agent_id
        let agent_bytes = agent_id.as_bytes();
        key.extend_from_slice(&(agent_bytes.len() as u16).to_be_bytes());
        key.extend_from_slice(agent_bytes);
        // Timestamp for time-ordered iteration
        key.extend_from_slice(&event_time_millis.to_be_bytes());
        // Episode UUID
        key.extend_from_slice(episode_id.as_uuid().as_bytes());
        key
    }

    /// Build episode prefix for scanning all episodes of an agent
    fn episode_prefix(agent_id: &str) -> Vec<u8> {
        let mut key = Vec::with_capacity(1 + 2 + agent_id.len());
        key.push(prefix::EPISODE);
        let agent_bytes = agent_id.as_bytes();
        key.extend_from_slice(&(agent_bytes.len() as u16).to_be_bytes());
        key.extend_from_slice(agent_bytes);
        key
    }

    /// Build episode index key: episode_id -> location info
    fn episode_index_key(episode_id: EpisodeId) -> Vec<u8> {
        let mut key = Vec::with_capacity(1 + 16);
        key.push(prefix::EPISODE_INDEX);
        key.extend_from_slice(episode_id.as_uuid().as_bytes());
        key
    }

    /// Build agent metadata key
    fn agent_meta_key(agent_id: &str) -> Vec<u8> {
        let mut key = Vec::with_capacity(1 + 2 + agent_id.len());
        key.push(prefix::AGENT_META);
        let agent_bytes = agent_id.as_bytes();
        key.extend_from_slice(&(agent_bytes.len() as u16).to_be_bytes());
        key.extend_from_slice(agent_bytes);
        key
    }

    /// Get column family handle
    fn cf(&self, name: &str) -> Result<&rocksdb::ColumnFamily> {
        self.db
            .cf_handle(name)
            .ok_or_else(|| Error::Internal(format!("Column family not found: {}", name)))
    }
}

#[async_trait]
impl MemoryStorage for RocksDbMemoryStorage {
    async fn store_episode(&self, agent_id: &str, episode: &Episode) -> Result<()> {
        Self::validate_agent(agent_id)?;
        if episode.agent_id != agent_id {
            return Err(Error::ValidationError(
                "Episode agent does not match storage scope".into(),
            ));
        }
        let _guard = self
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Memory mutation lock poisoned".into()))?;
        let episodes_cf = self.cf(cf::EPISODES)?;
        let index_cf = self.cf(cf::EPISODE_INDEX)?;

        // Serialize episode
        let value = Self::encode_episode(episode)?;

        // Build keys
        let episode_key = Self::episode_key(agent_id, episode.event_time.as_millis(), episode.id);
        let index_key = Self::episode_index_key(episode.id);

        // Index value: agent_id + timestamp for lookups
        let mut index_value = Vec::new();
        let agent_bytes = agent_id.as_bytes();
        index_value.extend_from_slice(&(agent_bytes.len() as u16).to_be_bytes());
        index_value.extend_from_slice(agent_bytes);
        index_value.extend_from_slice(&episode.event_time.as_millis().to_be_bytes());

        // Write batch for atomicity
        let mut batch = rocksdb::WriteBatch::default();
        if let Some(previous) = self
            .db
            .get_cf(index_cf, &index_key)
            .map_err(|e| Error::Storage(e.to_string()))?
        {
            let (owner, timestamp) = Self::index_location(&previous)?;
            if owner != agent_id {
                return Err(Error::ConstraintViolation(
                    "Episode ID already belongs to another agent".into(),
                ));
            }
            if timestamp != episode.event_time.as_millis() {
                batch.delete_cf(episodes_cf, Self::episode_key(owner, timestamp, episode.id));
            }
        }
        batch.put_cf(episodes_cf, &episode_key, &value);
        batch.put_cf(index_cf, &index_key, &index_value);

        self.db
            .write_opt(batch, &self.write_options())
            .map_err(|e| Error::Storage(format!("Failed to store episode: {}", e)))?;

        debug!("Stored episode {} for agent {}", episode.id, agent_id);

        Ok(())
    }

    async fn get_episode(&self, agent_id: &str, episode_id: EpisodeId) -> Result<Option<Episode>> {
        Self::validate_agent(agent_id)?;
        let _guard = self
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Memory mutation lock poisoned".into()))?;
        let index_cf = self.cf(cf::EPISODE_INDEX)?;
        let episodes_cf = self.cf(cf::EPISODES)?;

        // First lookup the index to get agent_id and timestamp
        let index_key = Self::episode_index_key(episode_id);
        let index_value = match self.db.get_cf(index_cf, &index_key) {
            Ok(Some(v)) => v,
            Ok(None) => return Ok(None),
            Err(e) => return Err(Error::Storage(format!("Failed to read index: {}", e))),
        };

        let (owner, timestamp) = Self::index_location(&index_value)?;
        if owner != agent_id {
            return Ok(None);
        }

        // Now read the actual episode
        let episode_key = Self::episode_key(agent_id, timestamp, episode_id);
        match self.db.get_cf(episodes_cf, &episode_key) {
            Ok(Some(value)) => {
                let episode = Self::decode_episode(&value)?;
                Ok(Some(episode))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(Error::Storage(format!("Failed to read episode: {}", e))),
        }
    }

    async fn get_all_episodes(&self, agent_id: &str) -> Result<Vec<Episode>> {
        Self::validate_agent(agent_id)?;
        let episodes_cf = self.cf(cf::EPISODES)?;
        let prefix = Self::episode_prefix(agent_id);

        let mut episodes = Vec::new();
        let iter = self.db.prefix_iterator_cf(episodes_cf, &prefix);

        for item in iter {
            let (key, value) = item.map_err(|e| Error::Storage(e.to_string()))?;

            // Check if still in prefix
            if !key.starts_with(&prefix) {
                break;
            }

            let episode = Self::decode_episode(&value)?;

            // Only include valid episodes
            if episode.is_valid() {
                episodes.push(episode);
            }
        }

        Ok(episodes)
    }

    async fn get_episodes_in_range(
        &self,
        agent_id: &str,
        start_time_millis: i64,
        end_time_millis: i64,
    ) -> Result<Vec<Episode>> {
        Self::validate_agent(agent_id)?;
        let episodes_cf = self.cf(cf::EPISODES)?;
        let prefix = Self::episode_prefix(agent_id);

        let mut episodes = Vec::new();
        let iter = self.db.prefix_iterator_cf(episodes_cf, &prefix);

        for item in iter {
            let (key, value) = item.map_err(|e| Error::Storage(e.to_string()))?;

            // Check if still in prefix
            if !key.starts_with(&prefix) {
                break;
            }

            let episode = Self::decode_episode(&value)?;

            // Filter by time range and validity
            let event_millis = episode.event_time.as_millis();
            if episode.is_valid()
                && event_millis >= start_time_millis
                && event_millis <= end_time_millis
            {
                episodes.push(episode);
            }
        }

        Ok(episodes)
    }

    async fn delete_episode(&self, agent_id: &str, episode_id: EpisodeId) -> Result<bool> {
        Self::validate_agent(agent_id)?;
        let _guard = self
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Memory mutation lock poisoned".into()))?;
        let index_cf = self.cf(cf::EPISODE_INDEX)?;
        let episodes_cf = self.cf(cf::EPISODES)?;

        // First lookup the index to get timestamp
        let index_key = Self::episode_index_key(episode_id);
        let index_value = match self.db.get_cf(index_cf, &index_key) {
            Ok(Some(v)) => v,
            Ok(None) => return Ok(false),
            Err(e) => return Err(Error::Storage(format!("Failed to read index: {}", e))),
        };

        let (owner, timestamp) = Self::index_location(&index_value)?;
        if owner != agent_id {
            return Ok(false);
        }

        // Delete both episode and index
        let episode_key = Self::episode_key(agent_id, timestamp, episode_id);

        let mut batch = rocksdb::WriteBatch::default();
        batch.delete_cf(episodes_cf, &episode_key);
        batch.delete_cf(index_cf, &index_key);

        self.db
            .write_opt(batch, &self.write_options())
            .map_err(|e| Error::Storage(format!("Failed to delete episode: {}", e)))?;

        debug!("Deleted episode {} for agent {}", episode_id, agent_id);

        Ok(true)
    }

    async fn delete_all_episodes(&self, agent_id: &str) -> Result<usize> {
        Self::validate_agent(agent_id)?;
        let _guard = self
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Memory mutation lock poisoned".into()))?;
        let episodes_cf = self.cf(cf::EPISODES)?;
        let index_cf = self.cf(cf::EPISODE_INDEX)?;
        let prefix = Self::episode_prefix(agent_id);

        let mut batch = rocksdb::WriteBatch::default();
        let mut count = 0;

        let iter = self.db.prefix_iterator_cf(episodes_cf, &prefix);

        for item in iter {
            let (key, value) = item.map_err(|e| Error::Storage(e.to_string()))?;

            if !key.starts_with(&prefix) {
                break;
            }

            // Parse episode to get ID for index deletion
            let episode = Self::decode_episode(&value)?;

            let index_key = Self::episode_index_key(episode.id);

            batch.delete_cf(episodes_cf, &key);
            batch.delete_cf(index_cf, &index_key);
            count += 1;
        }

        if count > 0 {
            self.db
                .write_opt(batch, &self.write_options())
                .map_err(|e| Error::Storage(format!("Failed to delete episodes: {}", e)))?;

            info!("Deleted {} episodes for agent {}", count, agent_id);
        }

        Ok(count)
    }

    async fn episode_count(&self, agent_id: &str) -> Result<usize> {
        Self::validate_agent(agent_id)?;
        let episodes_cf = self.cf(cf::EPISODES)?;
        let prefix = Self::episode_prefix(agent_id);

        let mut count = 0;
        let iter = self.db.prefix_iterator_cf(episodes_cf, &prefix);

        for item in iter {
            let (key, value) = item.map_err(|e| Error::Storage(e.to_string()))?;

            if !key.starts_with(&prefix) {
                break;
            }

            // Only count valid episodes
            let episode = Self::decode_episode(&value)?;

            if episode.is_valid() {
                count += 1;
            }
        }

        Ok(count)
    }

    async fn update_episode(&self, agent_id: &str, episode: &Episode) -> Result<()> {
        // Update is same as store - it will overwrite the existing episode
        self.store_episode(agent_id, episode).await
    }

    async fn apply_relevance_change(&self, agent_id: &str, episode_id: EpisodeId,
        change: RelevanceChange) -> Result<Option<RelevanceUpdate>> {
        Self::validate_agent(agent_id)?;
        let _guard = self.mutation_lock.lock()
            .map_err(|_| Error::Internal("Memory mutation lock poisoned".into()))?;
        let index = self.db.get_cf(self.cf(cf::EPISODE_INDEX)?, Self::episode_index_key(episode_id))
            .map_err(|e| Error::Storage(e.to_string()))?;
        let Some(index) = index else { return Ok(None); };
        let (owner, timestamp) = Self::index_location(&index)?;
        if owner != agent_id { return Ok(None); }
        let key = Self::episode_key(owner, timestamp, episode_id);
        let records = self.cf(cf::EPISODES)?;
        let Some(bytes) = self.db.get_cf(records, &key).map_err(|e| Error::Storage(e.to_string()))?
            else { return Ok(None); };
        let mut episode = Self::decode_episode(&bytes)?;
        if episode.id != episode_id || episode.agent_id != agent_id || episode.event_time.as_millis() != timestamp {
            return Err(Error::DataCorruption("Episode identity disagrees with its index".into()));
        }
        if !episode.is_valid() { return Ok(None); }
        let outcome = change.apply(&mut episode.relevance)?;
        let mut batch = rocksdb::WriteBatch::default();
        batch.put_cf(records, key, Self::encode_episode(&episode)?);
        self.db.write_opt(batch, &self.write_options()).map_err(|e| Error::Storage(e.to_string()))?;
        Ok(Some(RelevanceUpdate {episode, outcome}))
    }

    async fn invalidate_current_episode(&self, agent_id: &str, episode_id: EpisodeId) -> Result<bool> {
        Self::validate_agent(agent_id)?;
        let _guard = self.mutation_lock.lock()
            .map_err(|_| Error::Internal("Memory mutation lock poisoned".into()))?;
        let Some(index) = self.db.get_cf(self.cf(cf::EPISODE_INDEX)?, Self::episode_index_key(episode_id))
            .map_err(|e| Error::Storage(e.to_string()))? else { return Ok(false); };
        let (owner, timestamp) = Self::index_location(&index)?;
        if owner != agent_id { return Ok(false); }
        let key = Self::episode_key(owner, timestamp, episode_id);
        let records = self.cf(cf::EPISODES)?;
        let Some(bytes) = self.db.get_cf(records, &key).map_err(|e| Error::Storage(e.to_string()))?
            else { return Ok(false); };
        let mut episode = Self::decode_episode(&bytes)?;
        if episode.id != episode_id || episode.agent_id != agent_id || episode.event_time.as_millis() != timestamp {
            return Err(Error::DataCorruption("Episode identity disagrees with its index".into()));
        }
        if episode.is_valid() {
            episode.invalidate();
            self.db.put_cf_opt(records, key, Self::encode_episode(&episode)?, &self.write_options())
                .map_err(|e| Error::Storage(e.to_string()))?;
        }
        Ok(true)
    }

    async fn flush(&self) -> Result<()> {
        for name in [cf::EPISODES, cf::EPISODE_INDEX, cf::AGENT_META] {
            self.db
                .flush_cf(self.cf(name)?)
                .map_err(|e| Error::Storage(format!("Failed to flush: {}", e)))?;
        }
        debug!("Memory storage flushed");
        Ok(())
    }

    async fn close(&self) -> Result<()> {
        self.flush().await?;
        info!("Memory storage closed");
        Ok(())
    }
}

/// In-memory storage implementation for testing
pub struct InMemoryStorage {
    episodes: tokio::sync::RwLock<HashMap<String, HashMap<EpisodeId, Episode>>>,
}

impl InMemoryStorage {
    /// Create a new in-memory storage
    pub fn new() -> Self {
        Self {
            episodes: tokio::sync::RwLock::new(HashMap::new()),
        }
    }
}

impl Default for InMemoryStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl MemoryStorage for InMemoryStorage {
    async fn store_episode(&self, agent_id: &str, episode: &Episode) -> Result<()> {
        let mut episodes = self.episodes.write().await;
        let agent_episodes = episodes.entry(agent_id.to_string()).or_default();
        agent_episodes.insert(episode.id, episode.clone());
        Ok(())
    }

    async fn get_episode(&self, agent_id: &str, episode_id: EpisodeId) -> Result<Option<Episode>> {
        let episodes = self.episodes.read().await;
        Ok(episodes
            .get(agent_id)
            .and_then(|m| m.get(&episode_id))
            .cloned())
    }

    async fn get_all_episodes(&self, agent_id: &str) -> Result<Vec<Episode>> {
        let episodes = self.episodes.read().await;
        Ok(episodes
            .get(agent_id)
            .map(|m| m.values().filter(|e| e.is_valid()).cloned().collect())
            .unwrap_or_default())
    }

    async fn get_episodes_in_range(
        &self,
        agent_id: &str,
        start_time_millis: i64,
        end_time_millis: i64,
    ) -> Result<Vec<Episode>> {
        let episodes = self.episodes.read().await;
        Ok(episodes
            .get(agent_id)
            .map(|m| {
                m.values()
                    .filter(|e| {
                        e.is_valid()
                            && e.event_time.as_millis() >= start_time_millis
                            && e.event_time.as_millis() <= end_time_millis
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default())
    }

    async fn delete_episode(&self, agent_id: &str, episode_id: EpisodeId) -> Result<bool> {
        let mut episodes = self.episodes.write().await;
        Ok(episodes
            .get_mut(agent_id)
            .map(|m| m.remove(&episode_id).is_some())
            .unwrap_or(false))
    }

    async fn delete_all_episodes(&self, agent_id: &str) -> Result<usize> {
        let mut episodes = self.episodes.write().await;
        Ok(episodes.remove(agent_id).map(|m| m.len()).unwrap_or(0))
    }

    async fn episode_count(&self, agent_id: &str) -> Result<usize> {
        let episodes = self.episodes.read().await;
        Ok(episodes
            .get(agent_id)
            .map(|m| m.values().filter(|e| e.is_valid()).count())
            .unwrap_or(0))
    }

    async fn update_episode(&self, agent_id: &str, episode: &Episode) -> Result<()> {
        self.store_episode(agent_id, episode).await
    }

    async fn apply_relevance_change(&self, agent_id: &str, episode_id: EpisodeId,
        change: RelevanceChange) -> Result<Option<RelevanceUpdate>> {
        let mut episodes = self.episodes.write().await;
        let Some(episode) = episodes.get_mut(agent_id).and_then(|records| records.get_mut(&episode_id))
            else { return Ok(None); };
        if episode.agent_id != agent_id { return Err(Error::DataCorruption("Episode owner disagrees with storage scope".into())); }
        if !episode.is_valid() { return Ok(None); }
        let outcome = change.apply(&mut episode.relevance)?;
        Ok(Some(RelevanceUpdate {episode: episode.clone(), outcome}))
    }

    async fn invalidate_current_episode(&self, agent_id: &str, episode_id: EpisodeId) -> Result<bool> {
        let mut episodes = self.episodes.write().await;
        let Some(episode) = episodes.get_mut(agent_id).and_then(|records| records.get_mut(&episode_id))
            else { return Ok(false); };
        if episode.agent_id != agent_id { return Err(Error::DataCorruption("Episode owner disagrees with storage scope".into())); }
        if episode.is_valid() { episode.invalidate(); }
        Ok(true)
    }

    async fn flush(&self) -> Result<()> {
        Ok(())
    }

    async fn close(&self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::episode::{Episode, EpisodeContent, EpisodeType};
    use tempfile::TempDir;

    async fn assert_atomic_relevance_contract(storage: Arc<dyn MemoryStorage>) {
        use crate::relevance_accounting::AccountedRelevance;
        let mut episode = Episode::observation("owner", "Preserve current content");
        episode.relevance = AccountedRelevance::new_at(0);
        episode.content.data = Some(serde_json::json!({"evidence": ["original", 42]}));
        storage.store_episode("owner", &episode).await.unwrap();
        let barrier = Arc::new(tokio::sync::Barrier::new(16));
        let mut tasks = Vec::new();
        for _ in 0..16 {
            let storage = storage.clone();
            let barrier = barrier.clone();
            let id = episode.id;
            tasks.push(tokio::spawn(async move {
                barrier.wait().await;
                storage.apply_relevance_change("owner", id,
                    RelevanceChange::Access { at_millis: 3_600_000, boost: 0.0 })
                    .await.unwrap().unwrap();
            }));
        }
        for task in tasks { task.await.unwrap(); }
        let current = storage.get_episode("owner", episode.id).await.unwrap().unwrap();
        assert_eq!(current.relevance.access_count, 16);
        assert_eq!(current.content.data, episode.content.data);
        assert_eq!(current.content.primary, episode.content.primary);
        let before = serde_json::to_value(&current).unwrap();
        for change in [
            RelevanceChange::Decay { at_millis: 0, rate_per_hour: 0.1 },
            RelevanceChange::Decay { at_millis: 3_600_000, rate_per_hour: f64::NAN },
            RelevanceChange::Access { at_millis: 3_600_000, boost: f64::INFINITY },
        ] {
            assert!(storage.apply_relevance_change("owner", episode.id, change).await.is_err());
            let after = storage.get_episode("owner", episode.id).await.unwrap().unwrap();
            assert_eq!(serde_json::to_value(after).unwrap(), before);
        }
        let change = RelevanceChange::Decay { at_millis: 3_600_000, rate_per_hour: 0.1 };
        assert!(storage.apply_relevance_change("foreign", episode.id, change).await.unwrap().is_none());
        assert!(storage.apply_relevance_change("owner", EpisodeId::new(), change).await.unwrap().is_none());
        let mut invalidated = current;
        invalidated.invalidate();
        storage.update_episode("owner", &invalidated).await.unwrap();
        assert!(storage.apply_relevance_change("owner", episode.id, change).await.unwrap().is_none());
        assert_eq!(serde_json::to_value(storage.get_episode("owner", episode.id).await.unwrap().unwrap()).unwrap(),
            serde_json::to_value(invalidated).unwrap());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn atomic_relevance_rocksdb_concurrent_access_and_rejected_changes() {
        let (storage, _dir) = create_test_storage().await;
        assert_atomic_relevance_contract(Arc::new(storage)).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn atomic_relevance_in_memory_concurrent_access_and_rejected_changes() {
        assert_atomic_relevance_contract(Arc::new(InMemoryStorage::new())).await;
    }

    async fn assert_access_invalidation_race(storage: Arc<dyn MemoryStorage>) {
        use crate::relevance_accounting::AccountedRelevance;
        for _ in 0..8 {
            let mut episode = Episode::observation("owner", "Concurrent invalidation fixture");
            episode.relevance = AccountedRelevance::new_at(0);
            storage.store_episode("owner", &episode).await.unwrap();
            let barrier = Arc::new(tokio::sync::Barrier::new(17));
            let mut tasks = Vec::new();
            for _ in 0..16 {
                let storage = storage.clone();
                let barrier = barrier.clone();
                let id = episode.id;
                tasks.push(tokio::spawn(async move {
                    barrier.wait().await;
                    storage.apply_relevance_change("owner", id,
                        RelevanceChange::Access { at_millis: 0, boost: 0.0 })
                        .await.unwrap().is_some()
                }));
            }
            let invalidator = {
                let storage = storage.clone();
                let barrier = barrier.clone();
                let id = episode.id;
                tokio::spawn(async move {
                    barrier.wait().await;
                    assert!(storage.invalidate_current_episode("owner", id).await.unwrap());
                })
            };
            let mut successful = 0;
            for task in tasks { successful += u32::from(task.await.unwrap()); }
            invalidator.await.unwrap();
            let current = storage.get_episode("owner", episode.id).await.unwrap().unwrap();
            assert!(!current.is_valid());
            assert_eq!(current.relevance.access_count, successful,
                "Invalidation must preserve every committed access");
            assert!(!storage.invalidate_current_episode("foreign", episode.id).await.unwrap());
            assert!(storage.invalidate_current_episode("owner", episode.id).await.unwrap());
            let repeated = storage.get_episode("owner", episode.id).await.unwrap().unwrap();
            assert_eq!(serde_json::to_value(current).unwrap(), serde_json::to_value(repeated).unwrap());
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn atomic_relevance_rocksdb_invalidation_preserves_committed_accesses() {
        let (storage, _dir) = create_test_storage().await;
        assert_access_invalidation_race(Arc::new(storage)).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn atomic_relevance_in_memory_invalidation_preserves_committed_accesses() {
        assert_access_invalidation_race(Arc::new(InMemoryStorage::new())).await;
    }

    #[tokio::test]
    async fn atomic_relevance_checkpoint_survives_reopen_without_double_decay() {
        use crate::relevance_accounting::AccountedRelevance;
        let dir = TempDir::new().unwrap();
        let mut config = MemoryStorageConfig::for_testing(dir.path());
        config.sync_writes = true;
        let mut episode = Episode::observation("owner", "Restart fixture");
        episode.relevance = AccountedRelevance::new_at(0);
        let change = RelevanceChange::Decay { at_millis: 86_400_000, rate_per_hour: 0.1 };
        let before = {
            let storage = RocksDbMemoryStorage::open(config.clone()).unwrap();
            storage.store_episode("owner", &episode).await.unwrap();
            storage.apply_relevance_change("owner", episode.id, change).await.unwrap().unwrap().episode
        };
        let storage = RocksDbMemoryStorage::open(config).unwrap();
        let after = storage.apply_relevance_change("owner", episode.id, change).await.unwrap().unwrap();
        assert_eq!(after.outcome.decayed_millis, 0);
        assert_eq!(serde_json::to_value(after.episode).unwrap(), serde_json::to_value(before).unwrap());
    }

    #[tokio::test]
    async fn atomic_relevance_storage_write_failure_preserves_record() {
        use crate::relevance_accounting::AccountedRelevance;
        let dir = TempDir::new().unwrap();
        let config = MemoryStorageConfig::for_testing(dir.path());
        let mut storage = RocksDbMemoryStorage::open(config.clone()).unwrap();
        let mut episode = Episode::observation("owner", "Read-only write failure");
        episode.relevance = AccountedRelevance::new_at(0);
        storage.store_episode("owner", &episode).await.unwrap();
        storage.flush().await.unwrap();
        // Use RocksDB's real read-only writer rejection, not a mocked success path.
        let read_only = rocksdb::DB::open_cf_for_read_only(&rocksdb::Options::default(),
            dir.path(), [cf::EPISODES, cf::EPISODE_INDEX, cf::AGENT_META], false).unwrap();
        storage.db = Arc::new(read_only);
        let change = RelevanceChange::Decay { at_millis: 3_600_000, rate_per_hour: 0.1 };
        assert!(matches!(storage.apply_relevance_change("owner", episode.id, change).await, Err(Error::Storage(_))));
        assert!(matches!(storage.invalidate_current_episode("owner", episode.id).await, Err(Error::Storage(_))));
        let actual = storage.get_episode("owner", episode.id).await.unwrap().unwrap();
        assert_eq!(serde_json::to_value(actual).unwrap(), serde_json::to_value(&episode).unwrap());
        drop(storage);
        let storage = RocksDbMemoryStorage::open(config).unwrap();
        let actual = storage.get_episode("owner", episode.id).await.unwrap().unwrap();
        assert_eq!(serde_json::to_value(actual).unwrap(), serde_json::to_value(episode).unwrap());
    }

    #[tokio::test]
    async fn atomic_relevance_acknowledgement_survives_killed_process() {
        use crate::relevance_accounting::AccountedRelevance;
        const CHILD_DIRECTORY: &str = "QILBEEDB_RELEVANCE_CRASH_FIXTURE";
        if let Some(directory) = std::env::var_os(CHILD_DIRECTORY) {
            let directory = std::path::PathBuf::from(directory);
            let mut config = MemoryStorageConfig::for_testing(&directory.join("database"));
            config.sync_writes = true;
            let storage = RocksDbMemoryStorage::open(config).unwrap();
            let mut episode = Episode::observation("owner", "Killed-process recovery");
            episode.relevance = AccountedRelevance::new_at(0);
            storage.store_episode("owner", &episode).await.unwrap();
            let update = storage.apply_relevance_change("owner", episode.id,
                RelevanceChange::Decay { at_millis: 86_400_000, rate_per_hour: 0.1 })
                .await.unwrap().unwrap();
            use std::io::Write;
            let temporary = directory.join("acknowledged.tmp");
            let mut file = std::fs::File::create(&temporary).unwrap();
            file.write_all(&RocksDbMemoryStorage::encode_episode(&update.episode).unwrap()).unwrap();
            file.sync_all().unwrap();
            std::fs::rename(temporary, directory.join("acknowledged.bin")).unwrap();
            // Parent kills this process while RocksDB and the runtime remain alive.
            loop { std::thread::park(); }
        }
        let directory = TempDir::new().unwrap();
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "storage::tests::atomic_relevance_acknowledgement_survives_killed_process", "--nocapture"])
            .env(CHILD_DIRECTORY, directory.path())
            .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::piped())
            .spawn().unwrap();
        let acknowledged = directory.path().join("acknowledged.bin");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while !acknowledged.exists() && std::time::Instant::now() < deadline {
            if child.try_wait().unwrap().is_some() { break; }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let ready = acknowledged.exists();
        let _ = child.kill();
        let output = child.wait_with_output().unwrap();
        assert!(ready, "Child did not acknowledge: {}", String::from_utf8_lossy(&output.stderr));
        #[cfg(unix)] {
            use std::os::unix::process::ExitStatusExt;
            assert_eq!(output.status.signal(), Some(9));
        }
        let before = RocksDbMemoryStorage::decode_episode(&std::fs::read(acknowledged).unwrap()).unwrap();
        let storage = RocksDbMemoryStorage::open(MemoryStorageConfig::for_testing(&directory.path().join("database"))).unwrap();
        let actual = storage.get_episode("owner", before.id).await.unwrap().unwrap();
        assert_eq!(serde_json::to_value(&actual).unwrap(), serde_json::to_value(&before).unwrap());
        let replay = storage.apply_relevance_change("owner", before.id,
            RelevanceChange::Decay { at_millis: 86_400_000, rate_per_hour: 0.1 })
            .await.unwrap().unwrap();
        assert_eq!(replay.outcome.decayed_millis, 0);
        assert_eq!(serde_json::to_value(replay.episode).unwrap(), serde_json::to_value(before).unwrap());
    }

    #[tokio::test]
    async fn regression_wrong_agent_cannot_delete_another_agents_index() {
        let (storage, _dir) = create_test_storage().await;
        let episode = Episode::observation("owner", "Keep this memory");
        storage.store_episode("owner", &episode).await.unwrap();
        assert!(!storage.delete_episode("other", episode.id).await.unwrap());
        assert!(
            storage
                .get_episode("owner", episode.id)
                .await
                .unwrap()
                .is_some()
        );
    }

    #[tokio::test]
    async fn regression_episode_cannot_change_owner_or_impersonate_agent() {
        let (storage, _dir) = create_test_storage().await;
        let mut episode = Episode::observation("owner", "Original");
        assert!(storage.store_episode("other", &episode).await.is_err());
        storage.store_episode("owner", &episode).await.unwrap();
        episode.agent_id = "other".into();
        assert!(storage.store_episode("other", &episode).await.is_err());
        assert!(
            storage
                .get_episode("owner", episode.id)
                .await
                .unwrap()
                .is_some()
        );
        assert!(storage.get_all_episodes("other").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn regression_event_time_update_removes_old_row() {
        let (storage, _dir) = create_test_storage().await;
        let mut episode = Episode::observation("owner", "Before");
        storage.store_episode("owner", &episode).await.unwrap();
        episode.event_time = qilbee_core::temporal::EventTime::from_millis(42);
        episode.content.primary = "After".into();
        storage.update_episode("owner", &episode).await.unwrap();
        let episodes = storage.get_all_episodes("owner").await.unwrap();
        assert_eq!(episodes.len(), 1);
        assert_eq!(episodes[0].content.primary, "After");
        assert_eq!(storage.delete_all_episodes("owner").await.unwrap(), 1);
    }

    #[tokio::test]
    async fn regression_structured_episode_survives_restart() {
        let dir = TempDir::new().unwrap();
        let config = MemoryStorageConfig::for_testing(dir.path());
        let mut episode = Episode::task_execution("owner", "Solve", "Verified");
        episode.content.data = Some(serde_json::json!({
            "score": 0.9, "evidence": [true, null, {"source": "日本語"}]
        }));
        {
            let storage = RocksDbMemoryStorage::open(config.clone()).unwrap();
            storage.store_episode("owner", &episode).await.unwrap();
        }
        let storage = RocksDbMemoryStorage::open(config).unwrap();
        let restored = storage
            .get_episode("owner", episode.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(restored.content.data, episode.content.data);
        assert_eq!(storage.episode_count("owner").await.unwrap(), 1);
    }

    #[tokio::test]
    async fn regression_truncated_index_returns_error_instead_of_panicking() {
        let (storage, _dir) = create_test_storage().await;
        let episode = Episode::observation("owner", "Memory");
        storage.store_episode("owner", &episode).await.unwrap();
        let mut corrupt = vec![0; 10];
        corrupt[1] = 100;
        storage
            .db
            .put_cf(
                storage.cf(cf::EPISODE_INDEX).unwrap(),
                RocksDbMemoryStorage::episode_index_key(episode.id),
                corrupt,
            )
            .unwrap();
        assert!(storage.delete_episode("owner", episode.id).await.is_err());
    }

    fn original_relevance_fixture(key: &str) -> Vec<u8> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!("storage/fixtures/relevance-v1.json")).unwrap();
        let hex = fixture[key].as_str().unwrap();
        (0..hex.len()).step_by(2).map(|i|u8::from_str_radix(&hex[i..i+2],16).unwrap()).collect()
    }

    #[test]
    fn original_raw_and_v1_relevance_preserve_fields_and_adopt_into_v2() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!("storage/fixtures/relevance-v1.json")).unwrap();
        for (bytes, expected) in [("raw_hex","raw_expected"),("v1_hex","v1_expected")] {
            let mut episode = RocksDbMemoryStorage::decode_episode(&original_relevance_fixture(bytes)).unwrap();
            assert!(episode.relevance.checkpoint.is_none());
            let mut actual = serde_json::to_value(&episode).unwrap();
            actual["relevance"].as_object_mut().unwrap().remove("checkpoint");
            assert_eq!(actual, fixture[expected]);
            let outcome = episode.relevance.decay_at(1700000000300,0.1).unwrap();
            assert!(outcome.adopted_legacy);assert_eq!(episode.relevance.score,0.42);
            let encoded = RocksDbMemoryStorage::encode_episode(&episode).unwrap();
            assert!(encoded.starts_with(b"QMEP\0\x02"));
            let reopened = RocksDbMemoryStorage::decode_episode(&encoded).unwrap();
            assert_eq!(serde_json::to_value(reopened).unwrap(),serde_json::to_value(episode).unwrap());
        }
    }

    #[tokio::test]
    async fn regression_legacy_binary_episode_remains_readable_and_updatable() {
        let (storage, _dir) = create_test_storage().await;
        let bytes = original_relevance_fixture("raw_hex");
        let mut episode = RocksDbMemoryStorage::decode_episode(&bytes).unwrap();
        assert!(episode.relevance.checkpoint.is_none());
        storage.store_episode("legacy-owner", &episode).await.unwrap();
        let key =
            RocksDbMemoryStorage::episode_key("legacy-owner", episode.event_time.as_millis(), episode.id);
        storage
            .db
            .put_cf(
                storage.cf(cf::EPISODES).unwrap(),
                key,
                bytes,
            )
            .unwrap();
        assert_eq!(
            storage
                .get_episode("legacy-owner", episode.id)
                .await
                .unwrap()
                .unwrap()
                .content
                .primary,
            "Legacy relevance fixture"
        );
        episode.content.data = Some(serde_json::json!({"updated": true}));
        storage.update_episode("legacy-owner", &episode).await.unwrap();
        assert_eq!(
            storage.get_all_episodes("legacy-owner").await.unwrap()[0]
                .content
                .data,
            episode.content.data
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn regression_concurrent_timestamp_moves_keep_one_row() {
        let (storage, _dir) = create_test_storage().await;
        let storage = Arc::new(storage);
        let episode = Episode::observation("owner", "Moving event");
        let mut tasks = Vec::new();
        for millis in 0..32 {
            let storage = storage.clone();
            let mut episode = episode.clone();
            episode.event_time = qilbee_core::temporal::EventTime::from_millis(millis);
            tasks.push(tokio::spawn(async move {
                storage.store_episode("owner", &episode).await.unwrap();
            }));
        }
        for task in tasks {
            task.await.unwrap();
        }
        assert_eq!(storage.get_all_episodes("owner").await.unwrap().len(), 1);
        assert_eq!(storage.episode_count("owner").await.unwrap(), 1);
        assert!(
            storage
                .get_episode("owner", episode.id)
                .await
                .unwrap()
                .is_some()
        );
    }

    #[tokio::test]
    async fn regression_invalid_scope_and_unknown_record_version_fail_closed() {
        let (storage, _dir) = create_test_storage().await;
        let episode = Episode::observation("", "No owner");
        assert!(storage.store_episode("", &episode).await.is_err());
        assert!(storage.get_all_episodes(&"x".repeat(65536)).await.is_err());
        assert!(RocksDbMemoryStorage::decode_episode(b"QMEP\0\x02unknown").is_err());
    }

    async fn create_test_storage() -> (RocksDbMemoryStorage, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let config = MemoryStorageConfig::for_testing(temp_dir.path());
        let storage = RocksDbMemoryStorage::open(config).unwrap();
        (storage, temp_dir)
    }

    #[tokio::test]
    async fn test_store_and_get_episode() {
        let (storage, _dir) = create_test_storage().await;

        let episode = Episode::conversation("agent-1", "Hello", "Hi there!");
        storage.store_episode("agent-1", &episode).await.unwrap();

        let retrieved = storage
            .get_episode("agent-1", episode.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(retrieved.id, episode.id);
        assert_eq!(retrieved.content.primary, "Hello");
    }

    #[tokio::test]
    async fn test_get_all_episodes() {
        let (storage, _dir) = create_test_storage().await;

        let ep1 = Episode::conversation("agent-1", "First", "Response 1");
        let ep2 = Episode::conversation("agent-1", "Second", "Response 2");
        let ep3 = Episode::conversation("agent-2", "Other agent", "Response");

        storage.store_episode("agent-1", &ep1).await.unwrap();
        storage.store_episode("agent-1", &ep2).await.unwrap();
        storage.store_episode("agent-2", &ep3).await.unwrap();

        let agent1_episodes = storage.get_all_episodes("agent-1").await.unwrap();
        assert_eq!(agent1_episodes.len(), 2);

        let agent2_episodes = storage.get_all_episodes("agent-2").await.unwrap();
        assert_eq!(agent2_episodes.len(), 1);
    }

    #[tokio::test]
    async fn test_delete_episode() {
        let (storage, _dir) = create_test_storage().await;

        let episode = Episode::observation("agent-1", "Test event");
        storage.store_episode("agent-1", &episode).await.unwrap();

        assert!(storage.delete_episode("agent-1", episode.id).await.unwrap());
        assert!(
            storage
                .get_episode("agent-1", episode.id)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn test_delete_all_episodes() {
        let (storage, _dir) = create_test_storage().await;

        for i in 0..5 {
            let episode = Episode::observation("agent-1", &format!("Event {}", i));
            storage.store_episode("agent-1", &episode).await.unwrap();
        }

        let count = storage.delete_all_episodes("agent-1").await.unwrap();
        assert_eq!(count, 5);
        assert_eq!(storage.episode_count("agent-1").await.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_episode_count() {
        let (storage, _dir) = create_test_storage().await;

        assert_eq!(storage.episode_count("agent-1").await.unwrap(), 0);

        for i in 0..3 {
            let episode = Episode::observation("agent-1", &format!("Event {}", i));
            storage.store_episode("agent-1", &episode).await.unwrap();
        }

        assert_eq!(storage.episode_count("agent-1").await.unwrap(), 3);
    }

    #[tokio::test]
    async fn test_in_memory_storage() {
        let storage = InMemoryStorage::new();

        let episode = Episode::conversation("agent-1", "Hello", "Hi!");
        storage.store_episode("agent-1", &episode).await.unwrap();

        let retrieved = storage
            .get_episode("agent-1", episode.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(retrieved.id, episode.id);
    }
}

pub mod platform;
