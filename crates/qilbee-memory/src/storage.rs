//! Memory storage abstraction and implementations
//!
//! Provides persistent storage for agent memories using RocksDB.

use crate::episode::{Episode, EpisodeId};
use async_trait::async_trait;
use qilbee_core::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tracing::{debug, info, warn};

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

        Ok(Self {
            db: Arc::new(db),
            config,
        })
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
        let episodes_cf = self.cf(cf::EPISODES)?;
        let index_cf = self.cf(cf::EPISODE_INDEX)?;

        // Serialize episode
        let value = bincode::serialize(episode)
            .map_err(|e| Error::Serialization(format!("Failed to serialize episode: {}", e)))?;

        // Build keys
        let episode_key =
            Self::episode_key(agent_id, episode.event_time.as_millis(), episode.id);
        let index_key = Self::episode_index_key(episode.id);

        // Index value: agent_id + timestamp for lookups
        let mut index_value = Vec::new();
        let agent_bytes = agent_id.as_bytes();
        index_value.extend_from_slice(&(agent_bytes.len() as u16).to_be_bytes());
        index_value.extend_from_slice(agent_bytes);
        index_value.extend_from_slice(&episode.event_time.as_millis().to_be_bytes());

        // Write batch for atomicity
        let mut batch = rocksdb::WriteBatch::default();
        batch.put_cf(episodes_cf, &episode_key, &value);
        batch.put_cf(index_cf, &index_key, &index_value);

        let mut write_opts = rocksdb::WriteOptions::default();
        write_opts.set_sync(self.config.sync_writes);

        self.db
            .write_opt(batch, &write_opts)
            .map_err(|e| Error::Storage(format!("Failed to store episode: {}", e)))?;

        debug!(
            "Stored episode {} for agent {}",
            episode.id, agent_id
        );

        Ok(())
    }

    async fn get_episode(&self, agent_id: &str, episode_id: EpisodeId) -> Result<Option<Episode>> {
        let index_cf = self.cf(cf::EPISODE_INDEX)?;
        let episodes_cf = self.cf(cf::EPISODES)?;

        // First lookup the index to get agent_id and timestamp
        let index_key = Self::episode_index_key(episode_id);
        let index_value = match self.db.get_cf(index_cf, &index_key) {
            Ok(Some(v)) => v,
            Ok(None) => return Ok(None),
            Err(e) => return Err(Error::Storage(format!("Failed to read index: {}", e))),
        };

        // Parse index value to get timestamp
        if index_value.len() < 10 {
            return Err(Error::Internal("Invalid index value".to_string()));
        }
        let agent_len = u16::from_be_bytes([index_value[0], index_value[1]]) as usize;
        let timestamp_start = 2 + agent_len;
        if index_value.len() < timestamp_start + 8 {
            return Err(Error::Internal("Invalid index value".to_string()));
        }
        let timestamp_bytes: [u8; 8] = index_value[timestamp_start..timestamp_start + 8]
            .try_into()
            .map_err(|_| Error::Internal("Invalid timestamp bytes".to_string()))?;
        let timestamp = i64::from_be_bytes(timestamp_bytes);

        // Now read the actual episode
        let episode_key = Self::episode_key(agent_id, timestamp, episode_id);
        match self.db.get_cf(episodes_cf, &episode_key) {
            Ok(Some(value)) => {
                let episode: Episode = bincode::deserialize(&value).map_err(|e| {
                    Error::Deserialization(format!("Failed to deserialize episode: {}", e))
                })?;
                Ok(Some(episode))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(Error::Storage(format!("Failed to read episode: {}", e))),
        }
    }

    async fn get_all_episodes(&self, agent_id: &str) -> Result<Vec<Episode>> {
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

            let episode: Episode = bincode::deserialize(&value).map_err(|e| {
                Error::Deserialization(format!("Failed to deserialize episode: {}", e))
            })?;

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

            let episode: Episode = bincode::deserialize(&value).map_err(|e| {
                Error::Deserialization(format!("Failed to deserialize episode: {}", e))
            })?;

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
        let index_cf = self.cf(cf::EPISODE_INDEX)?;
        let episodes_cf = self.cf(cf::EPISODES)?;

        // First lookup the index to get timestamp
        let index_key = Self::episode_index_key(episode_id);
        let index_value = match self.db.get_cf(index_cf, &index_key) {
            Ok(Some(v)) => v,
            Ok(None) => return Ok(false),
            Err(e) => return Err(Error::Storage(format!("Failed to read index: {}", e))),
        };

        // Parse timestamp from index
        if index_value.len() < 10 {
            return Err(Error::Internal("Invalid index value".to_string()));
        }
        let agent_len = u16::from_be_bytes([index_value[0], index_value[1]]) as usize;
        let timestamp_start = 2 + agent_len;
        let timestamp_bytes: [u8; 8] = index_value[timestamp_start..timestamp_start + 8]
            .try_into()
            .map_err(|_| Error::Internal("Invalid timestamp bytes".to_string()))?;
        let timestamp = i64::from_be_bytes(timestamp_bytes);

        // Delete both episode and index
        let episode_key = Self::episode_key(agent_id, timestamp, episode_id);

        let mut batch = rocksdb::WriteBatch::default();
        batch.delete_cf(episodes_cf, &episode_key);
        batch.delete_cf(index_cf, &index_key);

        self.db
            .write(batch)
            .map_err(|e| Error::Storage(format!("Failed to delete episode: {}", e)))?;

        debug!("Deleted episode {} for agent {}", episode_id, agent_id);

        Ok(true)
    }

    async fn delete_all_episodes(&self, agent_id: &str) -> Result<usize> {
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
            let episode: Episode = bincode::deserialize(&value).map_err(|e| {
                Error::Deserialization(format!("Failed to deserialize episode: {}", e))
            })?;

            let index_key = Self::episode_index_key(episode.id);

            batch.delete_cf(episodes_cf, &key);
            batch.delete_cf(index_cf, &index_key);
            count += 1;
        }

        if count > 0 {
            self.db
                .write(batch)
                .map_err(|e| Error::Storage(format!("Failed to delete episodes: {}", e)))?;

            info!("Deleted {} episodes for agent {}", count, agent_id);
        }

        Ok(count)
    }

    async fn episode_count(&self, agent_id: &str) -> Result<usize> {
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
            let episode: Episode = bincode::deserialize(&value).map_err(|e| {
                Error::Deserialization(format!("Failed to deserialize episode: {}", e))
            })?;

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

    async fn flush(&self) -> Result<()> {
        self.db
            .flush()
            .map_err(|e| Error::Storage(format!("Failed to flush: {}", e)))?;
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
        Ok(episodes
            .remove(agent_id)
            .map(|m| m.len())
            .unwrap_or(0))
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

        assert!(storage
            .delete_episode("agent-1", episode.id)
            .await
            .unwrap());
        assert!(storage
            .get_episode("agent-1", episode.id)
            .await
            .unwrap()
            .is_none());
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
