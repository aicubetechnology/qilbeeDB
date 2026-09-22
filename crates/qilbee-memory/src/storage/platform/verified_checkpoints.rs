//! Subject-owned durable change-feed progress; external effects remain caller-owned.
use super::snapshot::MemorySnapshot;
use super::*;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifiedMemoryCheckpoint {
    pub schema_version: u32,
    pub consumer_id: String,
    pub revision: u64,
    pub cursor: VerifiedMemoryCursor,
    pub author: RecordAuthor,
    pub updated_at_millis: i64,
    pub checkpoint_digest: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifiedMemoryCheckpointCommand {
    pub contract_version: u32,
    pub idempotency_key: String,
    pub consumer_id: String,
    pub expected_revision: u64,
    pub expected_checkpoint_digest: Option<String>,
    pub cursor: VerifiedMemoryCursor,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifiedMemoryCheckpointReceipt {
    pub contract_version: u32,
    pub idempotency_key: String,
    pub checkpoint: VerifiedMemoryCheckpoint,
    pub receipt_digest: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredCheckpointReceipt {
    schema_version: u32,
    request_digest: [u8; 32],
    receipt: VerifiedMemoryCheckpointReceipt,
}
fn checkpoint_hash<T: Serialize>(value: &T) -> Result<String> {
    Ok(digest(&encode(value)?)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}
impl VerifiedMemoryCheckpoint {
    fn digest(&self, namespace: &str) -> Result<String> {
        checkpoint_hash(&(
            namespace,
            self.schema_version,
            &self.consumer_id,
            self.revision,
            &self.cursor,
            &self.author,
            self.updated_at_millis,
        ))
    }
    fn validate(&self, namespace: &str, subject: &str, consumer: &str) -> Result<()> {
        if self.schema_version != 2
            || self.cursor.validate().is_err()
            || self.revision == 0
            || self.consumer_id != consumer
            || self.author.subject_id != subject
            || self.checkpoint_digest != self.digest(namespace)?
        {
            return Err(inconsistent());
        }
        Ok(())
    }
}
impl VerifiedMemoryCheckpointReceipt {
    fn digest(&self, namespace: &str) -> Result<String> {
        checkpoint_hash(&(
            namespace,
            self.contract_version,
            &self.idempotency_key,
            &self.checkpoint,
        ))
    }
}
fn valid_checkpoint_identity(s: &str, max: usize) -> bool {
    !s.trim().is_empty() && s.len() <= max && !s.chars().any(char::is_control)
}
fn checkpoint_key(kind: u8, namespace: &str, subject: &str, key: &str) -> Result<Vec<u8>> {
    let mut out = vec![kind];
    out.extend(encode(&(namespace, subject, key))?);
    Ok(out)
}
impl RocksDbMemoryStorage {
    /// The caller authorizes both feed reading and checkpoint writes in this namespace.
    pub fn commit_verified_memory_checkpoint(
        &self,
        namespace: &str,
        author: &RecordAuthor,
        command: &VerifiedMemoryCheckpointCommand,
    ) -> Result<VerifiedMemoryCheckpointReceipt> {
        Self::validate_agent(namespace)?;
        if command.expected_revision == 0 && command.expected_checkpoint_digest.is_some()
            || command.expected_revision > 0
                && !command
                    .expected_checkpoint_digest
                    .as_ref()
                    .is_some_and(|s| {
                        s.len() == 64
                            && s.bytes()
                                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                    })
            || command.contract_version != 2
            || !valid_checkpoint_identity(&command.consumer_id, 128)
            || !valid_checkpoint_identity(&command.idempotency_key, 256)
            || !valid_checkpoint_identity(&author.subject_id, 256)
        {
            return Err(Error::ValidationError(
                "Invalid checkpoint identity or contract version".into(),
            ));
        }
        let _guard = self
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Memory mutation lock poisoned".into()))?;
        let cf = self.cf(super::super::cf::AGENT_META)?;
        let receipt_key = checkpoint_key(
            0x29,
            namespace,
            &author.subject_id,
            &command.idempotency_key,
        )?;
        let request_digest = digest(&encode(command)?);
        if let Some(bytes) = self.db.get_cf(cf, &receipt_key).map_err(storage_error)? {
            let stored: StoredCheckpointReceipt = decode(&bytes)?;
            if stored.schema_version != 2
                || stored.receipt.contract_version != 2
                || stored.receipt.idempotency_key != command.idempotency_key
                || stored.receipt.receipt_digest != stored.receipt.digest(namespace)?
            {
                return Err(inconsistent());
            }
            stored.receipt.checkpoint.validate(
                namespace,
                &author.subject_id,
                &stored.receipt.checkpoint.consumer_id,
            )?;
            if stored.request_digest != request_digest {
                return Err(Error::ConstraintViolation(
                    "Idempotency key was already used for another checkpoint command".into(),
                ));
            }
            if stored.receipt.checkpoint.consumer_id != command.consumer_id {
                return Err(inconsistent());
            }
            return Ok(stored.receipt);
        }
        let current = self.read_verified_memory_checkpoint(
            namespace,
            &author.subject_id,
            &command.consumer_id,
        )?;
        if current.as_ref().map_or(0, |c| c.revision) != command.expected_revision
            || current.as_ref().map(|c| &c.checkpoint_digest)
                != command.expected_checkpoint_digest.as_ref()
        {
            return Err(Error::TransactionConflict(
                "Checkpoint revision changed".into(),
            ));
        }
        command.cursor.validate()?;
        let view = self.memory_snapshot();
        let state = view.verified_journal(namespace)?.ok_or_else(|| {
            Error::JournalHistoryConflict("The scope has no verified change journal".into())
        })?;
        view.verify_memory_cursor(namespace, &state, &command.cursor)?;
        if let Some(previous) = current {
            if previous.cursor.journal_id != command.cursor.journal_id
                || previous.cursor.generation != command.cursor.generation
                || previous.cursor.sequence > command.cursor.sequence
            {
                return Err(Error::CheckpointRegression(
                    "Checkpoint cursor cannot move backward or switch journals".into(),
                ));
            }
        }
        let mut checkpoint = VerifiedMemoryCheckpoint {
            schema_version: 2,
            consumer_id: command.consumer_id.clone(),
            revision: command.expected_revision.checked_add(1).ok_or_else(|| {
                Error::TransactionConflict("Checkpoint revision exhausted".into())
            })?,
            cursor: command.cursor.clone(),
            author: author.clone(),
            updated_at_millis: chrono::Utc::now().timestamp_millis(),
            checkpoint_digest: String::new(),
        };
        checkpoint.checkpoint_digest = checkpoint.digest(namespace)?;
        let mut receipt = VerifiedMemoryCheckpointReceipt {
            contract_version: 2,
            idempotency_key: command.idempotency_key.clone(),
            checkpoint: checkpoint.clone(),
            receipt_digest: String::new(),
        };
        receipt.receipt_digest = receipt.digest(namespace)?;
        let mut batch = rocksdb::WriteBatch::default();
        batch.put_cf(
            cf,
            checkpoint_key(0x28, namespace, &author.subject_id, &command.consumer_id)?,
            encode(&checkpoint)?,
        );
        batch.put_cf(
            cf,
            receipt_key,
            encode(&StoredCheckpointReceipt {
                schema_version: 2,
                request_digest,
                receipt: receipt.clone(),
            })?,
        );
        let mut options = rocksdb::WriteOptions::default();
        options.disable_wal(false);
        options.set_sync(true);
        self.db.write_opt(batch, &options).map_err(storage_error)?;
        Ok(receipt)
    }
    /// Returns current progress owned by this exact subject and consumer identifier.
    pub fn read_verified_memory_checkpoint(
        &self,
        namespace: &str,
        subject: &str,
        consumer: &str,
    ) -> Result<Option<VerifiedMemoryCheckpoint>> {
        Self::validate_agent(namespace)?;
        if !valid_checkpoint_identity(subject, 256) || !valid_checkpoint_identity(consumer, 128) {
            return Err(Error::ValidationError(
                "Invalid checkpoint subject or consumer identifier".into(),
            ));
        }
        let view = self.memory_snapshot();
        let Some(checkpoint) = view.stored_verified_checkpoint(namespace, subject, consumer)?
        else {
            return Ok(None);
        };
        let state = view.verified_journal(namespace)?.ok_or_else(inconsistent)?;
        view.verify_memory_cursor(namespace, &state, &checkpoint.cursor)
            .map_err(|_| inconsistent())?;
        Ok(Some(checkpoint))
    }
}

impl MemorySnapshot<'_> {
    /// Verify intrinsic ownership and integrity without requiring a compatible journal.
    /// Ordinary reads must additionally verify history; explicit recovery may reconcile it.
    pub(in crate::storage::platform) fn stored_verified_checkpoint(
        &self,
        namespace: &str,
        subject: &str,
        consumer: &str,
    ) -> Result<Option<VerifiedMemoryCheckpoint>> {
        let Some(bytes) = self
            .db
            .get_cf(
                self.storage.cf(crate::storage::cf::AGENT_META)?,
                checkpoint_key(0x28, namespace, subject, consumer)?,
            )
            .map_err(storage_error)?
        else {
            return Ok(None);
        };
        let checkpoint: VerifiedMemoryCheckpoint = decode(&bytes)?;
        checkpoint.validate(namespace, subject, consumer)?;
        Ok(Some(checkpoint))
    }
}

mod diagnostics;
pub use diagnostics::*;
mod recovery;
pub use recovery::*;
