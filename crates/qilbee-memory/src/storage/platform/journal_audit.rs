//! Bounded journal integrity checks using the same snapshot and chain validation as the feed.
use super::*;

/// Coverage of one selected anchored range, not an attestation of the whole database.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryJournalAudit {
    pub active: bool,
    pub baseline: Option<VerifiedMemoryCursor>,
    pub checked_after: Option<VerifiedMemoryCursor>,
    pub checked_through: Option<VerifiedMemoryCursor>,
    pub high_watermark: Option<VerifiedMemoryCursor>,
    pub links_checked: u64,
    pub complete: bool,
}

impl RocksDbMemoryStorage {
    /// Check a bounded range without exposing event metadata or advancing consumer progress.
    ///
    /// Start at the baseline and retain the first high watermark to cover the anchored suffix.
    /// A successful page says nothing about older, later or unrelated data. Counts exclude
    /// constant-size baseline, cursor and tip checks performed by the verified feed.
    pub fn audit_memory_journal(
        &self,
        namespace: &str,
        query: &VerifiedMemoryChangesQuery,
    ) -> Result<MemoryJournalAudit> {
        let page = self.verified_memory_changes(namespace, query)?;
        Ok(MemoryJournalAudit {
            active: page.active,
            checked_after: query.after.clone().or_else(|| page.baseline.clone()),
            baseline: page.baseline,
            checked_through: page.next_cursor,
            high_watermark: page.high_watermark,
            links_checked: page.changes.len() as u64,
            complete: page.complete,
        })
    }
}
