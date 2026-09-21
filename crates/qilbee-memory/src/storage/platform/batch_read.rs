//! Bounded current-record reads with one snapshot and one eligibility clock.
use super::snapshot::MemorySnapshot;
use super::*;
use std::collections::BTreeSet;

pub const MAX_MEMORY_READ_RECORDS: usize = 100;
pub const MAX_MEMORY_READ_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryReadEntry {
    pub record_id: Uuid,
    /// Missing and ineligible records share the same non-disclosing result.
    pub record: Option<MemoryRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryReadBatch {
    /// The single server clock used for every root and dependency in this batch.
    /// This is an observation, not a lease or a future validity guarantee.
    pub evaluated_at_millis: i64,
    /// Exactly one entry per requested UUID, in request order.
    pub entries: Vec<MemoryReadEntry>,
    /// Serialized root-record bytes, including unavailable roots; excludes indexes.
    pub record_bytes: usize,
    pub dependency_work: DependencyWork,
}

impl RocksDbMemoryStorage {
    /// The caller must authorize the exact namespace. Reads do not advance a
    /// checkpoint, activate a journal, or acquire the memory mutation lock.
    /// Resource exhaustion or encountered corruption fails the complete batch.
    pub fn read_memory_records(&self, namespace: &str, ids: &[Uuid]) -> Result<MemoryReadBatch> {
        Self::validate_agent(namespace)?;
        if !(1..=MAX_MEMORY_READ_RECORDS).contains(&ids.len())
            || ids.iter().collect::<BTreeSet<_>>().len() != ids.len()
        {
            return Err(Error::ValidationError(
                "A memory read batch requires 1-100 distinct record UUIDs".into(),
            ));
        }
        self.memory_snapshot().read_records(namespace, ids)
    }
}

impl MemorySnapshot<'_> {
    pub(super) fn read_records(&self, namespace: &str, ids: &[Uuid]) -> Result<MemoryReadBatch> {
        let mut entries = Vec::with_capacity(ids.len());
        let mut record_bytes = 0usize;
        for &id in ids {
            let bytes = self
                .db
                .get_cf(
                    self.storage.cf(super::super::cf::EPISODES)?,
                    record_key(0x10, namespace, id),
                )
                .map_err(storage_error)?;
            record_bytes = record_bytes.saturating_add(bytes.as_ref().map_or(0, Vec::len));
            // Bound retained/decoded source bytes before deserializing this root.
            // RocksDB has already fetched the crossing value; this is not an RSS cap.
            if record_bytes > MAX_MEMORY_READ_BYTES {
                return Err(Error::ValidationError(
                    "Memory read byte budget exhausted; request fewer records".into(),
                ));
            }
            let index = self
                .db
                .get_cf(
                    self.storage.cf(super::super::cf::EPISODE_INDEX)?,
                    record_key(0x11, namespace, id),
                )
                .map_err(storage_error)?;
            let record = match decode_record_pair(id, bytes, index)? {
                Some(record) if self.eligible(namespace, &record)? => Some(record),
                _ => None,
            };
            entries.push(MemoryReadEntry {
                record_id: id,
                record,
            });
        }
        Ok(MemoryReadBatch {
            evaluated_at_millis: self.now,
            entries,
            record_bytes,
            dependency_work: self.dependency_work(),
        })
    }
}

#[cfg(test)]
mod tests;
