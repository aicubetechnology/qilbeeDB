//! Read-only audit of current relation integrity, adjacency rows, heads and tips.
use super::*;

fn namespace(key: &[u8], kind: u8, suffix: usize) -> Result<(&str, &[u8])> {
    if key.len() < 3 || key[0] != kind {
        return Err(inconsistent());
    }
    let length = u16::from_be_bytes([key[1], key[2]]) as usize;
    if length == 0 || key.len() != 3 + length + suffix {
        return Err(inconsistent());
    }
    let end = 3 + length;
    let namespace = std::str::from_utf8(&key[3..end]).map_err(|_| inconsistent())?;
    Ok((namespace, &key[end..]))
}

fn reference(bytes: &[u8]) -> Result<MemorySourceRef> {
    if bytes.len() != 24 {
        return Err(inconsistent());
    }
    let revision = u64::from_be_bytes(bytes[16..].try_into().map_err(|_| inconsistent())?);
    if revision == 0 {
        return Err(inconsistent());
    }
    Ok(MemorySourceRef {
        record_id: Uuid::from_slice(&bytes[..16]).map_err(|_| inconsistent())?,
        revision,
    })
}

impl MemorySnapshot<'_> {
    fn retained_endpoint(&self, namespace: &str, endpoint: &MemorySourceRef) -> Result<()> {
        // Updates leave empty historical heads, while startup can remove those
        // not referenced by a relation. Require a real identity and a nonfuture
        // revision, without requiring every optional historical empty head.
        let record = self
            .record(namespace, endpoint.record_id)?
            .ok_or_else(inconsistent)?;
        if endpoint.revision == 0 || endpoint.revision > record.revision {
            return Err(inconsistent());
        }
        Ok(())
    }

    fn relation_row_count(&self, kind: u8, suffix: usize) -> Result<u64> {
        let meta = self.storage.cf(super::super::super::cf::AGENT_META)?;
        let mut count = 0u64;
        for row in self.db.iterator_cf(
            meta,
            rocksdb::IteratorMode::From(&[kind], rocksdb::Direction::Forward),
        ) {
            let (key, _) = row.map_err(storage_error)?;
            if key.first() != Some(&kind) {
                break;
            }
            namespace(&key, kind, suffix)?;
            count = count.checked_add(1).ok_or_else(inconsistent)?;
        }
        Ok(count)
    }

    fn verify_relation_heads(&self) -> Result<u64> {
        let meta = self.storage.cf(super::super::super::cf::AGENT_META)?;
        let mut heads = 0u64;
        for row in self.db.iterator_cf(
            meta,
            rocksdb::IteratorMode::From(&[adjacency::HEAD], rocksdb::Direction::Forward),
        ) {
            let (key, _) = row.map_err(storage_error)?;
            if key.first() != Some(&adjacency::HEAD) {
                break;
            }
            let (namespace, tail) = namespace(&key, adjacency::HEAD, 25)?;
            let endpoint = reference(&tail[..24])?;
            let kind = tail[24];
            if ![OUTGOING, INCOMING].contains(&kind) {
                return Err(inconsistent());
            }
            self.retained_endpoint(namespace, &endpoint)?;
            let prefix = adjacency::prefix(kind, namespace, &endpoint);
            let head = self.adjacency_head(&prefix)?;
            let mut count = 0u64;
            let mut entries_digest = [0; 32];
            for entry in self.db.iterator_cf(
                meta,
                rocksdb::IteratorMode::From(&prefix, rocksdb::Direction::Forward),
            ) {
                let (key, value) = entry.map_err(storage_error)?;
                if !key.starts_with(&prefix) {
                    break;
                }
                if key.len() != prefix.len() + 16 {
                    return Err(inconsistent());
                }
                adjacency::accumulate(&mut entries_digest, &key, &value);
                count = count.checked_add(1).ok_or_else(inconsistent)?;
            }
            if count != head.count || entries_digest != head.entries_digest {
                return Err(inconsistent());
            }
            heads = heads.checked_add(1).ok_or_else(inconsistent)?;
        }
        Ok(heads)
    }
}

impl RocksDbMemoryStorage {
    /// Audit relation projections in one coherent snapshot without rebuilding.
    pub(in crate::storage::platform) fn verify_relation_projections(&self) -> Result<u64> {
        let _guard = self
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Memory mutation lock poisoned".into()))?;
        let view = self.memory_snapshot();
        let episodes = self.cf(super::super::super::cf::EPISODES)?;
        let meta = self.cf(super::super::super::cf::AGENT_META)?;
        let mut namespaces = 0u64;
        let mut previous_namespace = None;
        for row in view.db.iterator_cf(
            episodes,
            rocksdb::IteratorMode::From(&[0x10], rocksdb::Direction::Forward),
        ) {
            let (key, _) = row.map_err(storage_error)?;
            if key.first() != Some(&0x10) {
                break;
            }
            let (namespace, id) = namespace(&key, 0x10, 16)?;
            if previous_namespace.as_deref() != Some(namespace) {
                let journal = view
                    .db
                    .get_cf(meta, record_prefix(0x20, namespace))
                    .map_err(storage_error)?;
                let expected = digest(journal.as_deref().unwrap_or(&[]));
                if view
                    .db
                    .get_cf(meta, record_prefix(adjacency::TIP, namespace))
                    .map_err(storage_error)?
                    .as_deref()
                    != Some(expected.as_slice())
                {
                    return Err(inconsistent());
                }
                previous_namespace = Some(namespace.to_owned());
                namespaces = namespaces.checked_add(1).ok_or_else(inconsistent)?;
            }
            let record = view
                .record(namespace, Uuid::from_slice(id).map_err(|_| inconsistent())?)?
                .ok_or_else(inconsistent)?;
            let endpoint = MemorySourceRef {
                record_id: record.record_id,
                revision: record.revision,
            };
            for kind in [OUTGOING, INCOMING] {
                view.adjacency_head(&adjacency::prefix(kind, namespace, &endpoint))?;
            }
        }
        let mut relations = 0u64;
        let mut indexed = 0u64;
        for row in view.db.iterator_cf(
            meta,
            rocksdb::IteratorMode::From(&[RELATION], rocksdb::Direction::Forward),
        ) {
            let (key, _) = row.map_err(storage_error)?;
            if key.first() != Some(&RELATION) {
                break;
            }
            let (namespace, id) = namespace(&key, RELATION, 16)?;
            let (relation, _, _) = view
                .relation_with_bytes(namespace, Uuid::from_slice(id).map_err(|_| inconsistent())?)?
                .ok_or_else(inconsistent)?;
            for (kind, endpoint) in [
                (OUTGOING, &relation.input.source),
                (INCOMING, &relation.input.target),
            ] {
                view.retained_endpoint(namespace, endpoint)?;
                view.adjacency_head(&adjacency::prefix(kind, namespace, endpoint))?;
            }
            relations = relations.checked_add(1).ok_or_else(inconsistent)?;
            if relation.indexed() {
                indexed = indexed.checked_add(1).ok_or_else(inconsistent)?;
            }
        }
        // Every implied pair was already checked against its relation integrity
        // above. Equal global counts reject any additional/orphaned row, even
        // in a namespace with no canonical memories or adjacency head.
        if view.relation_row_count(INTEGRITY, 16)? != relations
            || view.relation_row_count(OUTGOING, 40)? != indexed
            || view.relation_row_count(INCOMING, 40)? != indexed
            || view.relation_row_count(adjacency::TIP, 0)? != namespaces
        {
            return Err(inconsistent());
        }
        view.verify_relation_heads()
    }
}
