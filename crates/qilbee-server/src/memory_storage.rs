//! Durable HTTP memory storage with blocking RocksDB work kept off Tokio workers.

use async_trait::async_trait;
use qilbee_core::{Error, Result};
use qilbee_memory::{
    Episode, MemoryStorage, MemoryStorageConfig, RocksDbMemoryStorage, episode::EpisodeId,
};
use std::{future::Future, path::Path, sync::Arc};

pub(crate) struct HttpMemoryStorage(Arc<RocksDbMemoryStorage>);

impl HttpMemoryStorage {
    pub(crate) fn open(path: &Path) -> Result<Self> {
        let path = path
            .to_str()
            .ok_or_else(|| Error::Configuration("Memory path must be valid UTF-8".into()))?;
        let config = MemoryStorageConfig {
            path: path.into(),
            enable_wal: true,
            sync_writes: true,
            ..Default::default()
        };
        Ok(Self(Arc::new(RocksDbMemoryStorage::open(config)?)))
    }

    async fn run<T, F, Fut>(&self, operation: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(Arc<RocksDbMemoryStorage>) -> Fut + Send + 'static,
        Fut: Future<Output = Result<T>> + Send + 'static,
    {
        let storage = self.0.clone();
        let runtime = tokio::runtime::Handle::current();
        crate::http_work::spawn_blocking(move || runtime.block_on(operation(storage)))
            .await
            .map_err(|error| Error::Storage(format!("Memory worker failed: {error}")))?
    }
}

#[async_trait]
impl MemoryStorage for HttpMemoryStorage {
    async fn store_episode(&self, agent_id: &str, episode: &Episode) -> Result<()> {
        let (agent, episode) = (agent_id.to_owned(), episode.clone());
        self.run(move |storage| async move { storage.store_episode(&agent, &episode).await })
            .await
    }

    async fn get_episode(&self, agent_id: &str, id: EpisodeId) -> Result<Option<Episode>> {
        let agent = agent_id.to_owned();
        self.run(move |storage| async move { storage.get_episode(&agent, id).await })
            .await
    }

    async fn get_all_episodes(&self, agent_id: &str) -> Result<Vec<Episode>> {
        let agent = agent_id.to_owned();
        self.run(move |storage| async move { storage.get_all_episodes(&agent).await })
            .await
    }

    async fn get_episodes_in_range(
        &self,
        agent_id: &str,
        start: i64,
        end: i64,
    ) -> Result<Vec<Episode>> {
        let agent = agent_id.to_owned();
        self.run(
            move |storage| async move { storage.get_episodes_in_range(&agent, start, end).await },
        )
        .await
    }

    async fn delete_episode(&self, agent_id: &str, id: EpisodeId) -> Result<bool> {
        let agent = agent_id.to_owned();
        self.run(move |storage| async move { storage.delete_episode(&agent, id).await })
            .await
    }

    async fn delete_all_episodes(&self, agent_id: &str) -> Result<usize> {
        let agent = agent_id.to_owned();
        self.run(move |storage| async move { storage.delete_all_episodes(&agent).await })
            .await
    }

    async fn episode_count(&self, agent_id: &str) -> Result<usize> {
        let agent = agent_id.to_owned();
        self.run(move |storage| async move { storage.episode_count(&agent).await })
            .await
    }

    async fn update_episode(&self, agent_id: &str, episode: &Episode) -> Result<()> {
        self.store_episode(agent_id, episode).await
    }

    async fn flush(&self) -> Result<()> {
        self.run(|storage| async move { storage.flush().await })
            .await
    }

    async fn close(&self) -> Result<()> {
        self.flush().await
    }
}
