//! Durable typed memory assertions sharing canonical endpoint authority and WAL.
use super::snapshot::MemorySnapshot;
use super::*;
mod types;
pub use types::*;
mod graph;
pub use graph::*;
mod adjacency;
mod changes;
mod publication;
pub use changes::*;

// All relation values use AGENT_META; ordinary memory records remain canonical.
const RELATION: u8 = 0x50;
const INTEGRITY: u8 = 0x51;
const HISTORY: u8 = 0x52;
const RECEIPT: u8 = 0x53;
const OUTGOING: u8 = 0x54;
const INCOMING: u8 = 0x55;
const MAX_RELATION_RECORD_BYTES: usize = 16 * 1024;

fn hex_digest(bytes: &[u8]) -> String {
    digest(bytes).iter().map(|v| format!("{v:02x}")).collect()
}
fn valid(value: &str, limit: usize) -> bool {
    !value.trim().is_empty() && value.len() <= limit && !value.chars().any(char::is_control)
}
fn history_key(namespace: &str, id: Uuid, revision: u64) -> Vec<u8> {
    let mut key = record_key(HISTORY, namespace, id);
    key.extend(revision.to_be_bytes());
    key
}
fn adjacency_key(kind: u8, namespace: &str, endpoint: &MemorySourceRef, id: Uuid) -> Vec<u8> {
    let mut key = record_key(kind, namespace, endpoint.record_id);
    key.extend(endpoint.revision.to_be_bytes());
    key.extend(id.as_bytes());
    key
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredRelationReceipt {
    schema_version: u32,
    namespace: String,
    request_digest: [u8; 32],
    receipt: MemoryRelationReceipt,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RelationIntegrity {
    schema_version: u32,
    revision: u64,
    record_digest: [u8; 32],
    history_digest: [u8; 32],
}
impl MemoryRelationReceipt {
    fn calculate_digest(&self, namespace: &str) -> Result<String> {
        let mut receipt = self.clone();
        receipt.receipt_digest.clear();
        Ok(digest(&encode(&(namespace, receipt))?)
            .iter()
            .map(|v| format!("{v:02x}"))
            .collect())
    }
    fn validate(&self, namespace: &str) -> Result<()> {
        if self.contract_version != 1
            || self.revision == 0
            || !valid(&self.idempotency_key, 256)
            || !valid(&self.author.subject_id, 256)
            || !valid(&self.evidence_ref, 2048)
            || ![&self.relation_digest, &self.command_digest]
                .iter()
                .all(|d| {
                    d.len() == 64
                        && d.bytes()
                            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
                })
            || self.receipt_digest != self.calculate_digest(namespace)?
        {
            return Err(inconsistent());
        }
        Ok(())
    }
}
fn validate_relation(relation: &MemoryRelation) -> Result<()> {
    relation.input.validate().map_err(|_| inconsistent())?;
    if relation.schema_version != 1
        || relation.revision == 0
        || !valid(&relation.reported_by.subject_id, 256)
        || relation
            .review
            .as_ref()
            .is_some_and(|r| !valid(&r.evidence_ref, 2048) || !valid(&r.author.subject_id, 256))
    {
        return Err(inconsistent());
    }
    Ok(())
}
fn local_relation_reason(relation: &MemoryRelation, now: i64) -> RelationEligibilityReason {
    if relation.state == RelationState::Retired {
        RelationEligibilityReason::Retired
    } else if !relation.indexed() {
        RelationEligibilityReason::Rejected
    } else if relation.input.valid_from_millis.is_some_and(|t| now < t) {
        RelationEligibilityReason::NotYetValid
    } else if relation.input.valid_until_millis.is_some_and(|t| now >= t) {
        RelationEligibilityReason::Expired
    } else {
        RelationEligibilityReason::Eligible
    }
}
impl MemorySnapshot<'_> {
    pub(super) fn relation_revision(
        &self,
        namespace: &str,
        id: Uuid,
        revision: u64,
    ) -> Result<Option<MemoryRelationRevision>> {
        let bytes = self
            .db
            .get_cf(
                self.storage.cf(super::super::cf::AGENT_META)?,
                history_key(namespace, id, revision),
            )
            .map_err(storage_error)?;
        bytes
            .map(|bytes| {
                if bytes.len() > MAX_RELATION_RECORD_BYTES {
                    return Err(inconsistent());
                }
                let stored: MemoryRelationRevision = decode(&bytes)?;
                validate_relation(&stored.relation)?;
                stored.receipt.validate(namespace)?;
                if stored.relation.relation_id != id
                    || stored.relation.revision != revision
                    || stored.receipt.relation_id != id
                    || stored.receipt.revision != revision
                    || stored.receipt.relation_digest != hex_digest(&encode(&stored.relation)?)
                {
                    return Err(inconsistent());
                }
                Ok(stored)
            })
            .transpose()
    }
    pub(super) fn relation(&self, namespace: &str, id: Uuid) -> Result<Option<MemoryRelation>> {
        Ok(self
            .relation_with_bytes(namespace, id)?
            .map(|(relation, _, _)| relation))
    }
    fn relation_with_bytes(
        &self,
        namespace: &str,
        id: Uuid,
    ) -> Result<Option<(MemoryRelation, usize, Vec<u8>)>> {
        let cf = self.storage.cf(super::super::cf::AGENT_META)?;
        let bytes = self
            .db
            .get_cf(cf, record_key(RELATION, namespace, id))
            .map_err(storage_error)?;
        let integrity = self
            .db
            .get_cf(cf, record_key(INTEGRITY, namespace, id))
            .map_err(storage_error)?;
        let (bytes, integrity) = match (bytes, integrity) {
            (None, None) => return Ok(None),
            (Some(b), Some(i)) => (b, i),
            _ => return Err(inconsistent()),
        };
        if bytes.len() > MAX_RELATION_RECORD_BYTES {
            return Err(inconsistent());
        }
        let relation: MemoryRelation = decode(&bytes)?;
        let index: RelationIntegrity = decode(&integrity)?;
        validate_relation(&relation)?;
        if relation.relation_id != id
            || index.schema_version != 1
            || index.revision != relation.revision
            || index.record_digest != digest(&bytes)
        {
            return Err(inconsistent());
        }
        let history = self
            .relation_revision(namespace, id, relation.revision)?
            .ok_or_else(inconsistent)?;
        if history.relation != relation || index.history_digest != digest(&encode(&history)?) {
            return Err(inconsistent());
        }
        for (kind, endpoint) in [
            (OUTGOING, &relation.input.source),
            (INCOMING, &relation.input.target),
        ] {
            let adjacency = self
                .db
                .get_cf(cf, adjacency_key(kind, namespace, endpoint, id))
                .map_err(storage_error)?;
            if relation.indexed() {
                if adjacency.as_deref() != Some(integrity.as_slice()) {
                    return Err(inconsistent());
                }
            } else if adjacency.is_some() {
                return Err(inconsistent());
            }
        }
        Ok(Some((relation, bytes.len(), integrity)))
    }
    pub(super) fn relation_eligibility(
        &self,
        namespace: &str,
        relation: &MemoryRelation,
    ) -> Result<RelationEligibility> {
        let mut reason = local_relation_reason(relation, self.now);
        let mut endpoint = None;
        let mut evidence_failure = None;
        if reason == RelationEligibilityReason::Eligible {
            if let Err(failure) = self.relation_endpoints(namespace, &relation.input)? {
                reason = failure.0;
                endpoint = Some(failure.1);
            }
        }
        if reason == RelationEligibilityReason::Eligible {
            evidence_failure =
                self.relation_evidence_failure(namespace, &relation.input.evidence_sources)?;
            if evidence_failure.is_some() {
                reason = RelationEligibilityReason::EvidenceUnavailable;
            }
        }
        Ok(RelationEligibility {
            eligible: reason == RelationEligibilityReason::Eligible,
            reason,
            evaluated_at_millis: self.now,
            endpoint,
            dependency_work: self.dependency_work(),
            evidence_failure,
        })
    }
    fn relation_endpoints(
        &self,
        namespace: &str,
        input: &MemoryRelationInput,
    ) -> Result<std::result::Result<[i64; 2], (RelationEligibilityReason, MemorySourceRef)>> {
        let mut bytes = 0usize;
        let mut times = [0; 2];
        for (i, reference) in [&input.source, &input.target].into_iter().enumerate() {
            let Some((record, size, _)) = self.record_with_bytes(namespace, reference.record_id)?
            else {
                return Ok(Err((
                    RelationEligibilityReason::EndpointUnavailable,
                    reference.clone(),
                )));
            };
            bytes = bytes.saturating_add(size);
            if bytes > MAX_MEMORY_READ_BYTES {
                return Err(Error::ValidationError(
                    "Relation endpoint byte budget exceeded".into(),
                ));
            }
            if record.revision != reference.revision {
                return Ok(Err((
                    RelationEligibilityReason::EndpointRevisionChanged,
                    reference.clone(),
                )));
            }
            if !self.eligible(namespace, &record)? {
                return Ok(Err((
                    RelationEligibilityReason::EndpointUnavailable,
                    reference.clone(),
                )));
            }
            times[i] = record.payload.ok_or_else(inconsistent)?.event_time_millis;
        }
        Ok(Ok(times))
    }
    fn validate_relation_endpoints(
        &self,
        namespace: &str,
        input: &MemoryRelationInput,
    ) -> Result<()> {
        let times = self.relation_endpoints(namespace, input)?.map_err(|_| {
            Error::TransactionConflict(
                "Relation endpoint is not an eligible current revision".into(),
            )
        })?;
        if input.kind == MemoryRelationKind::TemporalBefore && times[0] >= times[1] {
            return Err(Error::ValidationError(
                "temporal_before requires a strictly earlier source event timestamp".into(),
            ));
        }
        if let Some(failure) = self.relation_evidence_failure(namespace, &input.evidence_sources)? {
            return Err(match failure.reason {
                MemoryEligibilityReason::DepthLimit
                | MemoryEligibilityReason::NodeLimit
                | MemoryEligibilityReason::DependencyCycle => Error::ValidationError(
                    "Relation evidence exceeds dependency graph bounds or contains a cycle".into(),
                ),
                _ => Error::TransactionConflict(
                    "Relation evidence is not an eligible current revision".into(),
                ),
            });
        }
        Ok(())
    }
}
impl RocksDbMemoryStorage {
    /// Callers authorize the namespace and actor; review requires separate authority.
    /// Immutable endpoint/provenance input cannot be rewritten by retirement or review.
    pub fn apply_memory_relation_command(
        &self,
        namespace: &str,
        author: &RecordAuthor,
        command: &MemoryRelationCommand,
    ) -> Result<MemoryRelationReceipt> {
        Self::validate_agent(namespace)?;
        if command.contract_version != 1
            || !valid(&command.idempotency_key, 256)
            || !valid(&author.subject_id, 256)
        {
            return Err(Error::ValidationError(
                "Invalid relation command identity or version".into(),
            ));
        }
        let mut key = record_prefix(RECEIPT, namespace);
        key.extend(encode(&(&author.subject_id, &command.idempotency_key))?);
        let request_digest = digest(&encode(command)?);
        let _guard = self
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Memory mutation lock poisoned".into()))?;
        let cf = self.cf(super::super::cf::AGENT_META)?;
        let snapshot = self.memory_snapshot();
        if let Some(bytes) = snapshot.db.get_cf(cf, &key).map_err(storage_error)? {
            let stored: StoredRelationReceipt = decode(&bytes)?;
            stored.receipt.validate(namespace)?;
            if stored.schema_version != 1
                || stored.namespace != namespace
                || stored.receipt.author.subject_id != author.subject_id
                || stored.receipt.idempotency_key != command.idempotency_key
                || stored.receipt.command_digest
                    != stored
                        .request_digest
                        .iter()
                        .map(|v| format!("{v:02x}"))
                        .collect::<String>()
            {
                return Err(inconsistent());
            }
            if stored.request_digest != request_digest {
                return Err(Error::ConstraintViolation(
                    "Idempotency key was used for a different relation command".into(),
                ));
            }
            let historical = snapshot
                .relation_revision(
                    namespace,
                    stored.receipt.relation_id,
                    stored.receipt.revision,
                )?
                .ok_or_else(inconsistent)?;
            if historical.receipt != stored.receipt {
                return Err(inconsistent());
            }
            snapshot
                .relation(namespace, stored.receipt.relation_id)?
                .ok_or_else(inconsistent)?;
            return Ok(stored.receipt);
        }
        let now = snapshot.now;
        let (relation, action, evidence_ref) = match &command.operation {
            MemoryRelationOperation::Assert { relation: input } => {
                input.validate()?;
                snapshot.validate_relation_endpoints(namespace, input)?;
                let id = Uuid::new_v4();
                if snapshot.relation(namespace, id)?.is_some() {
                    return Err(Error::TransactionConflict(
                        "Relation identifier collision".into(),
                    ));
                }
                (
                    MemoryRelation {
                        schema_version: 1,
                        relation_id: id,
                        revision: 1,
                        input: input.clone(),
                        reported_by: author.clone(),
                        created_at_millis: now,
                        modified_at_millis: now,
                        state: RelationState::Active,
                        review: None,
                    },
                    RelationAction::Asserted,
                    input.provenance.evidence_ref.clone(),
                )
            }
            MemoryRelationOperation::Retire {
                relation_id,
                expected_revision,
                evidence_ref,
            }
            | MemoryRelationOperation::Restore {
                relation_id,
                expected_revision,
                evidence_ref,
            }
            | MemoryRelationOperation::Review {
                relation_id,
                expected_revision,
                evidence_ref,
                ..
            } => {
                if *expected_revision == 0 || !valid(evidence_ref, 2048) {
                    return Err(Error::ValidationError(
                        "Relation changes require a positive revision and evidence reference"
                            .into(),
                    ));
                }
                let mut current = snapshot
                    .relation(namespace, *relation_id)?
                    .ok_or_else(|| Error::KeyNotFound("Relation not found in scope".into()))?;
                if current.revision != *expected_revision {
                    return Err(Error::TransactionConflict(
                        "Relation revision changed".into(),
                    ));
                }
                let action = match &command.operation {
                    MemoryRelationOperation::Retire { .. } => {
                        if current.state == RelationState::Retired {
                            return Err(Error::ValidationError(
                                "Relation is already retired".into(),
                            ));
                        }
                        current.state = RelationState::Retired;
                        RelationAction::Retired
                    }
                    MemoryRelationOperation::Restore { .. } => {
                        if current.state != RelationState::Retired {
                            return Err(Error::ValidationError(
                                "Only retired relations can be restored".into(),
                            ));
                        }
                        snapshot.validate_relation_endpoints(namespace, &current.input)?;
                        current.state = RelationState::Active;
                        RelationAction::Restored
                    }
                    MemoryRelationOperation::Review { disposition, .. } => {
                        current.review = Some(MemoryReview {
                            disposition: *disposition,
                            evidence_ref: evidence_ref.clone(),
                            author: author.clone(),
                            reviewed_at_millis: now,
                        });
                        RelationAction::Reviewed
                    }
                    _ => unreachable!(),
                };
                current.revision = current
                    .revision
                    .checked_add(1)
                    .ok_or_else(|| Error::ValidationError("Relation revision exhausted".into()))?;
                current.modified_at_millis = now;
                (current, action, evidence_ref.clone())
            }
        };
        let mut receipt = MemoryRelationReceipt {
            contract_version: 1,
            idempotency_key: command.idempotency_key.clone(),
            relation_id: relation.relation_id,
            revision: relation.revision,
            action,
            author: author.clone(),
            committed_at_millis: now,
            evidence_ref,
            relation_digest: hex_digest(&encode(&relation)?),
            command_digest: hex_digest(&encode(command)?),
            receipt_digest: String::new(),
        };
        receipt.receipt_digest = receipt.calculate_digest(namespace)?;
        let history_key = history_key(namespace, relation.relation_id, relation.revision);
        if snapshot
            .db
            .get_cf(cf, &history_key)
            .map_err(storage_error)?
            .is_some()
        {
            return Err(inconsistent());
        }
        let history = encode(&MemoryRelationRevision {
            relation: relation.clone(),
            receipt: receipt.clone(),
        })?;
        let bytes = encode(&relation)?;
        if history.len() > MAX_RELATION_RECORD_BYTES || bytes.len() > MAX_RELATION_RECORD_BYTES {
            return Err(Error::ValidationError(
                "Relation metadata exceeds the 16 KiB bound".into(),
            ));
        }
        let integrity = encode(&RelationIntegrity {
            schema_version: 1,
            revision: relation.revision,
            record_digest: digest(&bytes),
            history_digest: digest(&history),
        })?;
        let mut batch = rocksdb::WriteBatch::default();
        self.append_relation_change(&snapshot, namespace, &relation, &receipt, &mut batch)?;
        self.append_relation_heads(&snapshot, namespace, &relation, &integrity, &mut batch)?;
        batch.put_cf(
            cf,
            record_key(RELATION, namespace, relation.relation_id),
            bytes,
        );
        batch.put_cf(
            cf,
            record_key(INTEGRITY, namespace, relation.relation_id),
            &integrity,
        );
        batch.put_cf(cf, history_key, history);
        batch.put_cf(
            cf,
            key,
            encode(&StoredRelationReceipt {
                schema_version: 1,
                namespace: namespace.into(),
                request_digest,
                receipt: receipt.clone(),
            })?,
        );
        for (kind, endpoint) in [
            (OUTGOING, &relation.input.source),
            (INCOMING, &relation.input.target),
        ] {
            let key = adjacency_key(kind, namespace, endpoint, relation.relation_id);
            if relation.indexed() {
                batch.put_cf(cf, key, &integrity);
            } else {
                batch.delete_cf(cf, key);
            }
        }
        let mut options = rocksdb::WriteOptions::default();
        options.disable_wal(false);
        options.set_sync(true);
        self.db.write_opt(batch, &options).map_err(storage_error)?;
        Ok(receipt)
    }
    pub fn inspect_memory_relation(
        &self,
        namespace: &str,
        id: Uuid,
    ) -> Result<Option<MemoryRelationInspection>> {
        Self::validate_agent(namespace)?;
        let snapshot = self.memory_snapshot();
        snapshot
            .relation(namespace, id)?
            .map(|relation| {
                let eligibility = snapshot.relation_eligibility(namespace, &relation)?;
                Ok(MemoryRelationInspection {
                    relation,
                    eligibility,
                })
            })
            .transpose()
    }
    pub fn read_memory_relation(
        &self,
        namespace: &str,
        id: Uuid,
    ) -> Result<Option<MemoryRelation>> {
        Ok(self
            .inspect_memory_relation(namespace, id)?
            .filter(|r| r.eligibility.eligible)
            .map(|r| r.relation))
    }
    pub fn memory_relation_revision(
        &self,
        namespace: &str,
        id: Uuid,
        revision: u64,
    ) -> Result<Option<MemoryRelationRevision>> {
        Self::validate_agent(namespace)?;
        if revision == 0 {
            return Err(Error::ValidationError(
                "Relation revision must be positive".into(),
            ));
        }
        self.memory_snapshot()
            .relation_revision(namespace, id, revision)
    }
}
#[cfg(test)]
mod evidence_tests;
#[cfg(test)]
mod tests;
