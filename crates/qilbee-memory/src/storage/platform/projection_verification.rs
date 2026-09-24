//! Offline verification of the memory store's rebuildable projections.
//!
//! A writable open rebuilds the candidate and chronological projections of any
//! namespace whose journal fingerprint changed. Offline verification re-derives
//! both projections from the canonical records with the same key and value
//! builders, requires every implied entry to exist with identical bytes, every
//! tip to match its journal state, and the persisted counts to equal the
//! implied counts. Nothing is written or repaired.
use super::*;

/// Counts observed while re-deriving the projections of a stopped store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryProjectionVerification {
    /// Namespaces holding canonical records.
    pub namespaces: u64,
    /// Candidate rows implied by current records and found identical.
    pub candidate_entries: u64,
    /// Scope-ordered chronological rows implied and found identical.
    pub chronological_scope_entries: u64,
    /// Company-ordered chronological rows implied and found identical.
    pub chronological_company_entries: u64,
    /// Relation-head entries present; counted, not re-derived.
    pub relation_head_entries: u64,
}

/// Entries that a writable open rebuilds from canonical records. They are
/// excluded from the authoritative digest and counted separately.
pub(in crate::storage) fn derived_entry(family: &str, key: &[u8]) -> bool {
    let Some(kind) = key.first() else {
        return false;
    };
    match family {
        f if f == super::super::cf::EPISODE_INDEX => matches!(
            *kind,
            candidates::ROW | candidates::TIP | ordered::SCOPE_ROW | ordered::COMPANY_ROW | ordered::TIP
        ),
        // Workspace and relation projections remain in the byte inventory until
        // their contents are independently re-derived, not merely counted.
        _ => false,
    }
}

fn namespace_of(key: &[u8]) -> Result<String> {
    if key.len() < 3 {
        return Err(inconsistent());
    }
    let length = u16::from_be_bytes([key[1], key[2]]) as usize;
    if key.len() != 3 + length + 16 {
        return Err(inconsistent());
    }
    std::str::from_utf8(&key[3..3 + length])
        .map(str::to_owned)
        .map_err(|_| inconsistent())
}

fn successor(mut prefix: Vec<u8>) -> Vec<u8> {
    while let Some(byte) = prefix.pop() {
        if byte < 255 {
            prefix.push(byte + 1);
            return prefix;
        }
    }
    unreachable!("record prefixes always have a successor")
}

impl RocksDbMemoryStorage {
    fn count_prefix(&self, family: &rocksdb::ColumnFamily, prefix: &[u8]) -> Result<u64> {
        let mut count = 0u64;
        for item in self.db.iterator_cf(
            family,
            rocksdb::IteratorMode::From(prefix, rocksdb::Direction::Forward),
        ) {
            let (key, _) = item.map_err(storage_error)?;
            if !key.starts_with(prefix) {
                break;
            }
            count += 1;
        }
        Ok(count)
    }

    fn expect_entry(
        &self,
        family: &rocksdb::ColumnFamily,
        key: &[u8],
        value: &[u8],
    ) -> Result<()> {
        match self.db.get_cf(family, key).map_err(storage_error)? {
            Some(persisted) if persisted == value => Ok(()),
            _ => Err(inconsistent()),
        }
    }

    /// Re-derive the candidate and chronological projections of every namespace.
    pub fn verify_memory_projections(&self) -> Result<MemoryProjectionVerification> {
        let episodes = self.cf(super::super::cf::EPISODES)?;
        let index = self.cf(super::super::cf::EPISODE_INDEX)?;
        let meta = self.cf(super::super::cf::AGENT_META)?;
        let mut report = MemoryProjectionVerification {
            namespaces: 0,
            candidate_entries: 0,
            chronological_scope_entries: 0,
            chronological_company_entries: 0,
            relation_head_entries: 0,
        };
        let mut iterator = self.db.raw_iterator_cf(episodes);
        iterator.seek([0x10]);
        while let Some(key) = iterator.key() {
            if key.first() != Some(&0x10) {
                break;
            }
            let namespace = namespace_of(key)?;
            let records = record_prefix(0x10, &namespace);
            report.namespaces += 1;
            // Both tips must carry the fingerprint of the current journal state;
            // anything else means a rebuild was interrupted or never ran.
            let journal = self
                .db
                .get_cf(meta, record_prefix(0x20, &namespace))
                .map_err(storage_error)?;
            let fingerprint = digest(journal.as_deref().unwrap_or(&[]));
            for tip in [candidates::TIP, ordered::TIP] {
                self.expect_entry(index, &record_prefix(tip, &namespace), &fingerprint)?;
            }
            let address = CompanyMemoryAddress::from_namespace(&namespace)?;
            let view = self.memory_snapshot();
            let mut candidates_implied = 0u64;
            let mut scope_implied = 0u64;
            for row in view.db.iterator_cf(
                episodes,
                rocksdb::IteratorMode::From(&records, rocksdb::Direction::Forward),
            ) {
                let (key, _) = row.map_err(storage_error)?;
                if !key.starts_with(&records) {
                    break;
                }
                let id = Uuid::from_slice(&key[records.len()..]).map_err(|_| inconsistent())?;
                let record = view.record(&namespace, id)?.ok_or_else(inconsistent)?;
                let value = candidates::value(&record)?;
                for key in candidates::keys(&namespace, &record)? {
                    self.expect_entry(index, &key, &value)?;
                    candidates_implied += 1;
                }
                self.expect_entry(
                    index,
                    &ordered::scope_key(&namespace, &record),
                    namespace.as_bytes(),
                )?;
                scope_implied += 1;
                if let Some(address) = &address {
                    self.expect_entry(
                        index,
                        &ordered::company_key(address, &record)?,
                        namespace.as_bytes(),
                    )?;
                    report.chronological_company_entries += 1;
                }
            }
            // No entry may exist that the records do not imply.
            if self.count_prefix(index, &record_prefix(candidates::ROW, &namespace))?
                != candidates_implied
                || self.count_prefix(index, &record_prefix(ordered::SCOPE_ROW, &namespace))?
                    != scope_implied
            {
                return Err(inconsistent());
            }
            report.candidate_entries += candidates_implied;
            report.chronological_scope_entries += scope_implied;
            iterator.seek(successor(records));
        }
        iterator.status().map_err(storage_error)?;
        // Global counts also reject rows and tips in namespaces without canonical
        // records; a per-namespace walk alone would never inspect those keys.
        if self.count_prefix(index, &[candidates::ROW])? != report.candidate_entries
            || self.count_prefix(index, &[ordered::SCOPE_ROW])?
                != report.chronological_scope_entries
            || self.count_prefix(index, &[candidates::TIP])? != report.namespaces
            || self.count_prefix(index, &[ordered::TIP])? != report.namespaces
            || self.count_prefix(index, &[ordered::COMPANY_ROW])?
                != report.chronological_company_entries
        {
            return Err(inconsistent());
        }
        report.relation_head_entries = self.count_prefix(meta, &[relations::adjacency::HEAD])?;
        Ok(report)
    }
}
