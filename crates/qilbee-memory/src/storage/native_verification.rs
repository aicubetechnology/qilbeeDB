//! Read-only consistency audit of native episode identities and UUID lookups.
use super::{EpisodeId, Error, Result, RocksDbMemoryStorage, cf, prefix};

fn inconsistent() -> Error {
    Error::DataCorruption("Native episode or UUID index is inconsistent".into())
}

fn storage_error(error: rocksdb::Error) -> Error {
    Error::Storage(format!("Native episode verification failed: {error}"))
}

/// Decode the complete key; a valid prefix alone cannot establish an identity.
fn identity(key: &[u8]) -> Result<(&str, i64, EpisodeId)> {
    if key.len() < 3 || key[0] != prefix::EPISODE {
        return Err(inconsistent());
    }
    let length = u16::from_be_bytes([key[1], key[2]]) as usize;
    if length == 0 || key.len() != 3 + length + 8 + 16 {
        return Err(inconsistent());
    }
    let end = 3 + length;
    let owner = std::str::from_utf8(&key[3..end]).map_err(|_| inconsistent())?;
    let time = i64::from_be_bytes(key[end..end + 8].try_into().map_err(|_| inconsistent())?);
    let id = uuid::Uuid::from_slice(&key[end + 8..]).map_err(|_| inconsistent())?;
    Ok((owner, time, EpisodeId::from_uuid(id)))
}

impl RocksDbMemoryStorage {
    /// Require a bijection between native records and their UUID lookup rows.
    /// The snapshot also makes both directions coherent for an in-process caller.
    pub(super) fn verify_native_episodes(&self) -> Result<()> {
        let _guard = self
            .mutation_lock
            .lock()
            .map_err(|_| Error::Storage("Native episode mutation lock is poisoned".into()))?;
        let episodes = self.cf(cf::EPISODES)?;
        let index = self.cf(cf::EPISODE_INDEX)?;
        let snapshot = self.db.snapshot();
        for item in snapshot.iterator_cf(
            episodes,
            rocksdb::IteratorMode::From(&[prefix::EPISODE], rocksdb::Direction::Forward),
        ) {
            let (key, bytes) = item.map_err(storage_error)?;
            if key.first() != Some(&prefix::EPISODE) {
                break;
            }
            let (owner, time, id) = identity(&key)?;
            let episode = Self::decode_episode(&bytes).map_err(|_| inconsistent())?;
            if episode.agent_id != owner
                || episode.id != id
                || episode.event_time.as_millis() != time
            {
                return Err(inconsistent());
            }
            // The primary key's location bytes have exactly the index value layout.
            let expected = &key[1..key.len() - 16];
            match snapshot
                .get_cf(index, Self::episode_index_key(id))
                .map_err(storage_error)?
            {
                Some(value) if value == expected => {}
                _ => return Err(inconsistent()),
            }
        }
        // A record-to-index walk alone would accept an orphaned or malformed index.
        for item in snapshot.iterator_cf(
            index,
            rocksdb::IteratorMode::From(&[prefix::EPISODE_INDEX], rocksdb::Direction::Forward),
        ) {
            let (key, value) = item.map_err(storage_error)?;
            if key.first() != Some(&prefix::EPISODE_INDEX) {
                break;
            }
            if key.len() != 17 {
                return Err(inconsistent());
            }
            let id = EpisodeId::from_uuid(
                uuid::Uuid::from_slice(&key[1..]).map_err(|_| inconsistent())?,
            );
            let (owner, time) = Self::index_location(&value).map_err(|_| inconsistent())?;
            Self::validate_agent(owner).map_err(|_| inconsistent())?;
            if snapshot
                .get_cf(episodes, Self::episode_key(owner, time, id))
                .map_err(storage_error)?
                .is_none()
            {
                return Err(inconsistent());
            }
        }
        Ok(())
    }
}
