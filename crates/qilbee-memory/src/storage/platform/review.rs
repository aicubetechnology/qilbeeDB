//! Scoped review decisions, committed with the revision and its change event.
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryReviewDisposition {
    Approved,
    Rejected,
    Unreviewed,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryReview {
    pub disposition: MemoryReviewDisposition,
    pub evidence_ref: String,
    pub author: RecordAuthor,
    pub reviewed_at_millis: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryReviewCommand {
    pub contract_version: u32,
    pub idempotency_key: String,
    pub record_id: Uuid,
    pub expected_revision: u64,
    pub disposition: MemoryReviewDisposition,
    pub evidence_ref: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryReviewReceipt {
    pub contract_version: u32,
    pub idempotency_key: String,
    pub record_id: Uuid,
    pub revision: u64,
    pub review: MemoryReview,
    pub receipt_digest: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryReviewState {
    pub record_id: Uuid,
    pub revision: u64,
    pub review: Option<MemoryReview>,
    pub deleted: bool,
    pub expired: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredReview {
    schema_version: u32,
    namespace: String,
    request_digest: [u8; 32],
    receipt: MemoryReviewReceipt,
}
impl MemoryReviewReceipt {
    fn digest(&self, namespace: &str) -> Result<String> {
        Ok(digest(&encode(&(
            namespace,
            self.contract_version,
            &self.idempotency_key,
            self.record_id,
            self.revision,
            &self.review,
        ))?)
        .iter()
        .map(|v| format!("{v:02x}"))
        .collect())
    }
}
fn history_key(namespace: &str, id: Uuid, revision: u64) -> Vec<u8> {
    let mut key = record_key(0x23, namespace, id);
    key.extend_from_slice(&revision.to_be_bytes());
    key
}
fn read_stored(bytes: &[u8], namespace: &str) -> Result<StoredReview> {
    let stored: StoredReview = decode(bytes)?;
    if stored.schema_version != 1
        || stored.namespace != namespace
        || stored.receipt.contract_version != 1
        || stored.receipt.revision < 2
        || stored.receipt.receipt_digest != stored.receipt.digest(namespace)?
    {
        return Err(inconsistent());
    }
    Ok(stored)
}
fn valid_identity(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}
impl RocksDbMemoryStorage {
    /// The caller must authorize review, independently of ordinary memory writes.
    /// Approval records a decision; it does not certify external facts.
    pub fn review_memory_record(
        &self,
        namespace: &str,
        author: &RecordAuthor,
        command: &MemoryReviewCommand,
    ) -> Result<MemoryReviewReceipt> {
        Self::validate_agent(namespace)?;
        if command.contract_version != 1
            || command.expected_revision == 0
            || !valid_identity(&command.idempotency_key, 256)
            || !valid_identity(&command.evidence_ref, 2048)
            || !valid_identity(&author.subject_id, 256)
        {
            return Err(Error::ValidationError(
                "Invalid review identity, revision or evidence reference".into(),
            ));
        }
        let _guard = self
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Memory mutation lock poisoned".into()))?;
        let cf = self.cf(super::super::cf::AGENT_META)?;
        let mut key = vec![0x22];
        key.extend(encode(&(
            namespace,
            &author.subject_id,
            &command.idempotency_key,
        ))?);
        let request_digest = digest(&encode(command)?);
        if let Some(bytes) = self.db.get_cf(cf, &key).map_err(storage_error)? {
            let stored = read_stored(&bytes, namespace)?;
            if stored.receipt.idempotency_key != command.idempotency_key
                || stored.receipt.review.author.subject_id != author.subject_id
            {
                return Err(inconsistent());
            }
            if stored.request_digest != request_digest {
                return Err(Error::ConstraintViolation(
                    "Idempotency key was already used for a different review".into(),
                ));
            }
            return Ok(stored.receipt);
        }
        let mut record = self.platform_record_for_write(
            namespace,
            command.record_id,
            command.expected_revision,
        )?;
        let now = chrono::Utc::now().timestamp_millis();
        let review = MemoryReview {
            disposition: command.disposition,
            evidence_ref: command.evidence_ref.clone(),
            author: author.clone(),
            reviewed_at_millis: now,
        };
        record.review = Some(review.clone());
        record.modified_at_millis = now;
        let mut receipt = MemoryReviewReceipt {
            contract_version: 1,
            idempotency_key: command.idempotency_key.clone(),
            record_id: record.record_id,
            revision: record.revision,
            review,
            receipt_digest: String::new(),
        };
        receipt.receipt_digest = receipt.digest(namespace)?;
        let stored = encode(&StoredReview {
            schema_version: 1,
            namespace: namespace.into(),
            request_digest,
            receipt: receipt.clone(),
        })?;
        let history = history_key(namespace, record.record_id, record.revision);
        if self
            .db
            .get_cf(cf, &history)
            .map_err(storage_error)?
            .is_some()
        {
            return Err(inconsistent());
        }
        let encoded = encode(&record)?;
        let index = RecordIndex {
            schema_version: 1,
            revision: record.revision,
            record_digest: digest(&encoded),
        };
        let mut batch = rocksdb::WriteBatch::default();
        batch.put_cf(
            self.cf(super::super::cf::EPISODES)?,
            record_key(0x10, namespace, record.record_id),
            encoded,
        );
        batch.put_cf(
            self.cf(super::super::cf::EPISODE_INDEX)?,
            record_key(0x11, namespace, record.record_id),
            encode(&index)?,
        );
        batch.put_cf(cf, key, &stored);
        batch.put_cf(cf, history, stored);
        self.append_memory_change(
            namespace,
            &mut batch,
            MemoryChangeKind::Reviewed,
            record.record_id,
            record.revision,
            author,
            now,
        )?;
        let mut options = rocksdb::WriteOptions::default();
        options.disable_wal(false);
        options.set_sync(true);
        self.db.write_opt(batch, &options).map_err(storage_error)?;
        Ok(receipt)
    }
    /// Metadata-only state, including records excluded from normal serving.
    pub fn memory_review_state(
        &self,
        namespace: &str,
        id: Uuid,
    ) -> Result<Option<MemoryReviewState>> {
        Self::validate_agent(namespace)?;
        let snapshot = self.memory_snapshot();
        Ok(snapshot.record(namespace, id)?.map(|r| MemoryReviewState {
            record_id: id,
            revision: r.revision,
            review: r.review,
            deleted: r.payload.is_none(),
            expired: r
                .payload
                .as_ref()
                .is_some_and(|p| p.valid_until_millis.is_some_and(|t| t <= snapshot.now)),
        }))
    }
    /// Read an immutable historical decision, even after content update or deletion.
    pub fn read_memory_review(
        &self,
        namespace: &str,
        id: Uuid,
        revision: u64,
    ) -> Result<Option<MemoryReviewReceipt>> {
        Self::validate_agent(namespace)?;
        if revision == 0 {
            return Err(Error::ValidationError(
                "Review revision must be positive".into(),
            ));
        }
        let Some(bytes) = self
            .db
            .get_cf(
                self.cf(super::super::cf::AGENT_META)?,
                history_key(namespace, id, revision),
            )
            .map_err(storage_error)?
        else {
            return Ok(None);
        };
        let stored = read_stored(&bytes, namespace)?;
        if stored.receipt.record_id != id || stored.receipt.revision != revision {
            return Err(inconsistent());
        }
        Ok(Some(stored.receipt))
    }
}
