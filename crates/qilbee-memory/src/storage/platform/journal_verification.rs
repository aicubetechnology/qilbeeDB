//! Offline verification of every memory journal in a stopped store.
//!
//! The physical inventory proves that bytes were copied faithfully; this walk
//! proves that each namespace's legacy journal and verified anchor chain are
//! internally consistent: physical keys are canonical and cover exactly the
//! recorded ranges, every legacy event validates, and every anchor's digest and
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

#[derive(Default)]
struct PhysicalRange {
    rows: u64,
    first: Option<u64>,
    last: Option<u64>,
}

impl PhysicalRange {
    fn observe(&mut self, sequence: u64) -> Result<()> {
        self.rows = self.rows.checked_add(1).ok_or_else(inconsistent)?;
        self.first = Some(self.first.map_or(sequence, |first| first.min(sequence)));
        self.last = Some(self.last.map_or(sequence, |last| last.max(sequence)));
        Ok(())
    }

    fn require(&self, rows: u64, first: Option<u64>, last: Option<u64>) -> Result<()> {
        if self.rows != rows || self.first != first || self.last != last {
            return Err(inconsistent());
        }
        Ok(())
    }
}

#[derive(Default)]
struct PhysicalJournal {
    changes: PhysicalRange,
    anchors: PhysicalRange,
}

fn namespace_of(key: &[u8]) -> Result<(String, Option<u64>)> {
    if key.len() < 3 {
        return Err(inconsistent());
    }
    let length = u16::from_be_bytes([key[1], key[2]]) as usize;
    let end = 3 + length;
    let sequence = match key[0] {
        0x20 | 0x26 if length > 0 && key.len() == end => None,
        0x21 | 0x27 if length > 0 && key.len() == end + 8 => {
            let sequence = u64::from_be_bytes(key[end..].try_into().map_err(|_| inconsistent())?);
            if sequence == 0 {
                return Err(inconsistent());
            }
            Some(sequence)
        }
        _ => return Err(inconsistent()),
    };
    let namespace = std::str::from_utf8(&key[3..end])
        .map(str::to_owned)
        .map_err(|_| inconsistent())?;
    Ok((namespace, sequence))
}

impl RocksDbMemoryStorage {
    /// Walk every journal. Any namespace that owns a journal state, a change
    /// record or an anchor is visited, so orphaned records fail rather than
    /// hide behind a missing state record.
    pub fn verify_memory_journals(&self) -> Result<MemoryJournalVerification> {
        let _guard = self
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Memory mutation lock poisoned".into()))?;
        let view = self.memory_snapshot();
        let meta = self.cf(super::super::cf::AGENT_META)?;
        let mut namespaces = std::collections::BTreeMap::<String, PhysicalJournal>::new();
        for kind in NAMESPACE_KINDS {
            for item in view.db.iterator_cf(
                meta,
                rocksdb::IteratorMode::From(&[kind], rocksdb::Direction::Forward),
            ) {
                let (key, _) = item.map_err(storage_error)?;
                if key.first() != Some(&kind) {
                    break;
                }
                let (namespace, sequence) = namespace_of(&key)?;
                let physical = namespaces.entry(namespace).or_default();
                match (kind, sequence) {
                    (0x21, Some(sequence)) => physical.changes.observe(sequence)?,
                    (0x27, Some(sequence)) => physical.anchors.observe(sequence)?,
                    _ => {}
                }
            }
        }
        let mut report = MemoryJournalVerification {
            namespaces: namespaces.len() as u64,
            legacy_journals: 0,
            verified_journals: 0,
            links_checked: 0,
        };
        for (namespace, physical) in namespaces {
            if let Some(state) = view.journal(&namespace)? {
                // Canonical keys are unique. Equal count and bounds therefore
                // establish every sequence from one through the recorded tip.
                // Check this before walking: a forged huge tip cannot amplify
                // work beyond the physical rows observed above.
                physical.changes.require(
                    state.cursor.sequence,
                    Some(1),
                    Some(state.cursor.sequence),
                )?;
                for sequence in 1..=state.cursor.sequence {
                    view.change(
                        &namespace,
                        &MemoryChangeCursor {
                            journal_id: state.cursor.journal_id,
                            sequence,
                        },
                    )?;
                }
                report.legacy_journals += 1;
            } else {
                physical.changes.require(0, None, None)?;
            }
            let Some(state) = view.verified_journal(&namespace)? else {
                physical.anchors.require(0, None, None)?;
                continue;
            };
            let anchors = state
                .tip
                .sequence
                .checked_sub(state.baseline.sequence)
                .ok_or_else(inconsistent)?;
            if anchors == 0 {
                physical.anchors.require(0, None, None)?;
            } else {
                physical.anchors.require(
                    anchors,
                    Some(
                        state
                            .baseline
                            .sequence
                            .checked_add(1)
                            .ok_or_else(inconsistent)?,
                    ),
                    Some(state.tip.sequence),
                )?;
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
