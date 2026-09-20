//! Atomic, scope-local mutation journal. Events carry identities, never memory content.
use super::snapshot::MemorySnapshot;
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryChangeKind {
    Created,
    Updated,
    Deleted,
    EmbeddingAttached,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryChangeCursor {
    pub journal_id: Uuid,
    pub sequence: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryChange {
    pub schema_version: u32,
    pub cursor: MemoryChangeCursor,
    pub kind: MemoryChangeKind,
    pub record_id: Uuid,
    pub record_revision: u64,
    pub author: RecordAuthor,
    pub committed_at_millis: i64,
    pub change_digest: String,
}
impl MemoryChange {
    fn digest(&self) -> Result<String> {
        Ok(digest(&encode(&(
            self.schema_version,
            &self.cursor,
            self.kind,
            self.record_id,
            self.record_revision,
            &self.author,
            self.committed_at_millis,
        ))?)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryChangesQuery {
    pub after: Option<MemoryChangeCursor>,
    pub through: Option<MemoryChangeCursor>,
    pub limit: usize,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryChangesPage {
    pub changes: Vec<MemoryChange>,
    pub next_cursor: Option<MemoryChangeCursor>,
    pub high_watermark: Option<MemoryChangeCursor>,
    pub complete: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct JournalState {
    schema_version: u32,
    namespace: String,
    pub(super) cursor: MemoryChangeCursor,
    last_change_digest: String,
}
fn change_key(namespace: &str, sequence: u64) -> Vec<u8> {
    let mut key = record_prefix(0x21, namespace);
    key.extend_from_slice(&sequence.to_be_bytes());
    key
}
impl MemorySnapshot<'_> {
    pub(super) fn journal(&self, namespace: &str) -> Result<Option<JournalState>> {
        let cf = self.storage.cf(super::super::cf::AGENT_META)?;
        let Some(bytes) = self
            .db
            .get_cf(cf, record_prefix(0x20, namespace))
            .map_err(storage_error)?
        else {
            let prefix = record_prefix(0x21, namespace);
            if let Some(entry) = self
                .db
                .iterator_cf(
                    cf,
                    rocksdb::IteratorMode::From(&prefix, rocksdb::Direction::Forward),
                )
                .next()
            {
                let (key, _) = entry.map_err(storage_error)?;
                if key.starts_with(&prefix) {
                    return Err(inconsistent());
                }
            }
            return Ok(None);
        };
        let state: JournalState = decode(&bytes)?;
        if state.schema_version != 1 || state.namespace != namespace || state.cursor.sequence == 0 {
            return Err(inconsistent());
        }
        let last = self.change(namespace, &state.cursor)?;
        if last.change_digest != state.last_change_digest {
            return Err(inconsistent());
        }
        if let Some(next) = state.cursor.sequence.checked_add(1) {
            if self
                .db
                .get_cf(cf, change_key(namespace, next))
                .map_err(storage_error)?
                .is_some()
            {
                return Err(inconsistent());
            }
        }
        Ok(Some(state))
    }
    fn change(&self, namespace: &str, cursor: &MemoryChangeCursor) -> Result<MemoryChange> {
        let bytes = self
            .db
            .get_cf(
                self.storage.cf(super::super::cf::AGENT_META)?,
                change_key(namespace, cursor.sequence),
            )
            .map_err(storage_error)?
            .ok_or_else(inconsistent)?;
        let change: MemoryChange = decode(&bytes)?;
        if change.schema_version != 1
            || change.cursor != *cursor
            || change.record_revision == 0
            || change.change_digest != change.digest()?
        {
            return Err(inconsistent());
        }
        Ok(change)
    }
}
impl RocksDbMemoryStorage {
    /// Caller holds mutation_lock and commits this batch together with the actual mutation.
    pub(super) fn append_memory_change(
        &self,
        namespace: &str,
        batch: &mut rocksdb::WriteBatch,
        kind: MemoryChangeKind,
        record_id: Uuid,
        record_revision: u64,
        author: &RecordAuthor,
        now: i64,
    ) -> Result<()> {
        let previous = self.memory_snapshot().journal(namespace)?;
        let cursor = match previous {
            Some(state) => MemoryChangeCursor {
                journal_id: state.cursor.journal_id,
                sequence: state.cursor.sequence.checked_add(1).ok_or_else(|| {
                    Error::TransactionConflict("Memory journal sequence exhausted".into())
                })?,
            },
            None => MemoryChangeCursor {
                journal_id: Uuid::new_v4(),
                sequence: 1,
            },
        };
        let mut change = MemoryChange {
            schema_version: 1,
            cursor: cursor.clone(),
            kind,
            record_id,
            record_revision,
            author: author.clone(),
            committed_at_millis: now,
            change_digest: String::new(),
        };
        change.change_digest = change.digest()?;
        let state = JournalState {
            schema_version: 1,
            namespace: namespace.into(),
            cursor,
            last_change_digest: change.change_digest.clone(),
        };
        let cf = self.cf(super::super::cf::AGENT_META)?;
        batch.put_cf(
            cf,
            change_key(namespace, state.cursor.sequence),
            encode(&change)?,
        );
        batch.put_cf(cf, record_prefix(0x20, namespace), encode(&state)?);
        Ok(())
    }
    /// Snapshot-fenced events since activation; pre-existing records require initial reconciliation.
    pub fn memory_changes(
        &self,
        namespace: &str,
        query: &MemoryChangesQuery,
    ) -> Result<MemoryChangesPage> {
        Self::validate_agent(namespace)?;
        if !(1..=256).contains(&query.limit) {
            return Err(Error::ValidationError(
                "Change limit must be 1 to 256".into(),
            ));
        }
        let view = self.memory_snapshot();
        let state = view.journal(namespace)?;
        let Some(state) = state else {
            if query.after.is_some() || query.through.is_some() {
                return Err(Error::ConstraintViolation(
                    "No matching journal in this scope".into(),
                ));
            }
            return Ok(MemoryChangesPage {
                changes: vec![],
                next_cursor: None,
                high_watermark: None,
                complete: true,
            });
        };
        for cursor in [&query.after, &query.through].into_iter().flatten() {
            if cursor.journal_id != state.cursor.journal_id
                || cursor.sequence > state.cursor.sequence
            {
                return Err(Error::ConstraintViolation(
                    "Change cursor is foreign or ahead of this journal".into(),
                ));
            }
        }
        let after = query.after.as_ref().map_or(0, |c| c.sequence);
        let through = query.through.clone().unwrap_or(state.cursor.clone());
        if after > through.sequence {
            return Err(Error::ValidationError(
                "Change cursor exceeds the requested fence".into(),
            ));
        }
        let count = (through.sequence - after).min(query.limit as u64);
        let mut changes = Vec::new();
        for step in 1..=count {
            changes.push(view.change(
                namespace,
                &MemoryChangeCursor {
                    journal_id: state.cursor.journal_id,
                    sequence: after + step,
                },
            )?);
        }
        Ok(MemoryChangesPage {
            changes,
            next_cursor: Some(MemoryChangeCursor {
                journal_id: state.cursor.journal_id,
                sequence: after + count,
            }),
            high_watermark: Some(through),
            complete: after + count
                == query
                    .through
                    .as_ref()
                    .map_or(state.cursor.sequence, |c| c.sequence),
        })
    }
}

#[cfg(test)]
mod integrity_tests {
    use super::*;
    #[test]
    fn memory_change_corruption_is_explicit_and_old_records_are_not_backfilled() {
        let dir = tempfile::TempDir::new().unwrap();
        let db = RocksDbMemoryStorage::open(crate::MemoryStorageConfig {
            path: dir.path().to_str().unwrap().into(),
            ..Default::default()
        })
        .unwrap();
        let actor = RecordAuthor {
            credential_id: Uuid::new_v4(),
            subject_id: "a".into(),
        };
        let command:MemoryCommand=serde_json::from_value(serde_json::json!({"contract_version":1,"idempotency_key":"one","operation":{"type":"create","record":{"episode_type":"Observation","event_time_millis":1,"content":{"primary":"old"}}}})).unwrap();
        let receipt = db.apply_memory_command("scope", &actor, &command).unwrap();
        let cf = db.cf(super::super::super::cf::AGENT_META).unwrap();
        let query = MemoryChangesQuery {
            after: None,
            through: None,
            limit: 10,
        };
        let mut event = db
            .memory_changes("scope", &query)
            .unwrap()
            .changes
            .remove(0);
        event.record_revision = 7;
        db.db
            .put_cf(cf, change_key("scope", 1), encode(&event).unwrap())
            .unwrap();
        assert!(matches!(
            db.memory_changes("scope", &query),
            Err(Error::DataCorruption(_))
        ));
        db.db.delete_cf(cf, change_key("scope", 1)).unwrap();
        assert!(matches!(
            db.memory_changes("scope", &query),
            Err(Error::DataCorruption(_))
        ));
        // Simulate a pre-journal record/receipt pair. An old retry creates no synthetic history.
        db.db.delete_cf(cf, record_prefix(0x20, "scope")).unwrap();
        assert_eq!(
            db.apply_memory_command("scope", &actor, &command).unwrap(),
            receipt
        );
        assert!(
            db.memory_changes("scope", &query)
                .unwrap()
                .changes
                .is_empty()
        );
        assert!(
            db.read_memory_record("scope", receipt.record_id)
                .unwrap()
                .is_some()
        );
    }
}
