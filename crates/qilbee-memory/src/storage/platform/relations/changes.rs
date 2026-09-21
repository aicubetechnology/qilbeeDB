//! A separate, history-bound journal for typed assertions; memory feeds stay unchanged.
use super::*;
mod types;
pub use types::*;
mod checkpoints;
pub use checkpoints::*;

const JOURNAL: u8 = 0x58;
const EVENT: u8 = 0x59;
const MAX_CHANGE_BYTES: usize = 4096;

fn hash<T: Serialize>(value: &T) -> Result<String> {
    Ok(hex_digest(&encode(value)?))
}
fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn event_key(namespace: &str, sequence: u64) -> Vec<u8> {
    let mut key = record_prefix(EVENT, namespace);
    key.extend(sequence.to_be_bytes());
    key
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RelationJournal {
    schema_version: u32,
    namespace: String,
    baseline: RelationChangeCursor,
    tip: RelationChangeCursor,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredChange {
    cursor: RelationChangeCursor,
    previous_digest: String,
    nonce: Uuid,
    change: RelationChange,
}
impl RelationChangeCursor {
    fn validate(&self) -> Result<()> {
        if self.version != 1 || !valid_digest(&self.prefix_digest) {
            return Err(Error::ValidationError(
                "Invalid relation cursor version or digest".into(),
            ));
        }
        Ok(())
    }
}
impl RelationJournal {
    fn baseline_digest(&self) -> Result<String> {
        hash(&(
            "qilbee.relations.baseline.v1",
            &self.namespace,
            &self.baseline.stream,
            self.baseline.journal_id,
        ))
    }
    fn new(namespace: &str) -> Result<Self> {
        let cursor = RelationChangeCursor {
            version: 1,
            stream: RelationChangeStream::TypedMemoryRelations,
            journal_id: Uuid::new_v4(),
            sequence: 0,
            prefix_digest: String::new(),
        };
        let mut journal = Self {
            schema_version: 1,
            namespace: namespace.into(),
            baseline: cursor.clone(),
            tip: cursor,
        };
        journal.baseline.prefix_digest = journal.baseline_digest()?;
        journal.tip = journal.baseline.clone();
        Ok(journal)
    }
}
impl StoredChange {
    fn digest(&self, namespace: &str) -> Result<String> {
        hash(&(
            "qilbee.relations.change.v1",
            namespace,
            self.cursor.version,
            &self.cursor.stream,
            self.cursor.journal_id,
            self.cursor.sequence,
            &self.previous_digest,
            self.nonce,
            &self.change,
        ))
    }
}
impl RelationChange {
    fn from_revision(relation: &MemoryRelation, receipt: &MemoryRelationReceipt) -> Self {
        Self {
            schema_version: 1,
            kind: receipt.action,
            relation_id: relation.relation_id,
            relation_revision: relation.revision,
            source: relation.input.source.clone(),
            target: relation.input.target.clone(),
            relation_kind: relation.input.kind,
            author: receipt.author.clone(),
            committed_at_millis: receipt.committed_at_millis,
            relation_digest: receipt.relation_digest.clone(),
            receipt_digest: receipt.receipt_digest.clone(),
        }
    }
}
impl MemorySnapshot<'_> {
    fn relation_metadata_prefix_exists(&self, prefix: Vec<u8>) -> Result<bool> {
        let mut end = prefix.clone();
        while let Some(last) = end.pop() {
            if last != u8::MAX {
                end.push(last + 1);
                break;
            }
        }
        let mut options = rocksdb::ReadOptions::default();
        options.set_iterate_lower_bound(prefix.clone());
        options.set_iterate_upper_bound(end);
        let mut iter = self
            .db
            .raw_iterator_cf_opt(self.storage.cf(crate::storage::cf::AGENT_META)?, options);
        iter.seek(&prefix);
        iter.status().map_err(storage_error)?;
        Ok(iter.valid())
    }

    fn relation_journal(&self, namespace: &str) -> Result<Option<RelationJournal>> {
        let cf = self.storage.cf(crate::storage::cf::AGENT_META)?;
        let Some(bytes) = self
            .db
            .get_cf(cf, record_prefix(JOURNAL, namespace))
            .map_err(storage_error)?
        else {
            if self.relation_metadata_prefix_exists(record_prefix(EVENT, namespace))? {
                return Err(inconsistent());
            }
            return Ok(None);
        };
        if bytes.len() > MAX_CHANGE_BYTES {
            return Err(inconsistent());
        }
        let state: RelationJournal = decode(&bytes)?;
        if state.schema_version != 1
            || state.namespace != namespace
            || state.baseline.validate().is_err()
            || state.tip.validate().is_err()
            || state.baseline.sequence != 0
            || state.baseline.journal_id != state.tip.journal_id
            || state.baseline.prefix_digest != state.baseline_digest()?
            || self.relation_cursor_at(namespace, &state, state.tip.sequence)? != state.tip
        {
            return Err(inconsistent());
        }
        if let Some(next) = state.tip.sequence.checked_add(1) {
            if self
                .db
                .get_cf(cf, event_key(namespace, next))
                .map_err(storage_error)?
                .is_some()
            {
                return Err(inconsistent());
            }
        }
        Ok(Some(state))
    }
    fn relation_change(
        &self,
        namespace: &str,
        state: &RelationJournal,
        sequence: u64,
    ) -> Result<StoredChange> {
        let bytes = self
            .db
            .get_cf(
                self.storage.cf(crate::storage::cf::AGENT_META)?,
                event_key(namespace, sequence),
            )
            .map_err(storage_error)?
            .ok_or_else(inconsistent)?;
        if bytes.len() > MAX_CHANGE_BYTES {
            return Err(inconsistent());
        }
        let node: StoredChange = decode(&bytes)?;
        if sequence == 0
            || node.cursor.validate().is_err()
            || node.cursor.journal_id != state.tip.journal_id
            || node.cursor.sequence != sequence
            || !valid_digest(&node.previous_digest)
            || node.cursor.prefix_digest != node.digest(namespace)?
        {
            return Err(inconsistent());
        }
        let history = self
            .relation_revision(
                namespace,
                node.change.relation_id,
                node.change.relation_revision,
            )?
            .ok_or_else(inconsistent)?;
        if node.change != RelationChange::from_revision(&history.relation, &history.receipt) {
            return Err(inconsistent());
        }
        Ok(node)
    }
    fn relation_cursor_at(
        &self,
        namespace: &str,
        state: &RelationJournal,
        sequence: u64,
    ) -> Result<RelationChangeCursor> {
        if sequence == 0 {
            return Ok(state.baseline.clone());
        }
        Ok(self.relation_change(namespace, state, sequence)?.cursor)
    }
    fn verify_relation_cursor(
        &self,
        namespace: &str,
        state: &RelationJournal,
        cursor: &RelationChangeCursor,
    ) -> Result<()> {
        cursor.validate()?;
        if cursor.journal_id != state.tip.journal_id
            || cursor.sequence > state.tip.sequence
            || self.relation_cursor_at(namespace, state, cursor.sequence)? != *cursor
        {
            return Err(Error::JournalHistoryConflict(
                "Relation cursor does not match this scoped history; reconcile restored state"
                    .into(),
            ));
        }
        Ok(())
    }
}
impl RocksDbMemoryStorage {
    /// Caller holds mutation_lock and commits this batch with the assertion, indexes and receipt.
    pub(super) fn append_relation_change(
        &self,
        view: &MemorySnapshot<'_>,
        namespace: &str,
        relation: &MemoryRelation,
        receipt: &MemoryRelationReceipt,
        batch: &mut rocksdb::WriteBatch,
    ) -> Result<()> {
        let mut state = match view.relation_journal(namespace)? {
            Some(state) => state,
            None => RelationJournal::new(namespace)?,
        };
        let sequence =
            state.tip.sequence.checked_add(1).ok_or_else(|| {
                Error::ValidationError("Relation journal sequence exhausted".into())
            })?;
        let mut node = StoredChange {
            cursor: RelationChangeCursor {
                sequence,
                ..state.tip.clone()
            },
            previous_digest: state.tip.prefix_digest.clone(),
            nonce: Uuid::new_v4(),
            change: RelationChange::from_revision(relation, receipt),
        };
        node.cursor.prefix_digest = node.digest(namespace)?;
        let bytes = encode(&node)?;
        if bytes.len() > MAX_CHANGE_BYTES {
            return Err(Error::ValidationError(
                "Relation journal entry exceeds 4 KiB".into(),
            ));
        }
        state.tip = node.cursor.clone();
        let cf = self.cf(crate::storage::cf::AGENT_META)?;
        batch.put_cf(cf, event_key(namespace, sequence), bytes);
        batch.put_cf(cf, record_prefix(JOURNAL, namespace), encode(&state)?);
        Ok(())
    }
    /// Activate a zero-position baseline without inventing historical relation events.
    pub fn activate_relation_changes(&self, namespace: &str) -> Result<RelationChangeCursor> {
        Self::validate_agent(namespace)?;
        let _guard = self
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Memory mutation lock poisoned".into()))?;
        if let Some(state) = self.memory_snapshot().relation_journal(namespace)? {
            return Ok(state.baseline);
        }
        let state = RelationJournal::new(namespace)?;
        let mut batch = rocksdb::WriteBatch::default();
        batch.put_cf(
            self.cf(crate::storage::cf::AGENT_META)?,
            record_prefix(JOURNAL, namespace),
            encode(&state)?,
        );
        self.commit_relation_consumer_batch(batch)?;
        Ok(state.baseline)
    }
    /// Read a bounded, history-verified suffix; the cursor is not authorization or a freshness lease.
    pub fn relation_changes(
        &self,
        namespace: &str,
        query: &RelationChangesQuery,
    ) -> Result<RelationChangesPage> {
        Self::validate_agent(namespace)?;
        if !(1..=256).contains(&query.limit) {
            return Err(Error::ValidationError(
                "Relation change limit must be 1 to 256".into(),
            ));
        }
        for cursor in [&query.after, &query.through].into_iter().flatten() {
            cursor.validate()?;
        }
        let view = self.memory_snapshot();
        let Some(state) = view.relation_journal(namespace)? else {
            if query.after.is_some() || query.through.is_some() {
                return Err(Error::JournalHistoryConflict(
                    "No relation journal in this scope".into(),
                ));
            }
            return Ok(RelationChangesPage {
                active: false,
                baseline: None,
                changes: vec![],
                next_cursor: None,
                high_watermark: None,
                complete: true,
            });
        };
        for cursor in [&query.after, &query.through].into_iter().flatten() {
            view.verify_relation_cursor(namespace, &state, cursor)?;
        }
        let mut previous = query.after.clone().unwrap_or(state.baseline.clone());
        let through = query.through.clone().unwrap_or(state.tip.clone());
        if previous.sequence > through.sequence {
            return Err(Error::ValidationError(
                "Relation cursor exceeds requested fence".into(),
            ));
        }
        let count = (through.sequence - previous.sequence).min(query.limit as u64);
        let mut changes = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let node = view.relation_change(namespace, &state, previous.sequence + 1)?;
            if node.previous_digest != previous.prefix_digest {
                return Err(inconsistent());
            }
            previous = node.cursor.clone();
            changes.push(RelationChangeDelivery {
                cursor: node.cursor,
                change: node.change,
            });
        }
        Ok(RelationChangesPage {
            active: true,
            baseline: Some(state.baseline),
            complete: previous == through,
            changes,
            next_cursor: Some(previous),
            high_watermark: Some(through),
        })
    }
    fn commit_relation_consumer_batch(&self, batch: rocksdb::WriteBatch) -> Result<()> {
        let mut options = rocksdb::WriteOptions::default();
        options.disable_wal(false);
        options.set_sync(true);
        self.db.write_opt(batch, &options).map_err(storage_error)
    }
}

#[cfg(test)]
mod tests;
