//! A request-local coherent view across records, integrity indexes and vectors.
use super::*;

pub(super) struct MemorySnapshot<'a> {
    pub(super) storage: &'a RocksDbMemoryStorage,
    pub(super) db: rocksdb::Snapshot<'a>,
    pub(super) now: i64,
    pub(super) dependencies: std::cell::RefCell<super::derivation::DependencyState>,
}
impl RocksDbMemoryStorage {
    pub(super) fn memory_snapshot(&self) -> MemorySnapshot<'_> {
        MemorySnapshot {
            storage: self,
            db: self.db.snapshot(),
            now: chrono::Utc::now().timestamp_millis(),
            dependencies: Default::default(),
        }
    }
}
impl MemorySnapshot<'_> {
    pub(super) fn record(&self, namespace: &str, id: Uuid) -> Result<Option<MemoryRecord>> {
        Ok(self
            .record_with_bytes(namespace, id)?
            .map(|(record, _, _)| record))
    }
    pub(super) fn record_with_bytes(
        &self,
        namespace: &str,
        id: Uuid,
    ) -> Result<Option<(MemoryRecord, usize, [u8; 32])>> {
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
        let bytes = record.as_deref().unwrap_or(&[]);
        let size = bytes.len();
        let record_digest = digest(bytes);
        Ok(decode_record_pair(id, record, index)?.map(|record| (record, size, record_digest)))
    }
}
