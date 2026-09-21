//! Subject-owned progress with immutable revisions and explicit, evidenced reconciliation.
use super::*;
pub(super) const CHECKPOINT: u8 = 0x5a;
pub(super) const CHECKPOINT_RECEIPT: u8 = 0x5b;
const CHECKPOINT_HISTORY: u8 = 0x5c;
const MAX_CHECKPOINT_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelationCheckpoint {
    pub schema_version: u32,
    pub consumer_id: String,
    pub revision: u64,
    pub cursor: RelationChangeCursor,
    pub author: RecordAuthor,
    pub updated_at_millis: i64,
    pub checkpoint_digest: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RelationCheckpointOperation {
    Advance {
        cursor: RelationChangeCursor,
    },
    Reconcile {
        cursor: RelationChangeCursor,
        evidence_ref: String,
    },
}
impl RelationCheckpointOperation {
    fn cursor(&self) -> &RelationChangeCursor {
        match self {
            Self::Advance { cursor } | Self::Reconcile { cursor, .. } => cursor,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelationCheckpointCommand {
    pub contract_version: u32,
    pub idempotency_key: String,
    pub consumer_id: String,
    pub expected_revision: u64,
    pub expected_checkpoint_digest: Option<String>,
    pub operation: RelationCheckpointOperation,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelationCheckpointReceipt {
    pub contract_version: u32,
    pub idempotency_key: String,
    pub previous: Option<RelationCheckpoint>,
    pub checkpoint: RelationCheckpoint,
    pub reconciliation_evidence: Option<String>,
    pub receipt_digest: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredCheckpointReceipt {
    schema_version: u32,
    namespace: String,
    request_digest: [u8; 32],
    receipt: RelationCheckpointReceipt,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelationConsumerDiagnostics {
    pub consumer_id: String,
    pub active: bool,
    pub baseline: Option<RelationChangeCursor>,
    pub high_watermark: Option<RelationChangeCursor>,
    pub checkpoint: Option<RelationCheckpoint>,
    pub checkpoint_status: ConsumerCheckpointStatus,
    pub pending_positions: Option<u64>,
    pub witness_status: ConsumerWitnessStatus,
    pub checkpoint_relative_to_witness: Option<ConsumerCursorOrder>,
}
pub(super) fn key(kind: u8, namespace: &str, subject: &str, id: &str) -> Result<Vec<u8>> {
    let mut key = vec![kind];
    key.extend(encode(&(namespace, subject, id))?);
    Ok(key)
}
pub(super) fn historical_key(
    namespace: &str,
    subject: &str,
    consumer: &str,
    revision: u64,
) -> Result<Vec<u8>> {
    let mut key = key(CHECKPOINT_HISTORY, namespace, subject, consumer)?;
    key.extend(revision.to_be_bytes());
    Ok(key)
}
fn validate_identity(namespace: &str, subject: &str, consumer: &str) -> Result<()> {
    RocksDbMemoryStorage::validate_agent(namespace)?;
    if !valid(subject, 256) || !valid(consumer, 128) {
        return Err(Error::ValidationError(
            "Invalid relation checkpoint subject or consumer".into(),
        ));
    }
    Ok(())
}
impl RelationCheckpoint {
    fn digest(&self, namespace: &str) -> Result<String> {
        let mut value = self.clone();
        value.checkpoint_digest.clear();
        hash(&("qilbee.relations.checkpoint.v1", namespace, value))
    }
    fn validate(&self, namespace: &str, subject: &str, consumer: &str) -> Result<()> {
        if self.schema_version != 1
            || self.revision == 0
            || self.cursor.validate().is_err()
            || self.author.subject_id != subject
            || self.consumer_id != consumer
            || !valid(subject, 256)
            || !valid(consumer, 128)
            || self.checkpoint_digest != self.digest(namespace)?
        {
            return Err(inconsistent());
        }
        Ok(())
    }
}
impl RelationCheckpointReceipt {
    fn digest(&self, namespace: &str) -> Result<String> {
        let mut value = self.clone();
        value.receipt_digest.clear();
        hash(&("qilbee.relations.checkpoint.receipt.v1", namespace, value))
    }
    fn validate(&self, namespace: &str, subject: &str, consumer: &str) -> Result<()> {
        self.checkpoint.validate(namespace, subject, consumer)?;
        if let Some(previous) = &self.previous {
            previous.validate(namespace, subject, consumer)?;
        }
        if self.contract_version != 1
            || !valid(&self.idempotency_key, 256)
            || self
                .previous
                .as_ref()
                .map_or(Some(1), |p| p.revision.checked_add(1))
                != Some(self.checkpoint.revision)
            || self
                .reconciliation_evidence
                .as_ref()
                .is_some_and(|s| !valid(s, 2048) || self.previous.is_none())
            || self.receipt_digest != self.digest(namespace)?
        {
            return Err(inconsistent());
        }
        if self.reconciliation_evidence.is_none() {
            if let Some(p) = &self.previous {
                if p.cursor.journal_id != self.checkpoint.cursor.journal_id
                    || p.cursor.sequence > self.checkpoint.cursor.sequence
                    || (p.cursor.sequence == self.checkpoint.cursor.sequence
                        && p.cursor != self.checkpoint.cursor)
                {
                    return Err(inconsistent());
                }
            }
        }
        Ok(())
    }
}
impl MemorySnapshot<'_> {
    fn relation_checkpoint_history(
        &self,
        namespace: &str,
        subject: &str,
        consumer: &str,
        revision: u64,
    ) -> Result<Option<RelationCheckpointReceipt>> {
        let Some(bytes) = self
            .db
            .get_cf(
                self.storage.cf(crate::storage::cf::AGENT_META)?,
                historical_key(namespace, subject, consumer, revision)?,
            )
            .map_err(storage_error)?
        else {
            return Ok(None);
        };
        if bytes.len() > MAX_CHECKPOINT_BYTES {
            return Err(inconsistent());
        }
        let receipt: RelationCheckpointReceipt = decode(&bytes)?;
        receipt.validate(namespace, subject, consumer)?;
        if receipt.checkpoint.revision != revision {
            return Err(inconsistent());
        }
        Ok(Some(receipt))
    }
    pub(in crate::storage::platform) fn stored_relation_checkpoint(
        &self,
        namespace: &str,
        subject: &str,
        consumer: &str,
    ) -> Result<Option<RelationCheckpoint>> {
        let Some(bytes) = self
            .db
            .get_cf(
                self.storage.cf(crate::storage::cf::AGENT_META)?,
                key(CHECKPOINT, namespace, subject, consumer)?,
            )
            .map_err(storage_error)?
        else {
            if self.relation_metadata_prefix_exists(key(
                CHECKPOINT_HISTORY,
                namespace,
                subject,
                consumer,
            )?)? {
                return Err(inconsistent());
            }
            return Ok(None);
        };
        if bytes.len() > MAX_CHECKPOINT_BYTES {
            return Err(inconsistent());
        }
        let checkpoint: RelationCheckpoint = decode(&bytes)?;
        checkpoint.validate(namespace, subject, consumer)?;
        let history = self
            .relation_checkpoint_history(namespace, subject, consumer, checkpoint.revision)?
            .ok_or_else(inconsistent)?;
        if history.checkpoint != checkpoint {
            return Err(inconsistent());
        }
        Ok(Some(checkpoint))
    }
}
impl RocksDbMemoryStorage {
    /// Save progress only after caller-managed effects are durable; this creates no relation event.
    pub fn commit_relation_checkpoint(
        &self,
        namespace: &str,
        author: &RecordAuthor,
        command: &RelationCheckpointCommand,
    ) -> Result<RelationCheckpointReceipt> {
        validate_identity(namespace, &author.subject_id, &command.consumer_id)?;
        if command.contract_version != 1
            || !valid(&command.idempotency_key, 256)
            || (command.expected_revision == 0 && command.expected_checkpoint_digest.is_some())
            || (command.expected_revision > 0
                && !command
                    .expected_checkpoint_digest
                    .as_deref()
                    .is_some_and(valid_digest))
        {
            return Err(Error::ValidationError(
                "Invalid relation checkpoint version, idempotency key or expectation".into(),
            ));
        }
        command.operation.cursor().validate()?;
        let evidence = match &command.operation {
            RelationCheckpointOperation::Advance { .. } => None,
            RelationCheckpointOperation::Reconcile { evidence_ref, .. } => {
                if command.expected_revision == 0 || !valid(evidence_ref, 2048) {
                    return Err(Error::ValidationError(
                        "Reconciliation requires an existing checkpoint and evidence reference"
                            .into(),
                    ));
                }
                Some(evidence_ref.clone())
            }
        };
        let _guard = self
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Memory mutation lock poisoned".into()))?;
        let view = self.memory_snapshot();
        let cf = self.cf(crate::storage::cf::AGENT_META)?;
        let receipt_key = key(
            CHECKPOINT_RECEIPT,
            namespace,
            &author.subject_id,
            &command.idempotency_key,
        )?;
        let request_digest = digest(&encode(command)?);
        if let Some(bytes) = view.db.get_cf(cf, &receipt_key).map_err(storage_error)? {
            if bytes.len() > MAX_CHECKPOINT_BYTES {
                return Err(inconsistent());
            }
            let stored: StoredCheckpointReceipt = decode(&bytes)?;
            if stored.schema_version != 1
                || stored.namespace != namespace
                || stored.receipt.idempotency_key != command.idempotency_key
            {
                return Err(inconsistent());
            }
            stored.receipt.validate(
                namespace,
                &author.subject_id,
                &stored.receipt.checkpoint.consumer_id,
            )?;
            if stored.request_digest != request_digest {
                return Err(Error::ConstraintViolation(
                    "Idempotency key was used for a different relation checkpoint command".into(),
                ));
            }
            if stored.receipt.checkpoint.consumer_id != command.consumer_id
                || view
                    .relation_checkpoint_history(
                        namespace,
                        &author.subject_id,
                        &command.consumer_id,
                        stored.receipt.checkpoint.revision,
                    )?
                    .as_ref()
                    != Some(&stored.receipt)
            {
                return Err(inconsistent());
            }
            return Ok(stored.receipt);
        }
        let previous =
            view.stored_relation_checkpoint(namespace, &author.subject_id, &command.consumer_id)?;
        if previous.as_ref().map_or(0, |p| p.revision) != command.expected_revision
            || previous.as_ref().map(|p| &p.checkpoint_digest)
                != command.expected_checkpoint_digest.as_ref()
        {
            return Err(Error::TransactionConflict(
                "Relation checkpoint revision or digest changed".into(),
            ));
        }
        let journal = view.relation_journal(namespace)?.ok_or_else(|| {
            Error::JournalHistoryConflict("No relation journal in this scope".into())
        })?;
        let cursor = command.operation.cursor();
        view.verify_relation_cursor(namespace, &journal, cursor)?;
        if evidence.is_none() {
            if let Some(p) = &previous {
                view.verify_relation_cursor(namespace, &journal, &p.cursor)?;
                if p.cursor.sequence > cursor.sequence {
                    return Err(Error::CheckpointRegression(
                        "Relation checkpoint cannot move backward without explicit reconciliation"
                            .into(),
                    ));
                }
            }
        }
        let mut checkpoint = RelationCheckpoint {
            schema_version: 1,
            consumer_id: command.consumer_id.clone(),
            revision: command.expected_revision.checked_add(1).ok_or_else(|| {
                Error::TransactionConflict("Relation checkpoint revision exhausted".into())
            })?,
            cursor: cursor.clone(),
            author: author.clone(),
            updated_at_millis: view.now,
            checkpoint_digest: String::new(),
        };
        checkpoint.checkpoint_digest = checkpoint.digest(namespace)?;
        let mut receipt = RelationCheckpointReceipt {
            contract_version: 1,
            idempotency_key: command.idempotency_key.clone(),
            previous,
            checkpoint: checkpoint.clone(),
            reconciliation_evidence: evidence,
            receipt_digest: String::new(),
        };
        receipt.receipt_digest = receipt.digest(namespace)?;
        let history_key = historical_key(
            namespace,
            &author.subject_id,
            &command.consumer_id,
            checkpoint.revision,
        )?;
        if view
            .db
            .get_cf(cf, &history_key)
            .map_err(storage_error)?
            .is_some()
        {
            return Err(inconsistent());
        }
        let stored = encode(&StoredCheckpointReceipt {
            schema_version: 1,
            namespace: namespace.into(),
            request_digest,
            receipt: receipt.clone(),
        })?;
        if stored.len() > MAX_CHECKPOINT_BYTES {
            return Err(Error::ValidationError(
                "Relation checkpoint receipt exceeds 16 KiB".into(),
            ));
        }
        let mut batch = rocksdb::WriteBatch::default();
        batch.put_cf(
            cf,
            key(
                CHECKPOINT,
                namespace,
                &author.subject_id,
                &command.consumer_id,
            )?,
            encode(&checkpoint)?,
        );
        batch.put_cf(cf, history_key, encode(&receipt)?);
        batch.put_cf(cf, receipt_key, stored);
        self.commit_relation_consumer_batch(batch)?;
        Ok(receipt)
    }
    pub fn read_relation_checkpoint(
        &self,
        namespace: &str,
        subject: &str,
        consumer: &str,
    ) -> Result<Option<RelationCheckpoint>> {
        validate_identity(namespace, subject, consumer)?;
        let view = self.memory_snapshot();
        let Some(checkpoint) = view.stored_relation_checkpoint(namespace, subject, consumer)?
        else {
            return Ok(None);
        };
        let journal = view.relation_journal(namespace)?.ok_or_else(|| {
            Error::JournalHistoryConflict(
                "Relation checkpoint belongs to an unavailable history".into(),
            )
        })?;
        view.verify_relation_cursor(namespace, &journal, &checkpoint.cursor)?;
        Ok(Some(checkpoint))
    }
    pub fn relation_checkpoint_revision(
        &self,
        namespace: &str,
        subject: &str,
        consumer: &str,
        revision: u64,
    ) -> Result<Option<RelationCheckpointReceipt>> {
        validate_identity(namespace, subject, consumer)?;
        if revision == 0 {
            return Err(Error::ValidationError(
                "Checkpoint revision must be positive".into(),
            ));
        }
        let view = self.memory_snapshot();
        let current = view.stored_relation_checkpoint(namespace, subject, consumer)?;
        let receipt = view.relation_checkpoint_history(namespace, subject, consumer, revision)?;
        if receipt.is_none() && current.as_ref().is_some_and(|p| revision <= p.revision) {
            return Err(inconsistent());
        }
        Ok(receipt)
    }
    /// Observe journal, server progress and caller witness in one snapshot; this is not an audit of every event.
    pub fn diagnose_relation_consumer(
        &self,
        namespace: &str,
        subject: &str,
        consumer: &str,
        witness: Option<&RelationChangeCursor>,
    ) -> Result<RelationConsumerDiagnostics> {
        validate_identity(namespace, subject, consumer)?;
        if let Some(cursor) = witness {
            cursor.validate()?;
        }
        let view = self.memory_snapshot();
        let checkpoint = view.stored_relation_checkpoint(namespace, subject, consumer)?;
        let journal = view.relation_journal(namespace)?;
        let compatible = |cursor: &RelationChangeCursor| -> Result<bool> {
            let Some(journal) = &journal else {
                return Ok(false);
            };
            match view.verify_relation_cursor(namespace, journal, cursor) {
                Ok(()) => Ok(true),
                Err(Error::JournalHistoryConflict(_)) => Ok(false),
                Err(e) => Err(e),
            }
        };
        let checkpoint_status = match &checkpoint {
            None => ConsumerCheckpointStatus::Missing,
            Some(p) if compatible(&p.cursor)? => ConsumerCheckpointStatus::Compatible,
            Some(_) => ConsumerCheckpointStatus::HistoryIncompatible,
        };
        let witness_status = match witness {
            None => ConsumerWitnessStatus::NotProvided,
            Some(c) if compatible(c)? => ConsumerWitnessStatus::Compatible,
            Some(_) => ConsumerWitnessStatus::HistoryIncompatible,
        };
        let mut pending_positions = None;
        let mut order = None;
        if checkpoint_status == ConsumerCheckpointStatus::Compatible {
            let cursor = &checkpoint.as_ref().ok_or_else(inconsistent)?.cursor;
            pending_positions =
                Some(journal.as_ref().ok_or_else(inconsistent)?.tip.sequence - cursor.sequence);
            if witness_status == ConsumerWitnessStatus::Compatible {
                order = Some(
                    match cursor
                        .sequence
                        .cmp(&witness.ok_or_else(inconsistent)?.sequence)
                    {
                        std::cmp::Ordering::Less => ConsumerCursorOrder::Before,
                        std::cmp::Ordering::Equal => ConsumerCursorOrder::Equal,
                        std::cmp::Ordering::Greater => ConsumerCursorOrder::After,
                    },
                );
            }
        }
        Ok(RelationConsumerDiagnostics {
            consumer_id: consumer.into(),
            active: journal.is_some(),
            baseline: journal.as_ref().map(|j| j.baseline.clone()),
            high_watermark: journal.map(|j| j.tip),
            checkpoint,
            checkpoint_status,
            pending_positions,
            witness_status,
            checkpoint_relative_to_witness: order,
        })
    }
}
