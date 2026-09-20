//! Explicit consumer reconciliation receipts; this never undoes external effects.
use super::*;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifiedCheckpointRecoveryCommand {
    pub contract_version: u32,
    pub idempotency_key: String,
    pub consumer_id: String,
    pub expected_revision: u64,
    pub expected_checkpoint_digest: String,
    pub cursor: VerifiedMemoryCursor,
    pub evidence_ref: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifiedCheckpointRecoveryReceipt {
    pub contract_version: u32,
    pub idempotency_key: String,
    pub previous: VerifiedMemoryCheckpoint,
    pub checkpoint: VerifiedMemoryCheckpoint,
    pub evidence_ref: String,
    pub receipt_digest: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredRecoveryReceipt {
    schema_version: u32,
    request_digest: [u8; 32],
    receipt: VerifiedCheckpointRecoveryReceipt,
}
fn history_key(namespace: &str, subject: &str, consumer: &str, revision: u64) -> Result<Vec<u8>> {
    let mut key = checkpoint_key(0x2b, namespace, subject, consumer)?;
    key.extend_from_slice(&revision.to_be_bytes());
    Ok(key)
}
impl VerifiedCheckpointRecoveryReceipt {
    fn digest(&self, namespace: &str) -> Result<String> {
        checkpoint_hash(&(
            "qilbee.checkpoint.recovery.v2",
            namespace,
            self.contract_version,
            &self.idempotency_key,
            &self.previous,
            &self.checkpoint,
            &self.evidence_ref,
        ))
    }
    fn validate(&self, namespace: &str, subject: &str, consumer: &str) -> Result<()> {
        self.previous.validate(namespace, subject, consumer)?;
        self.checkpoint.validate(namespace, subject, consumer)?;
        if self.contract_version != 2
            || !valid_checkpoint_identity(&self.idempotency_key, 256)
            || !valid_checkpoint_identity(&self.evidence_ref, 2048)
            || self.previous.revision.checked_add(1) != Some(self.checkpoint.revision)
            || self.receipt_digest != self.digest(namespace)?
        {
            return Err(inconsistent());
        }
        Ok(())
    }
}
impl RocksDbMemoryStorage {
    /// Explicitly replace existing subject-owned progress after caller-managed reconciliation.
    pub fn recover_verified_memory_checkpoint(
        &self,
        namespace: &str,
        author: &RecordAuthor,
        command: &VerifiedCheckpointRecoveryCommand,
    ) -> Result<VerifiedCheckpointRecoveryReceipt> {
        Self::validate_agent(namespace)?;
        if command.contract_version != 2
            || command.expected_revision == 0
            || !valid_checkpoint_identity(&command.consumer_id, 128)
            || !valid_checkpoint_identity(&command.idempotency_key, 256)
            || !valid_checkpoint_identity(&author.subject_id, 256)
            || !valid_checkpoint_identity(&command.evidence_ref, 2048)
            || command.expected_checkpoint_digest.len() != 64
            || !command
                .expected_checkpoint_digest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(Error::ValidationError(
                "Invalid checkpoint recovery identity, evidence or expectation".into(),
            ));
        }
        command.cursor.validate()?;
        let _guard = self
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Memory mutation lock poisoned".into()))?;
        let cf = self.cf(super::super::super::cf::AGENT_META)?;
        let receipt_key = checkpoint_key(
            0x2a,
            namespace,
            &author.subject_id,
            &command.idempotency_key,
        )?;
        let request_digest = digest(&encode(command)?);
        if let Some(bytes) = self.db.get_cf(cf, &receipt_key).map_err(storage_error)? {
            let stored: StoredRecoveryReceipt = decode(&bytes)?;
            if stored.schema_version != 2
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
                    "Idempotency key was already used for another recovery command".into(),
                ));
            }
            return Ok(stored.receipt);
        }
        let previous = self
            .read_verified_memory_checkpoint(namespace, &author.subject_id, &command.consumer_id)?
            .ok_or_else(|| {
                Error::TransactionConflict("No existing verified checkpoint to recover".into())
            })?;
        if previous.revision != command.expected_revision
            || previous.checkpoint_digest != command.expected_checkpoint_digest
        {
            return Err(Error::TransactionConflict(
                "Checkpoint revision or digest changed".into(),
            ));
        }
        let view = self.memory_snapshot();
        let state = view.verified_journal(namespace)?.ok_or_else(inconsistent)?;
        view.verify_memory_cursor(namespace, &state, &command.cursor)?;
        let mut checkpoint = VerifiedMemoryCheckpoint {
            schema_version: 2,
            consumer_id: command.consumer_id.clone(),
            revision: previous.revision.checked_add(1).ok_or_else(|| {
                Error::TransactionConflict("Checkpoint revision exhausted".into())
            })?,
            cursor: command.cursor.clone(),
            author: author.clone(),
            updated_at_millis: chrono::Utc::now().timestamp_millis(),
            checkpoint_digest: String::new(),
        };
        checkpoint.checkpoint_digest = checkpoint.digest(namespace)?;
        let mut receipt = VerifiedCheckpointRecoveryReceipt {
            contract_version: 2,
            idempotency_key: command.idempotency_key.clone(),
            previous,
            checkpoint: checkpoint.clone(),
            evidence_ref: command.evidence_ref.clone(),
            receipt_digest: String::new(),
        };
        receipt.receipt_digest = receipt.digest(namespace)?;
        let historical_key = history_key(
            namespace,
            &author.subject_id,
            &command.consumer_id,
            checkpoint.revision,
        )?;
        if self
            .db
            .get_cf(cf, &historical_key)
            .map_err(storage_error)?
            .is_some()
        {
            return Err(inconsistent());
        }
        let mut batch = rocksdb::WriteBatch::default();
        batch.put_cf(
            cf,
            checkpoint_key(0x28, namespace, &author.subject_id, &command.consumer_id)?,
            encode(&checkpoint)?,
        );
        batch.put_cf(
            cf,
            receipt_key,
            encode(&StoredRecoveryReceipt {
                schema_version: 2,
                request_digest,
                receipt: receipt.clone(),
            })?,
        );
        batch.put_cf(cf, historical_key, encode(&receipt)?);
        let mut options = rocksdb::WriteOptions::default();
        options.disable_wal(false);
        options.set_sync(true);
        self.db.write_opt(batch, &options).map_err(storage_error)?;
        Ok(receipt)
    }
    /// Read immutable recovery evidence by resulting checkpoint revision, not current progress.
    pub fn read_verified_checkpoint_recovery(
        &self,
        namespace: &str,
        subject: &str,
        consumer: &str,
        revision: u64,
    ) -> Result<Option<VerifiedCheckpointRecoveryReceipt>> {
        Self::validate_agent(namespace)?;
        if !valid_checkpoint_identity(subject, 256)
            || !valid_checkpoint_identity(consumer, 128)
            || revision == 0
        {
            return Err(Error::ValidationError(
                "Invalid recovery history identity or revision".into(),
            ));
        }
        let Some(bytes) = self
            .db
            .get_cf(
                self.cf(super::super::super::cf::AGENT_META)?,
                history_key(namespace, subject, consumer, revision)?,
            )
            .map_err(storage_error)?
        else {
            return Ok(None);
        };
        let receipt: VerifiedCheckpointRecoveryReceipt = decode(&bytes)?;
        receipt.validate(namespace, subject, consumer)?;
        if receipt.checkpoint.revision != revision {
            return Err(inconsistent());
        }
        Ok(Some(receipt))
    }
}
