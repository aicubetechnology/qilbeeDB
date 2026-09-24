//! Chronological projections over immutable creation keys and current canonical records.
use super::*;
mod query;
pub use query::*;

pub(super) const COMPANY_ROW: u8 = 0xa0;
pub(super) const SCOPE_ROW: u8 = 0xa1;
pub(super) const TIP: u8 = 0xa2;

fn successor(mut prefix: Vec<u8>) -> Vec<u8> {
    while let Some(byte) = prefix.pop() {
        if byte < 255 {
            prefix.push(byte + 1);
            return prefix;
        }
    }
    unreachable!("chronological prefixes always have a successor")
}

// Flipping the sign bit preserves the ordering of signed millisecond timestamps.
fn timestamp_key(timestamp: i64) -> [u8; 8] {
    ((timestamp as u64) ^ (1 << 63)).to_be_bytes()
}

pub(super) fn scope_key(namespace: &str, record: &MemoryRecord) -> Vec<u8> {
    let mut key = record_prefix(SCOPE_ROW, namespace);
    key.extend(timestamp_key(record.created_at_millis));
    key.extend(record.record_id.as_bytes());
    key
}

pub(super) fn company_key(address: &CompanyMemoryAddress, record: &MemoryRecord) -> Result<Vec<u8>> {
    let mut key = record_prefix(COMPANY_ROW, &address.company_id);
    key.extend(timestamp_key(record.created_at_millis));
    key.extend(address.workspace_id()?.as_bytes());
    key.extend(record.record_id.as_bytes());
    Ok(key)
}

impl RocksDbMemoryStorage {
    /// Immutable creation positions survive updates, reviews, expiration and tombstones.
    /// Canonical payloads and authorization are read again for each request.
    pub(super) fn append_chronological_record(
        &self,
        namespace: &str,
        record: &MemoryRecord,
        batch: &mut rocksdb::WriteBatch,
    ) -> Result<()> {
        let cf = self.cf(super::super::cf::EPISODE_INDEX)?;
        batch.put_cf(cf, scope_key(namespace, record), namespace.as_bytes());
        if let Some(address) = CompanyMemoryAddress::from_namespace(namespace)? {
            batch.put_cf(cf, company_key(&address, record)?, namespace.as_bytes());
        }
        Ok(())
    }

    pub(super) fn append_chronological_tip(
        &self,
        namespace: &str,
        journal: &[u8],
        batch: &mut rocksdb::WriteBatch,
    ) -> Result<()> {
        batch.put_cf(
            self.cf(super::super::cf::EPISODE_INDEX)?,
            record_prefix(TIP, namespace),
            digest(journal),
        );
        Ok(())
    }

    fn chronological_batch(&self, batch: rocksdb::WriteBatch) -> Result<()> {
        let mut options = rocksdb::WriteOptions::default();
        options.disable_wal(false);
        options.set_sync(true);
        self.db.write_opt(batch, &options).map_err(storage_error)
    }

    /// Upgrade before readiness. A missing/mismatched journal fingerprint also
    /// catches records written by an older binary. Backfill is idempotent, uses
    /// <=256 records per synced batch and publishes its tip only after completion.
    /// Canonical records are never physically purged and creation keys never move,
    /// so restarting an interrupted backfill safely overwrites the same positions.
    pub(in crate::storage) fn initialize_chronological_records(&self) -> Result<()> {
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
            let prefix = record_prefix(0x10, &namespace);
            let tip = record_prefix(TIP, &namespace);
            let journal = self
                .db
                .get_cf(meta, record_prefix(0x20, &namespace))
                .map_err(storage_error)?;
            let fingerprint = digest(journal.as_deref().unwrap_or(&[]));
            let ready = self.db.get_cf(index, &tip).map_err(storage_error)?;
            if journal.is_none() || ready.as_deref() != Some(fingerprint.as_slice()) {
                self.memory_snapshot().journal(&namespace)?;
                let mut batch = rocksdb::WriteBatch::default();
                batch.delete_cf(index, &tip);
                self.chronological_batch(batch)?;
                let view = self.memory_snapshot();
                let mut batch = rocksdb::WriteBatch::default();
                let mut count = 0;
                for row in view.db.iterator_cf(
                    episodes,
                    rocksdb::IteratorMode::From(&prefix, rocksdb::Direction::Forward),
                ) {
                    let (key, _) = row.map_err(storage_error)?;
                    if !key.starts_with(&prefix) {
                        break;
                    }
                    let id = Uuid::from_slice(&key[prefix.len()..]).map_err(|_| inconsistent())?;
                    let record = view.record(&namespace, id)?.ok_or_else(inconsistent)?;
                    self.append_chronological_record(&namespace, &record, &mut batch)?;
                    count += 1;
                    if count % 256 == 0 {
                        self.chronological_batch(std::mem::take(&mut batch))?;
                    }
                }
                batch.put_cf(index, tip, fingerprint);
                self.chronological_batch(batch)?;
                tracing::info!(
                    records = count,
                    "Backfilled chronological memory projection"
                );
            }
            iterator.seek(successor(prefix));
        }
        iterator.status().map_err(storage_error)
    }
}
