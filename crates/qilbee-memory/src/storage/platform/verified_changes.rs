//! Prefix-anchored continuations. Legacy journal events remain wire-compatible.
use super::snapshot::MemorySnapshot;
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifiedMemoryCursor {
    pub version: u32,
    pub journal_id: Uuid,
    pub generation: Uuid,
    pub sequence: u64,
    pub prefix_digest: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifiedMemoryChangesQuery {
    pub after: Option<VerifiedMemoryCursor>,
    pub through: Option<VerifiedMemoryCursor>,
    pub limit: usize,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifiedMemoryChange {
    pub cursor: VerifiedMemoryCursor,
    pub change: MemoryChange,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifiedMemoryChangesPage {
    pub active: bool,
    pub baseline: Option<VerifiedMemoryCursor>,
    pub changes: Vec<VerifiedMemoryChange>,
    pub next_cursor: Option<VerifiedMemoryCursor>,
    pub high_watermark: Option<VerifiedMemoryCursor>,
    pub complete: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct VerifiedJournal {
    schema_version: u32,
    namespace: String,
    pub(super) baseline: VerifiedMemoryCursor,
    pub(super) tip: VerifiedMemoryCursor,
    legacy_tip_digest: Option<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Anchor {
    cursor: VerifiedMemoryCursor,
    previous_digest: String,
    nonce: Uuid,
    event_digest: String,
}
fn hash<T: Serialize>(value: &T) -> Result<String> {
    Ok(digest(&encode(value)?)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}
fn anchor_key(namespace: &str, sequence: u64) -> Vec<u8> {
    let mut key = record_prefix(0x27, namespace);
    key.extend_from_slice(&sequence.to_be_bytes());
    key
}
fn valid_digest(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
impl VerifiedMemoryCursor {
    pub(super) fn validate(&self) -> Result<()> {
        if self.version != 2 || !valid_digest(&self.prefix_digest) {
            return Err(Error::ValidationError(
                "Invalid verified cursor version or digest".into(),
            ));
        }
        Ok(())
    }
    fn legacy(&self) -> MemoryChangeCursor {
        MemoryChangeCursor {
            journal_id: self.journal_id,
            sequence: self.sequence,
        }
    }
}
impl VerifiedJournal {
    fn baseline_digest(&self) -> Result<String> {
        hash(&(
            "qilbee.memory.journal.baseline.v2",
            &self.namespace,
            self.baseline.journal_id,
            self.baseline.generation,
            self.baseline.sequence,
            &self.legacy_tip_digest,
        ))
    }
    fn new(
        namespace: &str,
        journal_id: Uuid,
        sequence: u64,
        legacy_tip_digest: Option<String>,
    ) -> Result<Self> {
        let baseline = VerifiedMemoryCursor {
            version: 2,
            journal_id,
            generation: Uuid::new_v4(),
            sequence,
            prefix_digest: String::new(),
        };
        let mut state = Self {
            schema_version: 1,
            namespace: namespace.into(),
            tip: baseline.clone(),
            baseline,
            legacy_tip_digest,
        };
        state.baseline.prefix_digest = state.baseline_digest()?;
        state.tip = state.baseline.clone();
        Ok(state)
    }
}
impl Anchor {
    fn digest(&self, namespace: &str) -> Result<String> {
        hash(&(
            "qilbee.memory.journal.anchor.v2",
            namespace,
            self.cursor.version,
            self.cursor.journal_id,
            self.cursor.generation,
            self.cursor.sequence,
            &self.previous_digest,
            self.nonce,
            &self.event_digest,
        ))
    }
}
impl MemorySnapshot<'_> {
    pub(super) fn verified_journal(&self, namespace: &str) -> Result<Option<VerifiedJournal>> {
        let cf = self.storage.cf(super::super::cf::AGENT_META)?;
        let legacy = self.journal(namespace)?;
        let Some(bytes) = self
            .db
            .get_cf(cf, record_prefix(0x26, namespace))
            .map_err(storage_error)?
        else {
            let prefix = record_prefix(0x27, namespace);
            if let Some(entry) = self
                .db
                .iterator_cf(
                    cf,
                    rocksdb::IteratorMode::From(&prefix, rocksdb::Direction::Forward),
                )
                .next()
            {
                if entry.map_err(storage_error)?.0.starts_with(&prefix) {
                    return Err(inconsistent());
                }
            }
            return Ok(None);
        };
        let state: VerifiedJournal = decode(&bytes)?;
        if state.schema_version != 1
            || state.namespace != namespace
            || state.baseline.validate().is_err()
            || state.tip.validate().is_err()
            || state.baseline.journal_id != state.tip.journal_id
            || state.baseline.generation != state.tip.generation
            || state.baseline.sequence > state.tip.sequence
            || state.baseline.prefix_digest != state.baseline_digest()?
            || (state.baseline.sequence == 0) != state.legacy_tip_digest.is_none()
            || state
                .legacy_tip_digest
                .as_ref()
                .is_some_and(|d| !valid_digest(d))
        {
            return Err(inconsistent());
        }
        match legacy {
            Some(legacy) if legacy.cursor == state.tip.legacy() => {}
            None if state.tip.sequence == 0 => {}
            _ => return Err(inconsistent()),
        }
        if state.baseline.sequence > 0 {
            let event = self.change(namespace, &state.baseline.legacy())?;
            if Some(event.change_digest) != state.legacy_tip_digest {
                return Err(inconsistent());
            }
        }
        if self.verified_cursor_at(namespace, &state, state.tip.sequence)? != state.tip {
            return Err(inconsistent());
        }
        if let Some(next) = state.tip.sequence.checked_add(1) {
            if self
                .db
                .get_cf(cf, anchor_key(namespace, next))
                .map_err(storage_error)?
                .is_some()
            {
                return Err(inconsistent());
            }
        }
        Ok(Some(state))
    }
    fn anchor(
        &self,
        namespace: &str,
        state: &VerifiedJournal,
        sequence: u64,
    ) -> Result<(Anchor, MemoryChange)> {
        let bytes = self
            .db
            .get_cf(
                self.storage.cf(super::super::cf::AGENT_META)?,
                anchor_key(namespace, sequence),
            )
            .map_err(storage_error)?
            .ok_or_else(inconsistent)?;
        let node: Anchor = decode(&bytes)?;
        if node.cursor.validate().is_err()
            || node.cursor.journal_id != state.tip.journal_id
            || node.cursor.generation != state.tip.generation
            || node.cursor.sequence != sequence
            || !valid_digest(&node.previous_digest)
            || node.cursor.prefix_digest != node.digest(namespace)?
        {
            return Err(inconsistent());
        }
        let event = self.change(namespace, &node.cursor.legacy())?;
        if node.event_digest != event.change_digest {
            return Err(inconsistent());
        }
        Ok((node, event))
    }
    fn verified_cursor_at(
        &self,
        namespace: &str,
        state: &VerifiedJournal,
        sequence: u64,
    ) -> Result<VerifiedMemoryCursor> {
        if sequence == state.baseline.sequence {
            return Ok(state.baseline.clone());
        }
        Ok(self.anchor(namespace, state, sequence)?.0.cursor)
    }
    pub(super) fn verify_memory_cursor(
        &self,
        namespace: &str,
        state: &VerifiedJournal,
        cursor: &VerifiedMemoryCursor,
    ) -> Result<()> {
        cursor.validate()?;
        if cursor.journal_id != state.tip.journal_id
            || cursor.generation != state.tip.generation
            || cursor.sequence < state.baseline.sequence
            || cursor.sequence > state.tip.sequence
            || self.verified_cursor_at(namespace, state, cursor.sequence)? != *cursor
        {
            return Err(Error::ConstraintViolation(
                "Verified cursor does not match this journal history; reconcile restored state"
                    .into(),
            ));
        }
        Ok(())
    }
}
impl RocksDbMemoryStorage {
    /// Called while holding mutation_lock, before the legacy event and mutation are committed.
    pub(super) fn append_verified_change(
        &self,
        namespace: &str,
        batch: &mut rocksdb::WriteBatch,
        event: &MemoryChange,
    ) -> Result<()> {
        let view = self.memory_snapshot();
        let mut state = match view.verified_journal(namespace)? {
            Some(state) => state,
            None => {
                let legacy = view.journal(namespace)?;
                VerifiedJournal::new(
                    namespace,
                    event.cursor.journal_id,
                    event.cursor.sequence - 1,
                    legacy.map(|s| s.last_change_digest),
                )?
            }
        };
        if state.tip.journal_id != event.cursor.journal_id
            || state.tip.sequence.checked_add(1) != Some(event.cursor.sequence)
        {
            return Err(inconsistent());
        }
        let mut node = Anchor {
            cursor: VerifiedMemoryCursor {
                sequence: event.cursor.sequence,
                ..state.tip.clone()
            },
            previous_digest: state.tip.prefix_digest.clone(),
            nonce: Uuid::new_v4(),
            event_digest: event.change_digest.clone(),
        };
        node.cursor.prefix_digest = node.digest(namespace)?;
        state.tip = node.cursor.clone();
        let cf = self.cf(super::super::cf::AGENT_META)?;
        batch.put_cf(
            cf,
            anchor_key(namespace, event.cursor.sequence),
            encode(&node)?,
        );
        batch.put_cf(cf, record_prefix(0x26, namespace), encode(&state)?);
        Ok(())
    }
    /// Return an anchored suffix. The baseline is an upgrade boundary, not proof of legacy history.
    pub fn verified_memory_changes(
        &self,
        namespace: &str,
        query: &VerifiedMemoryChangesQuery,
    ) -> Result<VerifiedMemoryChangesPage> {
        Self::validate_agent(namespace)?;
        if !(1..=256).contains(&query.limit) {
            return Err(Error::ValidationError(
                "Change limit must be 1 to 256".into(),
            ));
        }
        for cursor in [&query.after, &query.through].into_iter().flatten() {
            cursor.validate()?;
        }
        let view = self.memory_snapshot();
        let Some(state) = view.verified_journal(namespace)? else {
            if query.after.is_some() || query.through.is_some() {
                return Err(Error::ConstraintViolation(
                    "No verified journal in this scope".into(),
                ));
            }
            return Ok(VerifiedMemoryChangesPage {
                active: false,
                baseline: None,
                changes: vec![],
                next_cursor: None,
                high_watermark: None,
                complete: true,
            });
        };
        for cursor in [&query.after, &query.through].into_iter().flatten() {
            view.verify_memory_cursor(namespace, &state, cursor)?;
        }
        let mut previous = query.after.clone().unwrap_or(state.baseline.clone());
        let through = query.through.clone().unwrap_or(state.tip.clone());
        if previous.sequence > through.sequence {
            return Err(Error::ValidationError(
                "Change cursor exceeds the requested fence".into(),
            ));
        }
        let count = (through.sequence - previous.sequence).min(query.limit as u64);
        let mut changes = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let (node, event) = view.anchor(namespace, &state, previous.sequence + 1)?;
            if node.previous_digest != previous.prefix_digest {
                return Err(inconsistent());
            }
            previous = node.cursor;
            changes.push(VerifiedMemoryChange {
                cursor: previous.clone(),
                change: event,
            });
        }
        Ok(VerifiedMemoryChangesPage {
            active: true,
            baseline: Some(state.baseline),
            complete: previous.sequence == through.sequence,
            changes,
            next_cursor: Some(previous),
            high_watermark: Some(through),
        })
    }
}

impl RocksDbMemoryStorage {
    /// Idempotently establish a baseline without inventing a memory mutation.
    pub fn activate_verified_memory_journal(
        &self,
        namespace: &str,
    ) -> Result<VerifiedMemoryCursor> {
        Self::validate_agent(namespace)?;
        let _guard = self
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Memory mutation lock poisoned".into()))?;
        let view = self.memory_snapshot();
        if let Some(state) = view.verified_journal(namespace)? {
            return Ok(state.baseline);
        }
        let state = match view.journal(namespace)? {
            Some(legacy) => VerifiedJournal::new(
                namespace,
                legacy.cursor.journal_id,
                legacy.cursor.sequence,
                Some(legacy.last_change_digest),
            )?,
            None => VerifiedJournal::new(namespace, Uuid::new_v4(), 0, None)?,
        };
        let mut batch = rocksdb::WriteBatch::default();
        batch.put_cf(
            self.cf(super::super::cf::AGENT_META)?,
            record_prefix(0x26, namespace),
            encode(&state)?,
        );
        let mut options = rocksdb::WriteOptions::default();
        options.disable_wal(false);
        options.set_sync(true);
        self.db.write_opt(batch, &options).map_err(storage_error)?;
        Ok(state.baseline)
    }
}
