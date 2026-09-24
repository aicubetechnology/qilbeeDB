//! Offline verification of every memory journal in a stopped store.
//!
//! The physical inventory proves that bytes were copied faithfully; this walk
//! proves that each namespace's legacy journal and verified anchor chain are
//! internally consistent: state records decode and bind their namespace, the
//! recorded tip has no dangling successor, every anchor's digest and
//! previous-digest link hold from the baseline to the tip, and every anchor's
//! event digest matches the stored change. Nothing is written or repaired.
use super::*;

/// Counts observed while walking every memory journal of a stopped store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryJournalVerification {
    /// Namespaces with any journal state, change or anchor record.
    pub namespaces: u64,
    /// Namespaces whose legacy journal validated.
    pub legacy_journals: u64,
    /// Namespaces whose verified anchor chain validated from baseline to tip.
    pub verified_journals: u64,
    /// Anchor links checked across all verified journals.
    pub links_checked: u64,
}

const NAMESPACE_KINDS: [u8; 4] = [0x20, 0x21, 0x26, 0x27];

fn namespace_of(key: &[u8]) -> Result<String> {
    if key.len() < 3 {
        return Err(inconsistent());
    }
    let length = u16::from_be_bytes([key[1], key[2]]) as usize;
    if key.len() < 3 + length {
        return Err(inconsistent());
    }
    std::str::from_utf8(&key[3..3 + length])
        .map(str::to_owned)
        .map_err(|_| inconsistent())
}

impl RocksDbMemoryStorage {
    /// Walk every journal. Any namespace that owns a journal state, a change
    /// record or an anchor is visited, so orphaned records fail rather than
    /// hide behind a missing state record.
    pub fn verify_memory_journals(&self) -> Result<MemoryJournalVerification> {
        let meta = self.cf(super::super::cf::AGENT_META)?;
        let mut namespaces = std::collections::BTreeSet::new();
        for kind in NAMESPACE_KINDS {
            for item in self.db.iterator_cf(
                meta,
                rocksdb::IteratorMode::From(&[kind], rocksdb::Direction::Forward),
            ) {
                let (key, _) = item.map_err(storage_error)?;
                if key.first() != Some(&kind) {
                    break;
                }
                namespaces.insert(namespace_of(&key)?);
            }
        }
        let mut report = MemoryJournalVerification {
            namespaces: namespaces.len() as u64,
            legacy_journals: 0,
            verified_journals: 0,
            links_checked: 0,
        };
        for namespace in namespaces {
            let view = self.memory_snapshot();
            if view.journal(&namespace)?.is_some() {
                report.legacy_journals += 1;
            }
            if view.verified_journal(&namespace)?.is_none() {
                continue;
            }
            report.verified_journals += 1;
            let mut query = VerifiedMemoryChangesQuery {
                after: None,
                through: None,
                limit: 256,
            };
            loop {
                let page = self.verified_memory_changes(&namespace, &query)?;
                report.links_checked += page.changes.len() as u64;
                if page.complete {
                    break;
                }
                if page.changes.is_empty() {
                    return Err(inconsistent());
                }
                query.after = page.next_cursor;
                query.through = page.high_watermark;
            }
        }
        Ok(report)
    }
}
