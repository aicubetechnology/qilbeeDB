//! A request-local coherent view across records, integrity indexes and vectors.
use super::*;

pub(super) struct MemorySnapshot<'a> {
    pub(super) storage: &'a RocksDbMemoryStorage,
    pub(super) db: rocksdb::Snapshot<'a>,
    pub(super) now: i64,
}
impl RocksDbMemoryStorage {
    pub(super) fn memory_snapshot(&self) -> MemorySnapshot<'_> {
        MemorySnapshot {
            storage: self,
            db: self.db.snapshot(),
            now: chrono::Utc::now().timestamp_millis(),
        }
    }
}
impl MemorySnapshot<'_> {
    pub(super) fn record(&self, namespace: &str, id: Uuid) -> Result<Option<MemoryRecord>> {
        let record = self
            .db
            .get_cf(
                self.storage.cf(super::super::cf::EPISODES)?,
                record_key(0x10, namespace, id),
            )
            .map_err(storage_error)?;
        let index = self
            .db
            .get_cf(
                self.storage.cf(super::super::cf::EPISODE_INDEX)?,
                record_key(0x11, namespace, id),
            )
            .map_err(storage_error)?;
        decode_record_pair(id, record, index)
    }
}
