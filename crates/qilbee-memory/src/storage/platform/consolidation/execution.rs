use super::*;

fn evidence(value: &str) -> Result<()> {
    if !valid(value, 2048) {
        return Err(Error::ValidationError(
            "An operation requires a nonblank evidence reference of at most 2048 bytes".into(),
        ));
    }
    Ok(())
}
pub(super) fn check_sources(
    snapshot: &MemorySnapshot<'_>,
    namespace: &str,
    spec: &ConsolidationSpec,
) -> Result<()> {
    if let Some(failure) = snapshot.relation_evidence_failure(namespace, &spec.sources)? {
        return Err(match failure.reason {
            MemoryEligibilityReason::DepthLimit
            | MemoryEligibilityReason::NodeLimit
            | MemoryEligibilityReason::DependencyCycle => Error::ValidationError(
                "Consolidation source context exceeds dependency graph bounds or contains a cycle"
                    .into(),
            ),
            _ => conflict("Consolidation context is not an eligible current revision"),
        });
    }
    let mut bytes = 0usize;
    for source in &spec.sources {
        bytes = bytes.saturating_add(
            snapshot
                .dependency(namespace, source.record_id)?
                .ok_or_else(inconsistent)?
                .record_bytes,
        );
        if bytes > 8 * 1024 * 1024 {
            return Err(Error::ValidationError(
                "Consolidation context exceeds 8 MiB of canonical records".into(),
            ));
        }
    }
    Ok(())
}
pub(super) fn require_lease(
    job: &ConsolidationJob,
    author: &RecordAuthor,
    fence: Uuid,
    incarnation: Uuid,
    now: i64,
) -> Result<()> {
    if !lease_active(job, incarnation, now)
        || !job
            .attempts
            .last()
            .is_some_and(|a| a.fence == fence && a.credential_id == author.credential_id)
    {
        return Err(conflict(
            "Consolidation lease is not active for this credential and fence",
        ));
    }
    Ok(())
}
fn close_attempt(
    job: &mut ConsolidationJob,
    now: i64,
    outcome: ConsolidationOutcome,
    usage: ConsolidationUsage,
    reference: &str,
) {
    let attempt = job.attempts.last_mut().expect("Running job has an attempt");
    attempt.ended_at_millis = Some(now);
    attempt.outcome = outcome;
    attempt.usage = usage;
    attempt.evidence_ref = Some(reference.into());
    attempt.usage_evidence_ref = if attempt.usage == ConsolidationUsage::Unknown {
        None
    } else {
        Some(reference.into())
    };
}
fn retry_state(job: &ConsolidationJob) -> ConsolidationStatus {
    if job.attempts.len() < job.spec.max_attempts as usize {
        ConsolidationStatus::Ready
    } else {
        ConsolidationStatus::Exhausted
    }
}
impl RocksDbMemoryStorage {
    /// Caller authorizes an exact namespace and the actor. All lifecycle writes, source
    /// checks and publication share the same mutation lock and synchronous WAL batch.
    pub fn apply_consolidation_command(
        &self,
        namespace: &str,
        author: &RecordAuthor,
        command: &ConsolidationCommand,
    ) -> Result<ConsolidationReceipt> {
        self.apply_owned_consolidation_command(namespace, &author.subject_id, author, command)
    }
    /// Caller must authorize company administration of the namespace before selecting
    /// another owner. This surface only cancels; it cannot claim or publish as an agent.
    pub fn cancel_admin_consolidation_job(
        &self,
        namespace: &str,
        owner: &str,
        author: &RecordAuthor,
        command: &ConsolidationCommand,
    ) -> Result<ConsolidationReceipt> {
        if !matches!(command.operation, ConsolidationOperation::Cancel { .. }) {
            return Err(Error::ValidationError(
                "Company administration only accepts cancellation".into(),
            ));
        }
        self.apply_owned_consolidation_command(namespace, owner, author, command)
    }
    fn apply_owned_consolidation_command(
        &self,
        namespace: &str,
        owner: &str,
        author: &RecordAuthor,
        command: &ConsolidationCommand,
    ) -> Result<ConsolidationReceipt> {
        check_identity(namespace, owner)?;
        check_identity(namespace, &author.subject_id)?;
        if command.contract_version != 1 || !valid(&command.idempotency_key, 256) {
            return Err(Error::ValidationError(
                "Invalid consolidation contract version or command identity".into(),
            ));
        }
        let _guard = self
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Memory mutation lock poisoned".into()))?;
        let snapshot = self.memory_snapshot();
        if let Some(receipt) = snapshot.consolidation_replay(namespace, owner, command)? {
            return Ok(receipt);
        }
        let now = snapshot.now;
        let mut job = if let Some((id, revision)) = command.operation.target() {
            let mut job = snapshot
                .consolidation_job(namespace, owner, id)?
                .ok_or_else(|| {
                    Error::KeyNotFound(
                        "Consolidation job is not available in the authorized scope and subject"
                            .into(),
                    )
                })?;
            if revision == 0 || revision != job.revision {
                return Err(conflict("Consolidation job revision changed"));
            }
            if now < job.modified_at_millis {
                return Err(conflict(
                    "Server clock precedes the latest consolidation revision",
                ));
            }
            job.revision = job
                .revision
                .checked_add(1)
                .ok_or_else(|| Error::ValidationError("Consolidation revision exhausted".into()))?;
            job.modified_at_millis = now;
            job
        } else {
            let ConsolidationOperation::Create { spec } = &command.operation else {
                unreachable!()
            };
            spec.validate()?;
            check_sources(&snapshot, namespace, spec)?;
            let id = Uuid::new_v4();
            if snapshot.consolidation_job(namespace, owner, id)?.is_some() {
                return Err(conflict("Consolidation identifier collision"));
            }
            ConsolidationJob {
                schema_version: 1,
                job_id: id,
                revision: 1,
                created_by: author.clone(),
                created_at_millis: now,
                modified_at_millis: now,
                spec: spec.clone(),
                status: ConsolidationStatus::Ready,
                attempts: Vec::new(),
                output_receipts: Vec::new(),
            }
        };
        let mut batch = rocksdb::WriteBatch::default();
        let action = match &command.operation {
            ConsolidationOperation::Create { .. } => ConsolidationAction::Created,
            ConsolidationOperation::Claim { worker_id, .. } => {
                if !valid(worker_id, 256) {
                    return Err(Error::ValidationError(
                        "Invalid consolidation worker identity".into(),
                    ));
                }
                if job.status != ConsolidationStatus::Ready
                    || job.attempts.len() >= job.spec.max_attempts as usize
                {
                    return Err(conflict("Consolidation job is not ready to claim"));
                }
                check_sources(&snapshot, namespace, &job.spec)?;
                let expires = now
                    .checked_add(job.spec.lease_millis as i64)
                    .ok_or_else(|| Error::ValidationError("Lease expiry exhausted".into()))?;
                job.attempts.push(ConsolidationAttempt {
                    number: job.attempts.len() as u32 + 1,
                    worker_id: worker_id.clone(),
                    credential_id: author.credential_id,
                    fence: Uuid::new_v4(),
                    storage_incarnation: self.consolidation_incarnation,
                    claimed_at_millis: now,
                    expires_at_millis: expires,
                    ended_at_millis: None,
                    outcome: ConsolidationOutcome::Running,
                    usage: ConsolidationUsage::Unknown,
                    evidence_ref: None,
                    usage_evidence_ref: None,
                });
                job.status = ConsolidationStatus::Running;
                ConsolidationAction::Claimed
            }
            ConsolidationOperation::Renew { fence, .. } => {
                require_lease(&job, author, *fence, self.consolidation_incarnation, now)?;
                let attempt = job.attempts.last_mut().unwrap();
                let hard = attempt
                    .claimed_at_millis
                    .checked_add(job.spec.max_attempt_millis as i64)
                    .ok_or_else(inconsistent)?;
                let expires = now
                    .checked_add(job.spec.lease_millis as i64)
                    .ok_or_else(inconsistent)?
                    .min(hard);
                if expires <= attempt.expires_at_millis {
                    return Err(conflict(
                        "Lease renewal would not advance the bounded expiry",
                    ));
                }
                attempt.expires_at_millis = expires;
                ConsolidationAction::Renewed
            }
            ConsolidationOperation::Publish {
                fence,
                assertions,
                usage,
                evidence_ref,
                ..
            } => {
                evidence(evidence_ref)?;
                require_lease(&job, author, *fence, self.consolidation_incarnation, now)?;
                check_sources(&snapshot, namespace, &job.spec)?;
                if assertions.len() > job.spec.max_relations {
                    return Err(Error::ValidationError(
                        "Consolidation output exceeds the job's assertion bound".into(),
                    ));
                }
                let mut inputs = Vec::new();
                for claim in assertions {
                    if !job.spec.sources.contains(&claim.source)
                        || !job.spec.sources.contains(&claim.target)
                    {
                        return Err(Error::ValidationError("Every output endpoint must be an exact revision in the immutable job manifest".into()));
                    }
                    let mut provenance = job.spec.extractor.clone();
                    provenance.evidence_ref = claim.evidence_ref.clone();
                    inputs.push(MemoryRelationInput {
                        source: claim.source.clone(),
                        target: claim.target.clone(),
                        kind: claim.kind,
                        provenance,
                        valid_from_millis: claim.valid_from_millis,
                        valid_until_millis: claim.valid_until_millis,
                        evidence_sources: job
                            .spec
                            .sources
                            .iter()
                            .filter(|s| {
                                s.record_id != claim.source.record_id
                                    && s.record_id != claim.target.record_id
                            })
                            .cloned()
                            .collect(),
                    });
                }
                job.output_receipts = self.prepare_relation_publication(
                    &snapshot, namespace, author, job.job_id, &inputs, &mut batch,
                )?;
                close_attempt(
                    &mut job,
                    now,
                    ConsolidationOutcome::Published,
                    usage.clone(),
                    evidence_ref,
                );
                job.status = ConsolidationStatus::Published;
                ConsolidationAction::Published
            }
            ConsolidationOperation::Fail {
                fence,
                usage,
                evidence_ref,
                ..
            } => {
                evidence(evidence_ref)?;
                require_lease(&job, author, *fence, self.consolidation_incarnation, now)?;
                close_attempt(
                    &mut job,
                    now,
                    ConsolidationOutcome::Failed,
                    usage.clone(),
                    evidence_ref,
                );
                job.status = retry_state(&job);
                ConsolidationAction::Failed
            }
            ConsolidationOperation::RecoverExpired { evidence_ref, .. } => {
                evidence(evidence_ref)?;
                if job.status != ConsolidationStatus::Running
                    || lease_active(&job, self.consolidation_incarnation, now)
                {
                    return Err(conflict(
                        "Only an expired or previous-incarnation lease can be recovered",
                    ));
                }
                close_attempt(
                    &mut job,
                    now,
                    ConsolidationOutcome::Unknown,
                    ConsolidationUsage::Unknown,
                    evidence_ref,
                );
                job.status = retry_state(&job);
                ConsolidationAction::Recovered
            }
            ConsolidationOperation::Cancel { evidence_ref, .. } => {
                evidence(evidence_ref)?;
                if !matches!(
                    job.status,
                    ConsolidationStatus::Ready | ConsolidationStatus::Running
                ) {
                    return Err(conflict("Consolidation job is already terminal"));
                }
                if job.status == ConsolidationStatus::Running {
                    // Fencing publication does not prove that external execution stopped.
                    close_attempt(
                        &mut job,
                        now,
                        ConsolidationOutcome::Unknown,
                        ConsolidationUsage::Unknown,
                        evidence_ref,
                    );
                }
                job.status = ConsolidationStatus::Cancelled;
                ConsolidationAction::Cancelled
            }
            ConsolidationOperation::ReconcileUsage {
                attempt_number,
                usage,
                evidence_ref,
                ..
            } => {
                evidence(evidence_ref)?;
                if *attempt_number == 0 || matches!(usage, ConsolidationUsage::Unknown) {
                    return Err(Error::ValidationError(
                        "Usage reconciliation requires a positive attempt and reported consumption"
                            .into(),
                    ));
                }
                let attempt = job
                    .attempts
                    .get_mut((*attempt_number - 1) as usize)
                    .ok_or_else(|| Error::KeyNotFound("Consolidation attempt not found".into()))?;
                if attempt.outcome == ConsolidationOutcome::Running {
                    return Err(conflict("Running attempt consumption is not final"));
                }
                attempt.usage = usage.clone();
                attempt.usage_evidence_ref = Some(evidence_ref.clone());
                // Historical uncertainty and all previous reports remain in immutable revisions.
                ConsolidationAction::UsageReconciled
            }
        };
        let receipt = self.append_consolidation_revision(
            &snapshot, namespace, author, command, &job, action, &mut batch,
        )?;
        let mut options = rocksdb::WriteOptions::default();
        options.disable_wal(false);
        options.set_sync(true);
        self.db.write_opt(batch, &options).map_err(storage_error)?;
        Ok(receipt)
    }
}
