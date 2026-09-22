use super::*;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct JobIntegrity {
    schema_version: u32,
    namespace: String,
    owner: String,
    revision: u64,
    record_digest: [u8; 32],
    history_digest: [u8; 32],
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StoredReceipt {
    pub schema_version: u32,
    pub namespace: String,
    pub owner: String,
    pub receipt: ConsolidationReceipt,
}
impl ConsolidationReceipt {
    pub(super) fn calculate_digest(&self, namespace: &str, owner: &str) -> Result<String> {
        let mut value = self.clone();
        value.receipt_digest.clear();
        hash(&("qilbee.consolidation.receipt.v1", namespace, owner, value))
    }
    fn validate(&self, namespace: &str, owner: &str) -> Result<()> {
        let is_hash = |s: &str| {
            s.len() == 64
                && s.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        };
        if self.contract_version != 1
            || self.revision == 0
            || !valid(&self.idempotency_key, 256)
            || !valid(&self.author.subject_id, 256)
            || (self.author.subject_id != owner && self.action != ConsolidationAction::Cancelled)
            || !is_hash(&self.job_digest)
            || !is_hash(&self.command_digest)
            || self.receipt_digest != self.calculate_digest(namespace, owner)?
        {
            return Err(inconsistent());
        }
        Ok(())
    }
}
impl ConsolidationJob {
    pub(super) fn validate(&self, owner: &str) -> Result<()> {
        self.spec.validate().map_err(|_| inconsistent())?;
        if self.schema_version != 1
            || self.revision == 0
            || self.created_by.subject_id != owner
            || self.modified_at_millis < self.created_at_millis
            || self.attempts.len() > self.spec.max_attempts as usize
            || self.output_receipts.len() > self.spec.max_relations
        {
            return Err(inconsistent());
        }
        let mut published = 0;
        let mut fences = BTreeSet::new();
        for (i, a) in self.attempts.iter().enumerate() {
            let hard = a
                .claimed_at_millis
                .checked_add(self.spec.max_attempt_millis as i64)
                .ok_or_else(inconsistent)?;
            if a.number as usize != i + 1
                || !valid(&a.worker_id, 256)
                || !fences.insert(a.fence)
                || a.claimed_at_millis < self.created_at_millis
                || a.expires_at_millis <= a.claimed_at_millis
                || a.expires_at_millis > hard
                || a.ended_at_millis.is_some_and(|t| t < a.claimed_at_millis)
                || a.evidence_ref.as_ref().is_some_and(|s| !valid(s, 2048))
                || a.usage_evidence_ref
                    .as_ref()
                    .is_some_and(|s| !valid(s, 2048))
            {
                return Err(inconsistent());
            }
            if a.outcome == ConsolidationOutcome::Running {
                if i + 1 != self.attempts.len()
                    || self.status != ConsolidationStatus::Running
                    || a.ended_at_millis.is_some()
                    || a.evidence_ref.is_some()
                    || a.usage != ConsolidationUsage::Unknown
                {
                    return Err(inconsistent());
                }
            } else if a.ended_at_millis.is_none() || a.evidence_ref.is_none() {
                return Err(inconsistent());
            }
            if a.outcome == ConsolidationOutcome::Published {
                published += 1;
            }
        }
        if (self.status == ConsolidationStatus::Running)
            != self
                .attempts
                .last()
                .is_some_and(|a| a.outcome == ConsolidationOutcome::Running)
            || (self.status == ConsolidationStatus::Published && published != 1)
            || (self.status != ConsolidationStatus::Published
                && (published != 0 || !self.output_receipts.is_empty()))
            || (self.status == ConsolidationStatus::Ready
                && self.attempts.len() >= self.spec.max_attempts as usize)
            || (self.status == ConsolidationStatus::Exhausted
                && self.attempts.len() != self.spec.max_attempts as usize)
        {
            return Err(inconsistent());
        }
        Ok(())
    }
}
impl MemorySnapshot<'_> {
    pub(super) fn consolidation_revision(
        &self,
        namespace: &str,
        owner: &str,
        id: Uuid,
        revision: u64,
    ) -> Result<Option<ConsolidationRevision>> {
        let Some(bytes) = self
            .db
            .get_cf(
                self.storage.cf(crate::storage::cf::AGENT_META)?,
                history_key(namespace, owner, id, revision)?,
            )
            .map_err(storage_error)?
        else {
            return Ok(None);
        };
        if bytes.len() > MAX_HISTORY_BYTES {
            return Err(inconsistent());
        }
        let value: ConsolidationRevision = decode(&bytes)?;
        value.job.validate(owner)?;
        value.receipt.validate(namespace, owner)?;
        if value.job.job_id != id
            || value.job.revision != revision
            || value.receipt.job_id != id
            || value.receipt.revision != revision
            || value.receipt.job_digest != hash(&value.job)?
        {
            return Err(inconsistent());
        }
        for receipt in &value.job.output_receipts {
            let original = self
                .relation_revision(namespace, receipt.relation_id, receipt.revision)?
                .ok_or_else(inconsistent)?;
            if original.receipt != *receipt || receipt.author.subject_id != owner {
                return Err(inconsistent());
            }
        }
        Ok(Some(value))
    }
    pub(super) fn consolidation_job(
        &self,
        namespace: &str,
        owner: &str,
        id: Uuid,
    ) -> Result<Option<ConsolidationJob>> {
        let cf = self.storage.cf(crate::storage::cf::AGENT_META)?;
        let record = self
            .db
            .get_cf(cf, job_key(JOB, namespace, owner, id)?)
            .map_err(storage_error)?;
        let integrity = self
            .db
            .get_cf(cf, job_key(INTEGRITY, namespace, owner, id)?)
            .map_err(storage_error)?;
        let (bytes, index) = match (record, integrity) {
            (None, None) => return Ok(None),
            (Some(r), Some(i)) => (r, i),
            _ => return Err(inconsistent()),
        };
        if bytes.len() > MAX_JOB_BYTES || index.len() > 70_000 {
            return Err(inconsistent());
        }
        let job: ConsolidationJob = decode(&bytes)?;
        let index: JobIntegrity = decode(&index)?;
        job.validate(owner)?;
        if job.job_id != id
            || index.schema_version != 1
            || index.namespace != namespace
            || index.owner != owner
            || index.revision != job.revision
            || index.record_digest != digest(&bytes)
        {
            return Err(inconsistent());
        }
        let history = self
            .consolidation_revision(namespace, owner, id, job.revision)?
            .ok_or_else(inconsistent)?;
        if history.job != job || index.history_digest != digest(&encode(&history)?) {
            return Err(inconsistent());
        }
        Ok(Some(job))
    }
    pub(super) fn consolidation_replay(
        &self,
        namespace: &str,
        owner: &str,
        command: &ConsolidationCommand,
    ) -> Result<Option<ConsolidationReceipt>> {
        let mut key = owner_prefix(RECEIPT, namespace, owner)?;
        key.extend(encode(&command.idempotency_key)?);
        let Some(bytes) = self
            .db
            .get_cf(self.storage.cf(crate::storage::cf::AGENT_META)?, key)
            .map_err(storage_error)?
        else {
            return Ok(None);
        };
        if bytes.len() > 70_000 {
            return Err(inconsistent());
        }
        let stored: StoredReceipt = decode(&bytes)?;
        let receipt = stored.receipt;
        receipt.validate(namespace, owner)?;
        if stored.schema_version != 1
            || stored.namespace != namespace
            || stored.owner != owner
            || receipt.idempotency_key != command.idempotency_key
        {
            return Err(inconsistent());
        }
        if receipt.command_digest != hash(command)? {
            return Err(Error::ConstraintViolation(
                "Idempotency key was used for a different consolidation command".into(),
            ));
        }
        let historical = self
            .consolidation_revision(namespace, owner, receipt.job_id, receipt.revision)?
            .ok_or_else(inconsistent)?;
        if historical.receipt != receipt {
            return Err(inconsistent());
        }
        self.consolidation_job(namespace, owner, receipt.job_id)?
            .ok_or_else(inconsistent)?;
        Ok(Some(receipt))
    }
}
impl RocksDbMemoryStorage {
    pub(super) fn append_consolidation_revision(
        &self,
        snapshot: &MemorySnapshot<'_>,
        namespace: &str,
        author: &RecordAuthor,
        command: &ConsolidationCommand,
        job: &ConsolidationJob,
        action: ConsolidationAction,
        batch: &mut rocksdb::WriteBatch,
    ) -> Result<ConsolidationReceipt> {
        let owner = &job.created_by.subject_id;
        job.validate(owner)?;
        let mut receipt = ConsolidationReceipt {
            contract_version: 1,
            idempotency_key: command.idempotency_key.clone(),
            job_id: job.job_id,
            revision: job.revision,
            action,
            author: author.clone(),
            committed_at_millis: snapshot.now,
            job_digest: hash(job)?,
            command_digest: hash(command)?,
            receipt_digest: String::new(),
        };
        receipt.receipt_digest = receipt.calculate_digest(namespace, owner)?;
        let record = encode(job)?;
        let history = encode(&ConsolidationRevision {
            job: job.clone(),
            receipt: receipt.clone(),
        })?;
        if record.len() > MAX_JOB_BYTES || history.len() > MAX_HISTORY_BYTES {
            return Err(Error::ValidationError(
                "Consolidation record/history exceeds its 512/1024 KiB bound".into(),
            ));
        }
        let cf = self.cf(crate::storage::cf::AGENT_META)?;
        let history_key = history_key(namespace, owner, job.job_id, job.revision)?;
        if snapshot
            .db
            .get_cf(cf, &history_key)
            .map_err(storage_error)?
            .is_some()
        {
            return Err(inconsistent());
        }
        let index = JobIntegrity {
            schema_version: 1,
            namespace: namespace.into(),
            owner: owner.clone(),
            revision: job.revision,
            record_digest: digest(&record),
            history_digest: digest(&history),
        };
        batch.put_cf(cf, job_key(JOB, namespace, owner, job.job_id)?, record);
        batch.put_cf(
            cf,
            job_key(INTEGRITY, namespace, owner, job.job_id)?,
            encode(&index)?,
        );
        batch.put_cf(cf, history_key, history);
        let mut key = owner_prefix(RECEIPT, namespace, owner)?;
        key.extend(encode(&command.idempotency_key)?);
        batch.put_cf(
            cf,
            key,
            encode(&StoredReceipt {
                schema_version: 1,
                namespace: namespace.into(),
                owner: owner.clone(),
                receipt: receipt.clone(),
            })?,
        );
        Ok(receipt)
    }
    pub fn inspect_consolidation_job(
        &self,
        namespace: &str,
        owner: &str,
        id: Uuid,
    ) -> Result<Option<ConsolidationInspection>> {
        check_identity(namespace, owner)?;
        let snapshot = self.memory_snapshot();
        let Some(job) = snapshot.consolidation_job(namespace, owner, id)? else {
            return Ok(None);
        };
        let source_failure = snapshot.relation_evidence_failure(namespace, &job.spec.sources)?;
        let active = lease_active(&job, self.consolidation_incarnation, snapshot.now);
        Ok(Some(ConsolidationInspection {
            evaluated_at_millis: snapshot.now,
            lease_active: active,
            recoverable: job.status == ConsolidationStatus::Running && !active,
            job,
            source_failure,
            dependency_work: snapshot.dependency_work(),
        }))
    }
    pub fn consolidation_job_revision(
        &self,
        namespace: &str,
        owner: &str,
        id: Uuid,
        revision: u64,
    ) -> Result<Option<ConsolidationRevision>> {
        check_identity(namespace, owner)?;
        if revision == 0 {
            return Err(Error::ValidationError(
                "Consolidation revision must be positive".into(),
            ));
        }
        self.memory_snapshot()
            .consolidation_revision(namespace, owner, id, revision)
    }
}
