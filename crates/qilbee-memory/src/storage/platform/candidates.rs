//! Current-record candidate projection, maintained in the memory mutation batch.
//!
//! The journal fingerprint detects writes made by an older binary. Startup rebuilds
//! stale namespaces before serving requests; each rebuild batch contains <= 256 records.
use super::snapshot::MemorySnapshot;
use super::*;

const ROW: u8 = 0x30;
const TIP: u8 = 0x31;
pub const CANDIDATE_SELECTION_VERSION: &str = "current_records_v1";

pub(super) fn prefix(
    namespace: &str,
    tag: Option<&str>,
    kind: Option<&EpisodeType>,
) -> Result<Vec<u8>> {
    let mut key = record_prefix(ROW, namespace);
    key.extend_from_slice(&digest(&encode(&(tag, kind))?));
    Ok(key)
}
fn successor(mut prefix: Vec<u8>) -> Vec<u8> {
    while let Some(last) = prefix.pop() {
        if last < 255 {
            prefix.push(last + 1);
            return prefix;
        }
    }
    unreachable!("candidate prefixes have a successor")
}
fn keys(namespace: &str, record: &MemoryRecord) -> Result<Vec<Vec<u8>>> {
    let Some(input) = &record.payload else {
        return Ok(vec![]);
    };
    if record
        .review
        .as_ref()
        .is_some_and(|r| r.disposition == MemoryReviewDisposition::Rejected)
    {
        return Ok(vec![]);
    }
    let tags: std::collections::BTreeSet<_> = std::iter::once(None)
        .chain(input.tags.iter().map(|tag| Some(tag.as_str())))
        .collect();
    let mut keys = vec![];
    for tag in tags {
        for kind in [None, Some(&input.episode_type)] {
            let mut key = prefix(namespace, tag, kind)?;
            key.extend_from_slice(record.record_id.as_bytes());
            keys.push(key);
        }
    }
    Ok(keys)
}
fn value(record: &MemoryRecord) -> Result<Vec<u8>> {
    let mut value = record.revision.to_be_bytes().to_vec();
    value.extend_from_slice(&digest(&encode(record)?));
    Ok(value)
}
impl RocksDbMemoryStorage {
    /// Caller holds the mutation lock. Canonical state, projection and journal share one WAL batch.
    pub(super) fn update_candidates(
        &self,
        namespace: &str,
        record: &MemoryRecord,
        batch: &mut rocksdb::WriteBatch,
    ) -> Result<()> {
        let cf = self.cf(super::super::cf::EPISODE_INDEX)?;
        if let Some(old) = self.memory_snapshot().record(namespace, record.record_id)? {
            for key in keys(namespace, &old)? {
                batch.delete_cf(cf, key);
            }
        }
        let value = value(record)?;
        for key in keys(namespace, record)? {
            batch.put_cf(cf, key, &value);
        }
        Ok(())
    }
    fn candidate_batch(&self, batch: rocksdb::WriteBatch) -> Result<()> {
        let mut options = rocksdb::WriteOptions::default();
        options.disable_wal(false);
        options.set_sync(true);
        self.db.write_opt(batch, &options).map_err(storage_error)
    }
    /// No concurrent writers exist until open returns. Seek over each namespace,
    /// avoiding a full history scan when its journal fingerprint is unchanged.
    pub(in crate::storage) fn initialize_candidates(&self) -> Result<()> {
        let episodes = self.cf(super::super::cf::EPISODES)?;
        let index = self.cf(super::super::cf::EPISODE_INDEX)?;
        let meta = self.cf(super::super::cf::AGENT_META)?;
        let mut iterator = self.db.raw_iterator_cf(episodes);
        iterator.seek([0x10]);
        while iterator.valid() {
            let key = iterator.key().ok_or_else(inconsistent)?;
            if key.first() != Some(&0x10) {
                break;
            }
            if key.len() < 3 {
                return Err(inconsistent());
            }
            let length = u16::from_be_bytes([key[1], key[2]]) as usize;
            if key.len() != 3 + length + 16 {
                return Err(inconsistent());
            }
            let namespace = std::str::from_utf8(&key[3..3 + length])
                .map_err(|_| inconsistent())?
                .to_owned();
            let record_prefix = record_prefix(0x10, &namespace);
            let tip_key = super::record_prefix(TIP, &namespace);
            let journal = self
                .db
                .get_cf(meta, super::record_prefix(0x20, &namespace))
                .map_err(storage_error)?;
            let fingerprint = digest(journal.as_deref().unwrap_or(&[]));
            let ready = self.db.get_cf(index, &tip_key).map_err(storage_error)?;
            if journal.is_none() || ready.as_deref() != Some(fingerprint.as_slice()) {
                // Validate the source journal before rebuilding its projection.
                self.memory_snapshot().journal(&namespace)?;
                let projection = super::record_prefix(ROW, &namespace);
                let mut batch = rocksdb::WriteBatch::default();
                batch.delete_cf(index, &tip_key);
                batch.delete_range_cf(index, &projection, &successor(projection.clone()));
                self.candidate_batch(batch)?;
                let view = self.memory_snapshot();
                let mut batch = rocksdb::WriteBatch::default();
                let mut count = 0;
                for item in view.db.iterator_cf(
                    episodes,
                    rocksdb::IteratorMode::From(&record_prefix, rocksdb::Direction::Forward),
                ) {
                    let (key, _) = item.map_err(storage_error)?;
                    if !key.starts_with(&record_prefix) {
                        break;
                    }
                    let id = Uuid::from_slice(&key[record_prefix.len()..])
                        .map_err(|_| inconsistent())?;
                    let record = view.record(&namespace, id)?.ok_or_else(inconsistent)?;
                    let value = value(&record)?;
                    for key in keys(&namespace, &record)? {
                        batch.put_cf(index, key, &value);
                    }
                    count += 1;
                    if count % 256 == 0 {
                        self.candidate_batch(std::mem::take(&mut batch))?;
                    }
                }
                batch.put_cf(index, tip_key, fingerprint);
                self.candidate_batch(batch)?;
                tracing::info!(
                    records = count,
                    "Rebuilt current memory candidate projection"
                );
            }
            iterator.seek(successor(record_prefix));
        }
        iterator.status().map_err(storage_error)
    }
}
impl MemorySnapshot<'_> {
    pub(super) fn candidate_record(
        &self,
        namespace: &str,
        id: Uuid,
        bytes: &[u8],
    ) -> Result<(MemoryRecord, usize)> {
        let (record, size, record_digest) = self
            .record_with_bytes(namespace, id)?
            .ok_or_else(inconsistent)?;
        if bytes.len() != 40
            || bytes[..8] != record.revision.to_be_bytes()
            || bytes[8..] != record_digest
        {
            return Err(inconsistent());
        }
        Ok((record, size))
    }
}
