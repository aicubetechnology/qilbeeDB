//! Monotonic exact-observation withdrawal; historical evidence remains immutable.
use super::experience::{ExperienceActor, ExperienceEvent, ExperienceRecord};
use super::experience_export::ExperienceExportRef;
use super::*;
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperienceWithdrawalCommand {
    pub idempotency_key: String,
    pub observation: ExperienceExportRef,
    pub reason: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperienceWithdrawalReceipt {
    pub withdrawal_id: uuid::Uuid,
    pub observation: ExperienceExportRef,
    pub status: String,
    pub reason: String,
    pub actor: ExperienceActor,
    pub committed_at_millis: i64,
    pub receipt_digest: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperienceWithdrawalInspection {
    pub evaluated_at_millis: i64,
    pub observation: ExperienceExportRef,
    pub status: String,
    pub receipt: Option<ExperienceWithdrawalReceipt>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StoredWithdrawal {
    pub(super) schema_version: u32,
    pub(super) tenant: String,
    pub(super) namespace: String,
    pub(super) command: ExperienceWithdrawalCommand,
    pub(super) receipt: ExperienceWithdrawalReceipt,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperienceReuseWork {
    pub learning_records_examined: usize,
    pub learning_bytes_inspected: usize,
    pub lookahead_bytes: usize,
}
/// Shared request-wide accounting. Misses consume record work; decode follows byte admission.
pub struct ExperienceReuseBudget {
    pub work: ExperienceReuseWork,
    records: usize,
    bytes: usize,
    exhausted: bool,
}
impl ExperienceReuseBudget {
    pub fn new(records: usize, bytes: usize) -> Self {
        Self {
            work: ExperienceReuseWork::default(),
            records,
            bytes,
            exhausted: false,
        }
    }
    pub(super) fn with_limits<T>(
        &mut self,
        records: usize,
        bytes: usize,
        action: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<T> {
        let old = (self.records, self.bytes);
        self.records = self
            .records
            .min(self.work.learning_records_examined.saturating_add(records));
        self.bytes = self
            .bytes
            .min(self.work.learning_bytes_inspected.saturating_add(bytes));
        let result = action(self);
        self.records = old.0;
        self.bytes = old.1;
        result
    }
    pub fn charge_record(&mut self) -> Result<()> {
        if self.exhausted || self.work.learning_records_examined >= self.records {
            self.exhausted = true;
            return Err(Error::ExperienceReuseLimitExceeded);
        }
        self.work.learning_records_examined += 1;
        Ok(())
    }
    pub(crate) fn remaining_records(&self) -> usize {
        if self.exhausted {
            0
        } else {
            self.records
                .saturating_sub(self.work.learning_records_examined)
        }
    }
    pub(crate) fn remaining_bytes(&self) -> usize {
        if self.exhausted {
            0
        } else {
            self.bytes
                .saturating_sub(self.work.learning_bytes_inspected)
        }
    }
    pub fn charge_bytes(&mut self, bytes: usize) -> Result<()> {
        if self.exhausted
            || bytes
                > self
                    .bytes
                    .saturating_sub(self.work.learning_bytes_inspected)
        {
            self.exhausted = true;
            self.work.lookahead_bytes = bytes;
            return Err(Error::ExperienceReuseLimitExceeded);
        }
        self.work.learning_bytes_inspected += bytes;
        Ok(())
    }
    pub(super) fn read<T: DeserializeOwned>(
        &mut self,
        store: &LearningMemory,
        key: Vec<u8>,
    ) -> Result<Option<T>> {
        self.charge_record()?;
        let Some(bytes) = store.inner.db.get(key).map_err(storage_error)? else {
            return Ok(None);
        };
        self.charge_bytes(bytes.len())?;
        decode(&bytes).map(Some)
    }
}
fn strict_text(value: &str, name: &str, limit: usize) -> Result<()> {
    validate_text(value, name, limit)?;
    if value.chars().any(char::is_control) {
        return Err(Error::ValidationError(format!("Invalid {name}")));
    }
    Ok(())
}
pub(super) fn validate_reference(reference: &ExperienceExportRef) -> Result<()> {
    strict_text(&reference.attempt_id, "attempt ID", 512)?;
    strict_text(&reference.event_id, "event ID", 512)?;
    super::experience::validate_digest(&reference.event_digest)
}
pub(super) fn target_key(
    tenant: &str,
    namespace: &str,
    reference: &ExperienceExportRef,
) -> Result<Vec<u8>> {
    validate_reference(reference)?;
    let mut key = super::experience::key(24, tenant, namespace, &reference.attempt_id)?;
    append_component(&mut key, &reference.event_id);
    Ok(key)
}
pub(super) fn command_key(
    tenant: &str,
    namespace: &str,
    actor: &ExperienceActor,
    id: &str,
) -> Result<Vec<u8>> {
    let mut key = super::experience::key(25, tenant, namespace, &actor.credential_id)?;
    append_component(&mut key, &actor.subject_id);
    append_component(&mut key, id);
    Ok(key)
}
impl StoredWithdrawal {
    fn digest(&self) -> Result<String> {
        registry::digest(&(
            "qilbee.experience-withdrawal.v1",
            self.schema_version,
            &self.tenant,
            &self.namespace,
            &self.command,
            self.receipt.withdrawal_id,
            &self.receipt.observation,
            &self.receipt.status,
            &self.receipt.reason,
            &self.receipt.actor,
            self.receipt.committed_at_millis,
        ))
    }
    pub(super) fn validate(
        &self,
        tenant: &str,
        namespace: &str,
        reference: &ExperienceExportRef,
    ) -> Result<()> {
        let valid = validate_reference(&self.command.observation)
            .and_then(|_| strict_text(&self.command.idempotency_key, "idempotency key", 128))
            .and_then(|_| strict_text(&self.command.reason, "withdrawal reason", 2048))
            .and_then(|_| self.receipt.actor.validate());
        if valid.is_err()
            || self.schema_version != 1
            || self.tenant != tenant
            || self.namespace != namespace
            || &self.command.observation != reference
            || &self.receipt.observation != reference
            || self.receipt.withdrawal_id.get_version_num() != 4
            || self.receipt.status != "withdrawn"
            || self.receipt.reason != self.command.reason
            || self.receipt.receipt_digest != self.digest()?
        {
            return Err(Error::DataCorruption(
                "Invalid experience withdrawal binding".into(),
            ));
        }
        Ok(())
    }
}
impl LearningMemory {
    /// Caller holds the learning mutation lock for any current reuse or write decision.
    pub(super) fn verify_observation_with_budget(
        &self,
        tenant: &str,
        namespace: &str,
        reference: &ExperienceExportRef,
        budget: &mut ExperienceReuseBudget,
    ) -> Result<ExperienceEvent> {
        validate_reference(reference)?;
        let event: ExperienceEvent = budget
            .read(
                self,
                super::experience::event_key(
                    tenant,
                    namespace,
                    &reference.attempt_id,
                    &reference.event_id,
                )?,
            )?
            .ok_or_else(|| Error::KeyNotFound("Experience event".into()))?;
        let current: ExperienceRecord = budget
            .read(
                self,
                super::experience::key(12, tenant, namespace, &reference.attempt_id)?,
            )?
            .ok_or_else(|| Error::DataCorruption("Experience attempt missing".into()))?;
        let context: registry::RegistryEntry<registry::EvaluationContext> = budget
            .read(
                self,
                registry::registry_key(5, tenant, &event.record.receipt.request.context_id),
            )?
            .ok_or_else(|| Error::DataCorruption("Experience context missing".into()))?;
        if context.schema_version != 1
            || context.payload_digest != registry::digest(&context.payload)?
        {
            return Err(Error::DataCorruption("Invalid experience context".into()));
        }
        Self::validate_experience_receipt_context(
            tenant,
            namespace,
            &reference.attempt_id,
            &event.record.receipt,
            &context,
        )?;
        Self::validate_experience_receipt_context(
            tenant,
            namespace,
            &reference.attempt_id,
            &current.receipt,
            &context,
        )?;
        Self::validate_experience_event_record(&reference.event_id, &event, &current)?;
        if event.event_digest != reference.event_digest {
            return Err(Error::ExperienceReferenceConflict);
        }
        Ok(event)
    }
    /// Exact immutable event must already have been verified by the caller in this observation.
    pub(super) fn withdrawal_with_budget(
        &self,
        tenant: &str,
        namespace: &str,
        reference: &ExperienceExportRef,
        budget: &mut ExperienceReuseBudget,
    ) -> Result<Option<ExperienceWithdrawalReceipt>> {
        let Some(stored): Option<StoredWithdrawal> =
            budget.read(self, target_key(tenant, namespace, reference)?)?
        else {
            return Ok(None);
        };
        stored.validate(tenant, namespace, reference)?;
        let replay: StoredWithdrawal = budget
            .read(
                self,
                command_key(
                    tenant,
                    namespace,
                    &stored.receipt.actor,
                    &stored.command.idempotency_key,
                )?,
            )?
            .ok_or_else(|| Error::DataCorruption("Withdrawal command missing".into()))?;
        replay.validate(tenant, namespace, reference)?;
        if encode(&replay)? != encode(&stored)? {
            return Err(Error::DataCorruption("Withdrawal records disagree".into()));
        }
        Ok(Some(stored.receipt))
    }
    pub fn withdraw_experience(
        &self,
        tenant: &str,
        namespace: &str,
        command: ExperienceWithdrawalCommand,
        actor: &ExperienceActor,
    ) -> Result<ExperienceWithdrawalReceipt> {
        validate_reference(&command.observation)?;
        strict_text(&command.idempotency_key, "idempotency key", 128)?;
        strict_text(&command.reason, "withdrawal reason", 2048)?;
        actor.validate()?;
        let _guard = self
            .inner
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Learning mutation lock poisoned".into()))?;
        let mut budget = ExperienceReuseBudget::new(32, 2_097_152);
        let key = command_key(tenant, namespace, actor, &command.idempotency_key)?;
        let prior: Option<StoredWithdrawal> = budget.read(self, key.clone())?;
        if let Some(prior) = prior {
            prior.validate(tenant, namespace, &prior.command.observation)?;
            if prior.receipt.actor != *actor || prior.command != command {
                return Err(Error::ConstraintViolation(
                    "Withdrawal command identity conflict".into(),
                ));
            }
            self.verify_observation_with_budget(
                tenant,
                namespace,
                &command.observation,
                &mut budget,
            )?;
            let receipt = self
                .withdrawal_with_budget(tenant, namespace, &command.observation, &mut budget)?
                .ok_or_else(|| Error::DataCorruption("Withdrawal target missing".into()))?;
            if receipt != prior.receipt {
                return Err(Error::DataCorruption("Withdrawal replay differs".into()));
            }
            return Ok(receipt);
        }
        self.verify_observation_with_budget(tenant, namespace, &command.observation, &mut budget)?;
        if self
            .withdrawal_with_budget(tenant, namespace, &command.observation, &mut budget)?
            .is_some()
        {
            return Err(Error::ExperienceAlreadyWithdrawn);
        }
        let mut stored = StoredWithdrawal {
            schema_version: 1,
            tenant: tenant.into(),
            namespace: namespace.into(),
            receipt: ExperienceWithdrawalReceipt {
                withdrawal_id: uuid::Uuid::new_v4(),
                observation: command.observation.clone(),
                status: "withdrawn".into(),
                reason: command.reason.clone(),
                actor: actor.clone(),
                committed_at_millis: chrono::Utc::now().timestamp_millis(),
                receipt_digest: String::new(),
            },
            command,
        };
        stored.receipt.receipt_digest = stored.digest()?;
        let bytes = encode(&stored)?;
        let mut batch = WriteBatch::default();
        batch.put(
            target_key(tenant, namespace, &stored.command.observation)?,
            &bytes,
        );
        batch.put(key, &bytes);
        self.inner
            .db
            .write_opt(batch, &write_options())
            .map_err(storage_error)?;
        Ok(stored.receipt)
    }
    pub fn inspect_experience_withdrawal(
        &self,
        tenant: &str,
        namespace: &str,
        observation: ExperienceExportRef,
    ) -> Result<ExperienceWithdrawalInspection> {
        let _guard = self
            .inner
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Learning mutation lock poisoned".into()))?;
        let evaluated_at_millis = chrono::Utc::now().timestamp_millis();
        let mut budget = ExperienceReuseBudget::new(32, 2_097_152);
        self.verify_observation_with_budget(tenant, namespace, &observation, &mut budget)?;
        let receipt = self.withdrawal_with_budget(tenant, namespace, &observation, &mut budget)?;
        Ok(ExperienceWithdrawalInspection {
            evaluated_at_millis,
            observation,
            status: if receipt.is_some() {
                "withdrawn"
            } else {
                "not_withdrawn"
            }
            .into(),
            receipt,
        })
    }
}
