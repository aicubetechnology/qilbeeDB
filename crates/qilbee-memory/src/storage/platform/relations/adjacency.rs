//! Per-endpoint completeness checks, maintained atomically with relation indexes.
use super::*;
use std::collections::BTreeMap;
const HEAD: u8 = 0x56;
const TIP: u8 = 0x57;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AdjacencyHead {
    schema_version: u32,
    prefix_digest: [u8; 32],
    pub count: u64,
    pub entries_digest: [u8; 32],
}
pub(super) fn head_key(prefix: &[u8]) -> Vec<u8> {
    let mut key = prefix.to_vec();
    key[0] = HEAD;
    key.push(prefix[0]);
    key
}
pub(super) fn prefix(kind: u8, namespace: &str, endpoint: &MemorySourceRef) -> Vec<u8> {
    let mut prefix = record_key(kind, namespace, endpoint.record_id);
    prefix.extend(endpoint.revision.to_be_bytes());
    prefix
}
fn successor(mut prefix: Vec<u8>) -> Vec<u8> {
    while let Some(last) = prefix.pop() {
        if last != u8::MAX {
            prefix.push(last + 1);
            return prefix;
        }
    }
    unreachable!("Platform prefixes begin with a non-maximal byte")
}
pub(super) fn accumulate(accumulator: &mut [u8; 32], key: &[u8], value: &[u8]) {
    let leaf = digest(&[key, value].concat());
    for (target, value) in accumulator.iter_mut().zip(leaf) {
        *target ^= value;
    }
}
impl AdjacencyHead {
    fn empty(prefix: &[u8]) -> Self {
        Self {
            schema_version: 1,
            prefix_digest: digest(prefix),
            count: 0,
            entries_digest: [0; 32],
        }
    }
    fn decode(prefix: &[u8], bytes: &[u8]) -> Result<Self> {
        if bytes.len() > 1024 {
            return Err(inconsistent());
        }
        let head: Self = decode(bytes)?;
        if head.schema_version != 1
            || head.prefix_digest != digest(prefix)
            || (head.count == 0 && head.entries_digest != [0; 32])
        {
            return Err(inconsistent());
        }
        Ok(head)
    }
}
impl MemorySnapshot<'_> {
    pub(super) fn adjacency_head(&self, prefix: &[u8]) -> Result<AdjacencyHead> {
        let bytes = self
            .db
            .get_cf(
                self.storage.cf(super::super::super::cf::AGENT_META)?,
                head_key(prefix),
            )
            .map_err(storage_error)?
            .ok_or_else(inconsistent)?;
        AdjacencyHead::decode(prefix, &bytes)
    }
}
impl RocksDbMemoryStorage {
    /// New canonical revisions receive empty outgoing and incoming integrity heads.
    pub(in crate::storage::platform) fn append_empty_relation_heads(
        &self,
        namespace: &str,
        record: &MemoryRecord,
        batch: &mut rocksdb::WriteBatch,
    ) -> Result<()> {
        let cf = self.cf(super::super::super::cf::AGENT_META)?;
        let reference = MemorySourceRef {
            record_id: record.record_id,
            revision: record.revision,
        };
        for kind in [OUTGOING, INCOMING] {
            let prefix = prefix(kind, namespace, &reference);
            let key = head_key(&prefix);
            if self.db.get_cf(cf, &key).map_err(storage_error)?.is_some() {
                return Err(inconsistent());
            }
            batch.put_cf(cf, key, encode(&AdjacencyHead::empty(&prefix))?);
        }
        Ok(())
    }
    pub(in crate::storage::platform) fn append_relation_projection_tip(
        &self,
        namespace: &str,
        journal: &[u8],
        batch: &mut rocksdb::WriteBatch,
    ) -> Result<()> {
        batch.put_cf(
            self.cf(super::super::super::cf::AGENT_META)?,
            record_prefix(TIP, namespace),
            digest(journal),
        );
        Ok(())
    }
    pub(super) fn append_relation_heads(
        &self,
        snapshot: &MemorySnapshot<'_>,
        namespace: &str,
        relation: &MemoryRelation,
        integrity: &[u8],
        batch: &mut rocksdb::WriteBatch,
    ) -> Result<()> {
        self.append_relation_head_batch(snapshot, namespace, &[(relation, integrity)], batch)
    }
    pub(super) fn append_relation_head_batch(
        &self,
        snapshot: &MemorySnapshot<'_>,
        namespace: &str,
        revisions: &[(&MemoryRelation, &[u8])],
        batch: &mut rocksdb::WriteBatch,
    ) -> Result<()> {
        let cf = self.cf(super::super::super::cf::AGENT_META)?;
        let mut heads = BTreeMap::<Vec<u8>, AdjacencyHead>::new();
        for (relation, integrity) in revisions {
            let old = snapshot.relation(namespace, relation.relation_id)?;
            let old_integrity = if old.is_some() {
                Some(
                    snapshot
                        .db
                        .get_cf(cf, record_key(INTEGRITY, namespace, relation.relation_id))
                        .map_err(storage_error)?
                        .ok_or_else(inconsistent)?,
                )
            } else {
                None
            };
            for (kind, endpoint) in [
                (OUTGOING, &relation.input.source),
                (INCOMING, &relation.input.target),
            ] {
                let prefix = prefix(kind, namespace, endpoint);
                if !heads.contains_key(&prefix) {
                    heads.insert(prefix.clone(), snapshot.adjacency_head(&prefix)?);
                }
                let head = heads.get_mut(&prefix).unwrap();
                let key = adjacency_key(kind, namespace, endpoint, relation.relation_id);
                if old.as_ref().is_some_and(MemoryRelation::indexed) {
                    head.count = head.count.checked_sub(1).ok_or_else(inconsistent)?;
                    accumulate(
                        &mut head.entries_digest,
                        &key,
                        old_integrity.as_ref().unwrap(),
                    );
                }
                if relation.indexed() {
                    head.count = head.count.checked_add(1).ok_or_else(inconsistent)?;
                    accumulate(&mut head.entries_digest, &key, integrity);
                }
                if head.count == 0 && head.entries_digest != [0; 32] {
                    return Err(inconsistent());
                }
            }
        }
        for (prefix, head) in heads {
            batch.put_cf(cf, head_key(&prefix), encode(&head)?);
        }
        Ok(())
    }
    fn adjacency_batch(&self, batch: rocksdb::WriteBatch) -> Result<()> {
        let mut options = rocksdb::WriteOptions::default();
        options.disable_wal(false);
        options.set_sync(true);
        self.db.write_opt(batch, &options).map_err(storage_error)
    }
    /// Startup only. The memory journal detects legacy memory writes; matching namespaces are skipped.
    pub(in crate::storage) fn initialize_relation_adjacency(&self) -> Result<()> {
        let episodes = self.cf(super::super::super::cf::EPISODES)?;
        let meta = self.cf(super::super::super::cf::AGENT_META)?;
        let mut namespaces = self.db.raw_iterator_cf(episodes);
        namespaces.seek([0x10]);
        while let Some(key) = namespaces.key() {
            if key.first() != Some(&0x10) {
                break;
            }
            if key.len() < 3 {
                return Err(inconsistent());
            }
            let len = u16::from_be_bytes([key[1], key[2]]) as usize;
            if key.len() != 3 + len + 16 {
                return Err(inconsistent());
            }
            let namespace = std::str::from_utf8(&key[3..3 + len])
                .map_err(|_| inconsistent())?
                .to_owned();
            let records_prefix = record_prefix(0x10, &namespace);
            let tip_key = record_prefix(TIP, &namespace);
            let journal = self
                .db
                .get_cf(meta, record_prefix(0x20, &namespace))
                .map_err(storage_error)?;
            let fingerprint = digest(journal.as_deref().unwrap_or(&[]));
            if self
                .db
                .get_cf(meta, &tip_key)
                .map_err(storage_error)?
                .as_deref()
                != Some(fingerprint.as_slice())
            {
                self.memory_snapshot().journal(&namespace)?;
                self.rebuild_relation_adjacency(&namespace, &tip_key, &fingerprint)?;
            }
            namespaces.seek(successor(records_prefix));
        }
        namespaces.status().map_err(storage_error)
    }
    fn rebuild_relation_adjacency(
        &self,
        namespace: &str,
        tip_key: &[u8],
        fingerprint: &[u8],
    ) -> Result<()> {
        let meta = self.cf(super::super::super::cf::AGENT_META)?;
        let mut batch = rocksdb::WriteBatch::default();
        let heads = record_prefix(HEAD, namespace);
        batch.delete_cf(meta, tip_key);
        batch.delete_range_cf(meta, &heads, &successor(heads.clone()));
        self.adjacency_batch(batch)?;
        // A crash leaves no ready tip; reopening clears the partial build and starts again.
        let view = self.memory_snapshot();
        let mut batch = rocksdb::WriteBatch::default();
        let records = record_prefix(0x10, namespace);
        let mut count = 0usize;
        for row in view.db.iterator_cf(
            self.cf(super::super::super::cf::EPISODES)?,
            rocksdb::IteratorMode::From(&records, rocksdb::Direction::Forward),
        ) {
            let (key, _) = row.map_err(storage_error)?;
            if !key.starts_with(&records) {
                break;
            }
            let id = Uuid::from_slice(&key[records.len()..]).map_err(|_| inconsistent())?;
            let record = view.record(namespace, id)?.ok_or_else(inconsistent)?;
            self.append_empty_relation_heads(namespace, &record, &mut batch)?;
            count += 1;
            if count % 128 == 0 {
                self.adjacency_batch(std::mem::take(&mut batch))?;
            }
        }
        if !batch.is_empty() {
            self.adjacency_batch(batch)?;
        }
        let relations = record_prefix(RELATION, namespace);
        let mut pending = BTreeMap::<Vec<u8>, AdjacencyHead>::new();
        let mut count = 0usize;
        for row in view.db.iterator_cf(
            meta,
            rocksdb::IteratorMode::From(&relations, rocksdb::Direction::Forward),
        ) {
            let (key, _) = row.map_err(storage_error)?;
            if !key.starts_with(&relations) {
                break;
            }
            let id = Uuid::from_slice(&key[relations.len()..]).map_err(|_| inconsistent())?;
            let (relation, _, integrity) = view
                .relation_with_bytes(namespace, id)?
                .ok_or_else(inconsistent)?;
            for (kind, endpoint) in [
                (OUTGOING, &relation.input.source),
                (INCOMING, &relation.input.target),
            ] {
                let prefix = prefix(kind, namespace, endpoint);
                let key = head_key(&prefix);
                let head = match pending.entry(key) {
                    std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        let stored = self.db.get_cf(meta, entry.key()).map_err(storage_error)?;
                        entry.insert(match stored {
                            Some(bytes) => AdjacencyHead::decode(&prefix, &bytes)?,
                            None => AdjacencyHead::empty(&prefix),
                        })
                    }
                };
                if relation.indexed() {
                    head.count = head.count.checked_add(1).ok_or_else(inconsistent)?;
                    accumulate(
                        &mut head.entries_digest,
                        &adjacency_key(kind, namespace, endpoint, id),
                        &integrity,
                    );
                }
            }
            count += 1;
            if count % 128 == 0 {
                self.flush_adjacency_heads(std::mem::take(&mut pending))?;
            }
        }
        self.flush_adjacency_heads(pending)?;
        let mut batch = rocksdb::WriteBatch::default();
        batch.put_cf(meta, tip_key, fingerprint);
        self.adjacency_batch(batch)
    }
    fn flush_adjacency_heads(&self, pending: BTreeMap<Vec<u8>, AdjacencyHead>) -> Result<()> {
        if pending.is_empty() {
            return Ok(());
        }
        let cf = self.cf(super::super::super::cf::AGENT_META)?;
        let mut batch = rocksdb::WriteBatch::default();
        for (key, value) in pending {
            batch.put_cf(cf, key, encode(&value)?);
        }
        self.adjacency_batch(batch)
    }
}
