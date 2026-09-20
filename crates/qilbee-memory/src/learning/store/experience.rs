//! Scoped experience observations. A recorded assertion does not qualify a procedure.
use super::*;
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperienceActor {
    pub subject_id: String,
    pub credential_id: String,
}
impl ExperienceActor {
    fn validate(&self) -> Result<()> {
        validate_text(&self.subject_id, "experience subject", 512)?;
        validate_text(&self.credential_id, "experience credential", 512)
    }
}

/// The digest and reference are declarations; this ledger does not fetch evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperienceEvidence {
    pub reference: String,
    pub sha256: String,
}
impl ExperienceEvidence {
    fn validate(&self) -> Result<()> {
        validate_text(&self.reference, "evidence reference", 2048)?;
        validate_digest(&self.sha256)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperienceParent {
    pub attempt_id: String,
    pub event_id: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperienceRequest {
    pub id: String,
    pub context_id: String,
    pub reporter_subject_id: String,
    pub accounting_unit: String,
    pub input: ExperienceEvidence,
    pub parent: Option<ExperienceParent>,
}
impl ExperienceRequest {
    fn validate(&self) -> Result<()> {
        for (label, value) in [
            ("attempt ID", &self.id),
            ("context ID", &self.context_id),
            ("reporter subject", &self.reporter_subject_id),
            ("accounting unit", &self.accounting_unit),
        ] {
            validate_text(value, label, 512)?;
        }
        self.input.validate()?;
        if let Some(parent) = &self.parent {
            validate_text(&parent.attempt_id, "parent attempt ID", 512)?;
            validate_text(&parent.event_id, "parent event ID", 512)?;
            if parent.attempt_id == self.id {
                return Err(Error::ValidationError(
                    "An attempt cannot be its own parent".into(),
                ));
            }
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperienceReceipt {
    pub schema_version: u32,
    pub tenant: String,
    pub namespace: String,
    pub request: ExperienceRequest,
    pub context_digest: String,
    pub parent_event_digest: Option<String>,
    pub actor: ExperienceActor,
    pub recorded_at_millis: i64,
    pub receipt_digest: String,
}
impl ExperienceReceipt {
    fn digest(&self) -> Result<String> {
        registry::digest(&(
            &self.schema_version,
            &self.tenant,
            &self.namespace,
            &self.request,
            &self.context_digest,
            &self.parent_event_digest,
            &self.actor,
            self.recorded_at_millis,
        ))
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExperienceOutcome {
    Unknown,
    Succeeded,
    Failed,
    Cancelled,
}
impl ExperienceOutcome {
    fn terminal(self) -> bool {
        self != Self::Unknown
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperienceRecord {
    pub receipt: ExperienceReceipt,
    pub revision: u64,
    pub outcome: Option<ExperienceOutcome>,
    pub last_event_id: Option<String>,
    pub reported_cost_units: Option<u64>,
    pub reported_latency_ms: Option<u64>,
    pub observed_cost_units: Option<u64>,
    pub observed_latency_ms: Option<u64>,
}
impl ExperienceRecord {
    fn initial(receipt: ExperienceReceipt) -> Self {
        Self {
            receipt,
            revision: 1,
            outcome: None,
            last_event_id: None,
            reported_cost_units: None,
            reported_latency_ms: None,
            observed_cost_units: None,
            observed_latency_ms: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperienceCommand {
    pub event_id: String,
    pub expected_revision: u64,
    pub context_digest: String,
    pub outcome: ExperienceOutcome,
    pub evidence: ExperienceEvidence,
    pub cost_units: Option<u64>,
    pub latency_ms: Option<u64>,
}
impl ExperienceCommand {
    fn validate(&self) -> Result<()> {
        validate_text(&self.event_id, "experience event ID", 512)?;
        validate_digest(&self.context_digest)?;
        self.evidence.validate()?;
        if self.expected_revision == 0 || self.expected_revision == u64::MAX {
            return Err(Error::ValidationError(
                "Expected revision must allow a successor".into(),
            ));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperienceEvent {
    pub schema_version: u32,
    pub command: ExperienceCommand,
    pub actor: ExperienceActor,
    pub record: ExperienceRecord,
    pub recorded_at_millis: i64,
    pub event_digest: String,
}
impl ExperienceEvent {
    fn digest(&self) -> Result<String> {
        registry::digest(&(
            self.schema_version,
            &self.command,
            &self.actor,
            &self.record,
            self.recorded_at_millis,
        ))
    }
}

fn validate_digest(value: &str) -> Result<()> {
    if value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        Ok(())
    } else {
        Err(Error::ValidationError(
            "Expected 64 lowercase SHA-256 hexadecimal digits".into(),
        ))
    }
}
pub(super) fn key(kind: u8, tenant: &str, namespace: &str, id: &str) -> Result<Vec<u8>> {
    validate_text(tenant, "tenant", 512)?;
    validate_text(namespace, "experience namespace", 4096)?;
    validate_text(id, "attempt ID", 512)?;
    let mut key = vec![kind];
    for value in [tenant, namespace, id] {
        append_component(&mut key, value);
    }
    Ok(key)
}
pub(super) fn event_key(tenant: &str, namespace: &str, id: &str, event: &str) -> Result<Vec<u8>> {
    let mut key = key(13, tenant, namespace, id)?;
    validate_text(event, "experience event ID", 512)?;
    append_component(&mut key, event);
    Ok(key)
}
fn corrupt() -> Error {
    Error::DataCorruption("Experience identity, digest or state mismatch".into())
}

impl LearningMemory {
    /// Trusted library interface. HTTP callers need current scoped experience authority.
    pub fn create_experience(
        &self,
        tenant: &str,
        namespace: &str,
        request: ExperienceRequest,
        actor: ExperienceActor,
    ) -> Result<ExperienceReceipt> {
        let key = key(12, tenant, namespace, &request.id)?;
        request.validate()?;
        actor.validate()?;
        let _guard = self
            .inner
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Learning mutation lock poisoned".into()))?;
        if let Some(existing) = self.experience(tenant, namespace, &request.id)? {
            return if existing.receipt.request == request
                && existing.receipt.actor.subject_id == actor.subject_id
            {
                Ok(existing.receipt)
            } else {
                Err(Error::ConstraintViolation(
                    "Experience request IDs are immutable".into(),
                ))
            };
        }
        let context = self
            .context(tenant, &request.context_id)?
            .ok_or_else(|| Error::KeyNotFound("Experience context".into()))?;
        let parent_event_digest = request
            .parent
            .as_ref()
            .map(|p| {
                self.experience_event(tenant, namespace, &p.attempt_id, &p.event_id)?
                    .map(|event| event.event_digest)
                    .ok_or_else(|| Error::KeyNotFound("Parent experience event".into()))
            })
            .transpose()?;
        let mut receipt = ExperienceReceipt {
            schema_version: 1,
            tenant: tenant.into(),
            namespace: namespace.into(),
            request,
            context_digest: context.payload_digest,
            parent_event_digest,
            actor,
            recorded_at_millis: chrono::Utc::now().timestamp_millis(),
            receipt_digest: String::new(),
        };
        receipt.receipt_digest = receipt.digest()?;
        self.inner
            .db
            .put_opt(
                key,
                encode(&ExperienceRecord::initial(receipt.clone()))?,
                &write_options(),
            )
            .map_err(storage_error)?;
        Ok(receipt)
    }

    fn verify_experience_receipt(
        &self,
        tenant: &str,
        namespace: &str,
        id: &str,
        receipt: &ExperienceReceipt,
    ) -> Result<()> {
        receipt.request.validate().map_err(|_| corrupt())?;
        receipt.actor.validate().map_err(|_| corrupt())?;
        let context = self
            .context(tenant, &receipt.request.context_id)?
            .ok_or_else(corrupt)?;
        if receipt.schema_version != 1
            || receipt.tenant != tenant
            || receipt.namespace != namespace
            || receipt.request.id != id
            || receipt.context_digest != context.payload_digest
            || receipt.receipt_digest != receipt.digest()?
            || receipt.parent_event_digest.is_some() != receipt.request.parent.is_some()
        {
            return Err(corrupt());
        }
        Ok(())
    }
    fn read_experience_record(
        &self,
        tenant: &str,
        namespace: &str,
        id: &str,
    ) -> Result<Option<ExperienceRecord>> {
        self.inner
            .db
            .get(key(12, tenant, namespace, id)?)
            .map_err(storage_error)?
            .map(|bytes| {
                let record: ExperienceRecord = decode(&bytes)?;
                self.verify_experience_receipt(tenant, namespace, id, &record.receipt)?;
                Ok(record)
            })
            .transpose()
    }
    pub fn experience(
        &self,
        tenant: &str,
        namespace: &str,
        id: &str,
    ) -> Result<Option<ExperienceRecord>> {
        let Some(record) = self.read_experience_record(tenant, namespace, id)? else {
            return Ok(None);
        };
        match &record.last_event_id {
            Some(event) => {
                let event = self
                    .experience_event(tenant, namespace, id, event)?
                    .ok_or_else(corrupt)?;
                if event.record != record {
                    return Err(corrupt());
                }
            }
            None if record == ExperienceRecord::initial(record.receipt.clone()) => (),
            None => return Err(corrupt()),
        }
        Ok(Some(record))
    }
    /// Read an immutable historical event, not the latest attempt state.
    pub fn experience_event(
        &self,
        tenant: &str,
        namespace: &str,
        id: &str,
        event: &str,
    ) -> Result<Option<ExperienceEvent>> {
        self.inner
            .db
            .get(event_key(tenant, namespace, id, event)?)
            .map_err(storage_error)?
            .map(|bytes| {
                let receipt: ExperienceEvent = decode(&bytes)?;
                receipt.command.validate().map_err(|_| corrupt())?;
                receipt.actor.validate().map_err(|_| corrupt())?;
                self.verify_experience_receipt(tenant, namespace, id, &receipt.record.receipt)?;
                let current = self
                    .read_experience_record(tenant, namespace, id)?
                    .ok_or_else(corrupt)?;
                if receipt.schema_version != 1
                    || receipt.command.event_id != event
                    || receipt.record.last_event_id.as_deref() != Some(event)
                    || receipt.command.expected_revision.checked_add(1)
                        != Some(receipt.record.revision)
                    || receipt.command.context_digest != receipt.record.receipt.context_digest
                    || Some(receipt.command.outcome) != receipt.record.outcome
                    || receipt.command.cost_units != receipt.record.reported_cost_units
                    || receipt.command.latency_ms != receipt.record.reported_latency_ms
                    || receipt.actor.subject_id
                        != receipt.record.receipt.request.reporter_subject_id
                    || current.receipt != receipt.record.receipt
                    || receipt.event_digest != receipt.digest()?
                {
                    return Err(corrupt());
                }
                Ok(receipt)
            })
            .transpose()
    }

    /// Commit the event and resulting state atomically, retaining cumulative lower bounds.
    pub fn observe_experience(
        &self,
        tenant: &str,
        namespace: &str,
        id: &str,
        command: ExperienceCommand,
        actor: ExperienceActor,
    ) -> Result<ExperienceEvent> {
        let event_key = event_key(tenant, namespace, id, &command.event_id)?;
        command.validate()?;
        actor.validate()?;
        let _guard = self
            .inner
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Learning mutation lock poisoned".into()))?;
        let mut record = self
            .experience(tenant, namespace, id)?
            .ok_or_else(|| Error::KeyNotFound("Experience attempt".into()))?;
        if actor.subject_id != record.receipt.request.reporter_subject_id {
            return Err(Error::Unauthorized(
                "Experience reporter does not match the bound subject".into(),
            ));
        }
        if let Some(existing) = self.experience_event(tenant, namespace, id, &command.event_id)? {
            return if existing.command == command && existing.actor.subject_id == actor.subject_id {
                Ok(existing)
            } else {
                Err(Error::ConstraintViolation(
                    "Experience event IDs are immutable".into(),
                ))
            };
        }
        if command.expected_revision != record.revision {
            return Err(Error::TransactionConflict(
                "Experience revision changed".into(),
            ));
        }
        if record
            .outcome
            .is_some_and(|outcome| outcome.terminal() && outcome != command.outcome)
        {
            return Err(Error::ConstraintViolation(
                "A terminal outcome cannot change; retain its history and use a new attempt".into(),
            ));
        }
        if command.context_digest != record.receipt.context_digest {
            return Err(Error::ConstraintViolation(
                "Experience context digest differs".into(),
            ));
        }
        for (previous, next) in [
            (record.observed_cost_units, command.cost_units),
            (record.observed_latency_ms, command.latency_ms),
        ] {
            if previous.zip(next).is_some_and(|(old, new)| new < old) {
                return Err(Error::ValidationError(
                    "Cumulative experience consumption cannot decrease".into(),
                ));
            }
        }
        record.revision = command.expected_revision + 1;
        record.outcome = Some(command.outcome);
        record.last_event_id = Some(command.event_id.clone());
        record.reported_cost_units = command.cost_units;
        record.reported_latency_ms = command.latency_ms;
        record.observed_cost_units = command.cost_units.or(record.observed_cost_units);
        record.observed_latency_ms = command.latency_ms.or(record.observed_latency_ms);
        let mut event = ExperienceEvent {
            schema_version: 1,
            command,
            actor,
            record,
            recorded_at_millis: chrono::Utc::now().timestamp_millis(),
            event_digest: String::new(),
        };
        event.event_digest = event.digest()?;
        let mut batch = WriteBatch::default();
        batch.put(event_key, encode(&event)?);
        batch.put(key(12, tenant, namespace, id)?, encode(&event.record)?);
        self.inner
            .db
            .write_opt(batch, &write_options())
            .map_err(storage_error)?;
        Ok(event)
    }
}

#[cfg(test)]
mod integrity_tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    #[test]
    fn experience_reads_fail_closed_for_modified_receipts_events_and_state() {
        let dir = TempDir::new().unwrap();
        let db = LearningMemory::open(dir.path()).unwrap();
        let context = serde_json::from_value(json!({"task":"task","baseline_revision":"base","model_provider":"provider","model_revision":"model","tools":{},"environment_revision":"env","evaluation_contract":"verifier","dataset_revision":"dataset","harness_revision":"harness","permissions_revision":"permissions"})).unwrap();
        db.register_context("tenant", "context", context, "operator")
            .unwrap();
        let actor = ExperienceActor {
            subject_id: "observer".into(),
            credential_id: "credential".into(),
        };
        let input = ExperienceRequest {
            id: "attempt".into(),
            context_id: "context".into(),
            reporter_subject_id: "observer".into(),
            accounting_unit: "credits-v1".into(),
            input: ExperienceEvidence {
                reference: "input".into(),
                sha256: "a".repeat(64),
            },
            parent: None,
        };
        let receipt = db
            .create_experience("tenant", "scope", input, actor.clone())
            .unwrap();
        let command = ExperienceCommand {
            event_id: "event".into(),
            expected_revision: 1,
            context_digest: receipt.context_digest,
            outcome: ExperienceOutcome::Unknown,
            evidence: ExperienceEvidence {
                reference: "trace".into(),
                sha256: "b".repeat(64),
            },
            cost_units: Some(7),
            latency_ms: None,
        };
        let event = db
            .observe_experience("tenant", "scope", "attempt", command, actor)
            .unwrap();
        let record_key = key(12, "tenant", "scope", "attempt").unwrap();
        let event_key = event_key("tenant", "scope", "attempt", "event").unwrap();
        let mut bad = event.record.clone();
        bad.receipt.context_digest = "c".repeat(64);
        db.inner.db.put(&record_key, encode(&bad).unwrap()).unwrap();
        assert!(matches!(
            db.experience("tenant", "scope", "attempt"),
            Err(Error::DataCorruption(_))
        ));
        db.inner
            .db
            .put(&record_key, encode(&event.record).unwrap())
            .unwrap();
        let mut bad = event.clone();
        bad.command.cost_units = Some(8);
        db.inner.db.put(&event_key, encode(&bad).unwrap()).unwrap();
        assert!(matches!(
            db.experience_event("tenant", "scope", "attempt", "event"),
            Err(Error::DataCorruption(_))
        ));
        db.inner
            .db
            .put(&event_key, encode(&event).unwrap())
            .unwrap();
        let mut bad = event.record.clone();
        bad.observed_cost_units = Some(0);
        db.inner.db.put(&record_key, encode(&bad).unwrap()).unwrap();
        assert!(matches!(
            db.experience("tenant", "scope", "attempt"),
            Err(Error::DataCorruption(_))
        ));
        db.inner
            .db
            .put(&record_key, encode(&event.record).unwrap())
            .unwrap();
        db.inner.db.delete(&event_key).unwrap();
        assert!(matches!(
            db.experience("tenant", "scope", "attempt"),
            Err(Error::DataCorruption(_))
        ));
    }
}
