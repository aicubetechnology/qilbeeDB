//! Research-informed strategy candidates bound atomically to exact development observations.
use super::{bound::*, experience_export::*, *};
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StrategyExtractor {
    pub provider: String,
    pub model: String,
    pub model_revision: String,
    pub prompt_revision: String,
    pub evidence_ref: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StrategyCandidateRequest {
    pub id: String,
    pub policy_id: String,
    pub context_id: String,
    pub instructions: String,
    pub preconditions: Vec<String>,
    pub counterexamples: Vec<String>,
    pub extractor: StrategyExtractor,
    pub selection: ExperienceExportRequest,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StrategyCandidateReceipt {
    pub schema_version: u32,
    pub method_version: String,
    pub request: StrategyCandidateRequest,
    pub export_digest: String,
    pub summary: ExperienceExportSummary,
    pub proposal: ProposalReceipt,
    pub strategy_digest: String,
}
fn key(tenant: &str, namespace: &str, id: &str) -> Result<Vec<u8>> {
    super::tools::tool_key(15, tenant, namespace, id)
}
impl StrategyCandidateRequest {
    fn validate(&self) -> Result<()> {
        for value in [
            &self.id,
            &self.policy_id,
            &self.context_id,
            &self.extractor.provider,
            &self.extractor.model,
            &self.extractor.model_revision,
            &self.extractor.prompt_revision,
        ] {
            validate_text(value, "strategy identity", 512)?;
        }
        validate_text(&self.instructions, "strategy instructions", 16_384)?;
        validate_text(&self.extractor.evidence_ref, "extractor evidence", 2048)?;
        if self.preconditions.is_empty()
            || self.preconditions.len() > 16
            || self.counterexamples.len() > 16
        {
            return Err(Error::ValidationError(
                "A strategy requires 1-16 preconditions and at most 16 counterexamples".into(),
            ));
        }
        for text in self.preconditions.iter().chain(&self.counterexamples) {
            validate_text(text, "strategy condition", 1024)?;
        }
        if self.selection.events.is_empty() || self.selection.events.len() > 16 {
            return Err(Error::ValidationError(
                "A strategy requires 1-16 distinct development attempts".into(),
            ));
        }
        Ok(())
    }
    fn proposal(&self, export: &ExperienceExport) -> Result<RegisteredProposal> {
        // Structured applicability is part of the exact instructions qualified and selected.
        let instructions = serde_json::to_string(&serde_json::json!({
            "format":"qilbee.strategy-instructions.v1", "instructions":self.instructions,
            "preconditions":self.preconditions, "counterexamples":self.counterexamples,
        }))
        .map_err(|e| Error::Serialization(e.to_string()))?;
        let mut refs = std::collections::BTreeSet::from([
            format!("qilbee:strategy-request:sha256:{}", registry::digest(self)?),
            self.extractor.evidence_ref.clone(),
        ]);
        for event in &export.events {
            refs.insert(event.command.evidence.reference.clone());
            refs.insert(event.record.receipt.request.input.reference.clone());
        }
        Ok(RegisteredProposal {
            id: self.id.clone(),
            policy_id: self.policy_id.clone(),
            context_id: self.context_id.clone(),
            instructions,
            source_refs: refs.into_iter().collect(),
        })
    }
}
impl StrategyCandidateReceipt {
    fn digest(&self) -> Result<String> {
        registry::digest(&(
            &self.schema_version,
            &self.method_version,
            &self.request,
            &self.export_digest,
            &self.summary,
            &self.proposal,
        ))
    }
}
impl LearningMemory {
    /// Derivation evidence is not qualification evidence. No trial is admitted by this write.
    pub fn propose_strategy(
        &self,
        tenant: &str,
        namespace: &str,
        request: StrategyCandidateRequest,
        actor: &str,
    ) -> Result<StrategyCandidateReceipt> {
        request.validate()?;
        validate_text(actor, "strategy author", 512)?;
        let _guard = self
            .inner
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Learning mutation lock poisoned".into()))?;
        let mut budget = experience_withdrawal::ExperienceReuseBudget::new(4096, 33_554_432);
        if let Some(existing) =
            self.strategy_candidate_budgeted(tenant, namespace, &request.id, &mut budget)?
        {
            return if existing.request == request {
                Ok(existing)
            } else {
                Err(Error::ConstraintViolation(
                    "Strategy candidates are immutable; use a new ID".into(),
                ))
            };
        }
        if self
            .registered_procedure_budgeted(tenant, namespace, &request.id, &mut budget)?
            .is_some()
        {
            return Err(Error::ConstraintViolation(
                "The procedure ID already belongs to another proposal".into(),
            ));
        }
        let export =
            self.strategy_export_budgeted(tenant, namespace, &request.selection, &mut budget)?;
        let proposal = self.prepare_registered_proposal_budgeted(
            tenant,
            namespace,
            request.proposal(&export)?,
            actor,
            &mut budget,
        )?;
        if export.context_digest != proposal.context_digest {
            return Err(Error::ConstraintViolation(
                "Strategy observations must match the registered context".into(),
            ));
        }
        for reference in &request.selection.events {
            if self
                .withdrawal_with_budget(tenant, namespace, reference, &mut budget)?
                .is_some()
            {
                return Err(Error::ExperienceEvidenceWithdrawn);
            }
        }
        let mut receipt = StrategyCandidateReceipt {
            schema_version: 1,
            method_version: "qilbee.experience-strategy.v1".into(),
            request,
            export_digest: export.export_digest,
            summary: export.summary,
            proposal,
            strategy_digest: String::new(),
        };
        receipt.strategy_digest = receipt.digest()?;
        if budget
            .read::<StrategyLocator>(
                self,
                locator_key(&receipt.proposal.record.scope, &receipt.request.id),
            )?
            .is_some()
        {
            return Err(Error::DataCorruption(
                "Strategy locator exists before admission".into(),
            ));
        }
        let mut batch = WriteBatch::default();
        Self::put_registered_proposal(&mut batch, &receipt.proposal)?;
        batch.put(
            locator_key(&receipt.proposal.record.scope, &receipt.request.id),
            encode(&receipt.locator())?,
        );
        batch.put(
            key(tenant, namespace, &receipt.request.id)?,
            encode(&receipt)?,
        );
        self.inner
            .db
            .write_opt(batch, &write_options())
            .map_err(storage_error)?;
        Ok(receipt)
    }
    /// Returns original candidate evidence, even after later qualification or observations.
    pub fn strategy_candidate(
        &self,
        tenant: &str,
        namespace: &str,
        id: &str,
    ) -> Result<Option<StrategyCandidateReceipt>> {
        let Some(bytes) = self
            .inner
            .db
            .get(key(tenant, namespace, id)?)
            .map_err(storage_error)?
        else {
            return Ok(None);
        };
        let receipt: StrategyCandidateReceipt = decode(&bytes)?;
        let invalid = || Error::DataCorruption("Strategy candidate binding is inconsistent".into());
        if receipt.schema_version != 1
            || receipt.method_version != "qilbee.experience-strategy.v1"
            || receipt.request.id != id
            || receipt.strategy_digest != receipt.digest()?
            || receipt.request.validate().is_err()
        {
            return Err(invalid());
        }
        let proposal = self
            .registered_procedure(tenant, namespace, id)?
            .ok_or_else(invalid)?;
        let export = self
            .export_experiences(tenant, namespace, receipt.request.selection.clone())
            .map_err(|_| invalid())?;
        if receipt.proposal != proposal.receipt
            || receipt.request.proposal(&export)? != proposal.receipt.request
            || receipt.export_digest != export.export_digest
            || receipt.summary != export.summary
            || receipt.proposal.context_digest != export.context_digest
        {
            return Err(invalid());
        }
        Ok(Some(receipt))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StrategyReuseInspection {
    pub strategy_id: String,
    pub evaluated_at_millis: i64,
    pub qualification_active: bool,
    pub eligible_for_knowledge_reuse: bool,
    pub reason: String,
    pub coverage: StrategyReuseCoverage,
    pub work: experience_withdrawal::ExperienceReuseWork,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StrategyReuseCoverage {
    pub complete: bool,
}
impl LearningMemory {
    pub(super) fn strategy_export_budgeted(
        &self,
        tenant: &str,
        namespace: &str,
        selection: &ExperienceExportRequest,
        budget: &mut experience_withdrawal::ExperienceReuseBudget,
    ) -> Result<ExperienceExport> {
        experience::validate_digest(&selection.context_digest)?;
        validate_text(&selection.accounting_unit, "accounting unit", 512)?;
        if selection.events.is_empty() || selection.events.len() > 64 {
            return Err(Error::ValidationError("Invalid export size".into()));
        }
        let mut seen = std::collections::BTreeSet::new();
        let mut events = Vec::new();
        for reference in &selection.events {
            if !seen.insert(&reference.attempt_id) {
                return Err(Error::ValidationError("Duplicate export attempt".into()));
            }
            let event =
                self.verify_observation_with_budget(tenant, namespace, reference, budget)?;
            if event.record.receipt.context_digest != selection.context_digest
                || event.record.receipt.request.accounting_unit != selection.accounting_unit
            {
                return Err(Error::ConstraintViolation(
                    "Export context or accounting mismatch".into(),
                ));
            }
            events.push(event);
        }
        Self::export_from_verified_events(
            selection.context_digest.clone(),
            selection.accounting_unit.clone(),
            events,
        )
    }
    pub(super) fn strategy_candidate_budgeted(
        &self,
        tenant: &str,
        namespace: &str,
        id: &str,
        budget: &mut experience_withdrawal::ExperienceReuseBudget,
    ) -> Result<Option<StrategyCandidateReceipt>> {
        let Some(receipt): Option<StrategyCandidateReceipt> =
            budget.read(self, key(tenant, namespace, id)?)?
        else {
            return Ok(None);
        };
        let invalid = || Error::DataCorruption("Strategy candidate binding is inconsistent".into());
        if receipt.schema_version != 1
            || receipt.method_version != "qilbee.experience-strategy.v1"
            || receipt.request.id != id
            || receipt.strategy_digest != receipt.digest()?
            || receipt.request.validate().is_err()
        {
            return Err(invalid());
        }
        let proposal = self
            .registered_procedure_budgeted(tenant, namespace, id, budget)?
            .ok_or_else(invalid)?;
        let export =
            self.strategy_export_budgeted(tenant, namespace, &receipt.request.selection, budget)?;
        if receipt.proposal != proposal.receipt
            || receipt.request.proposal(&export)? != proposal.receipt.request
            || receipt.export_digest != export.export_digest
            || receipt.summary != export.summary
            || receipt.proposal.context_digest != export.context_digest
        {
            return Err(invalid());
        }
        Ok(Some(receipt))
    }
    pub(super) fn strategy_reusable_budgeted(
        &self,
        tenant: &str,
        namespace: &str,
        id: &str,
        budget: &mut experience_withdrawal::ExperienceReuseBudget,
    ) -> Result<bool> {
        let Some(receipt) = self.strategy_candidate_budgeted(tenant, namespace, id, budget)? else {
            return Ok(true);
        };
        let mut withdrawn = false;
        for reference in &receipt.request.selection.events {
            withdrawn |= self
                .withdrawal_with_budget(tenant, namespace, reference, budget)?
                .is_some();
        }
        Ok(!withdrawn)
    }
    pub fn inspect_strategy_reuse(
        &self,
        tenant: &str,
        namespace: &str,
        id: &str,
    ) -> Result<StrategyReuseInspection> {
        validate_text(id, "strategy ID", 512)?;
        if id.chars().any(char::is_control) {
            return Err(Error::ValidationError("Invalid strategy ID".into()));
        }
        let _guard = self
            .inner
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Learning mutation lock poisoned".into()))?;
        let evaluated_at_millis = chrono::Utc::now().timestamp_millis();
        let mut budget = experience_withdrawal::ExperienceReuseBudget::new(256, 8_388_608);
        let receipt = self
            .strategy_candidate_budgeted(tenant, namespace, id, &mut budget)?
            .ok_or_else(|| Error::KeyNotFound("Strategy".into()))?;
        let procedure = self
            .registered_procedure_budgeted(tenant, namespace, id, &mut budget)?
            .ok_or_else(|| Error::DataCorruption("Strategy procedure missing".into()))?;
        let qualification_active = procedure.record.state == ProcedureState::Active;
        let mut withdrawn = false;
        for reference in &receipt.request.selection.events {
            withdrawn |= self
                .withdrawal_with_budget(tenant, namespace, reference, &mut budget)?
                .is_some();
        }
        Ok(StrategyReuseInspection {
            strategy_id: id.into(),
            evaluated_at_millis,
            qualification_active,
            eligible_for_knowledge_reuse: qualification_active && !withdrawn,
            reason: if !qualification_active {
                "qualification_inactive"
            } else if withdrawn {
                "experience_evidence_withdrawn"
            } else {
                "eligible"
            }
            .into(),
            coverage: StrategyReuseCoverage { complete: true },
            work: budget.work,
        })
    }
}

#[cfg(test)]
mod tests;

pub(super) const STRATEGY_LOCATOR_MARKER: &[u8] = b"\0experience-strategy-locator-v1";
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StrategyLocator {
    pub schema_version: u32,
    pub tenant: String,
    pub namespace: String,
    pub strategy_id: String,
    pub scope: LearningScope,
    pub strategy_digest: String,
}
pub(super) fn locator_key(scope: &LearningScope, id: &str) -> Vec<u8> {
    let mut key = scope_prefix(26, scope);
    append_component(&mut key, id);
    key
}
impl StrategyCandidateReceipt {
    pub(super) fn locator(&self) -> StrategyLocator {
        StrategyLocator {
            schema_version: 1,
            tenant: self.proposal.tenant.clone(),
            namespace: self.proposal.namespace.clone(),
            strategy_id: self.request.id.clone(),
            scope: self.proposal.record.scope.clone(),
            strategy_digest: self.strategy_digest.clone(),
        }
    }
}
impl LearningMemory {
    pub(super) fn strategy_reusable_for_record(
        &self,
        record: &ProcedureRecord,
        budget: &mut experience_withdrawal::ExperienceReuseBudget,
    ) -> Result<bool> {
        let locator: Option<StrategyLocator> =
            budget.read(self, locator_key(&record.scope, &record.proposal.id))?;
        match locator {
            Some(locator) => {
                if locator.schema_version != 1
                    || locator.scope != record.scope
                    || locator.tenant != record.scope.tenant
                    || locator.strategy_id != record.proposal.id
                {
                    return Err(Error::DataCorruption("Invalid strategy locator".into()));
                }
                let receipt = self
                    .strategy_candidate_budgeted(
                        &locator.tenant,
                        &locator.namespace,
                        &locator.strategy_id,
                        budget,
                    )?
                    .ok_or_else(|| {
                        Error::DataCorruption("Strategy locator target missing".into())
                    })?;
                if receipt.locator() != locator
                    || receipt.proposal.record.proposal != record.proposal
                {
                    return Err(Error::DataCorruption(
                        "Strategy locator binding differs".into(),
                    ));
                }
                let mut withdrawn = false;
                for reference in &receipt.request.selection.events {
                    withdrawn |= self
                        .withdrawal_with_budget(
                            &locator.tenant,
                            &locator.namespace,
                            reference,
                            budget,
                        )?
                        .is_some();
                }
                Ok(!withdrawn)
            }
            None => {
                if record
                    .proposal
                    .source_refs
                    .iter()
                    .any(|s| s.starts_with("qilbee:strategy-request:sha256:"))
                {
                    return Err(Error::DataCorruption("Strategy locator missing".into()));
                }
                Ok(true)
            }
        }
    }
    /// One crash-resumable maintenance pass. No other/older writer may run between passes.
    /// Callers must retain the maintenance window until the completion marker exists.
    pub fn rebuild_strategy_locators(&self) -> Result<()> {
        let _guard = self
            .inner
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Learning mutation lock poisoned".into()))?;
        let mut budget =
            experience_withdrawal::ExperienceReuseBudget::new(4_000_000, 1_073_741_824);
        let marker: Option<u32> = budget.read(self, STRATEGY_LOCATOR_MARKER.to_vec())?;
        let progress: Option<StrategyLocatorProgress> =
            budget.read(self, STRATEGY_LOCATOR_PROGRESS.to_vec())?;
        if let Some(version) = marker {
            if version != 1 || progress.is_some() {
                return Err(Error::DataCorruption(
                    "Inconsistent strategy locator completion".into(),
                ));
            }
            return Ok(());
        }
        let mut completed = 0u64;
        let mut rolling = registry::digest(&"qilbee.strategy-locator-migration.v1")?;
        let mut iterator = self.inner.db.raw_iterator();
        if let Some(progress) = progress {
            if progress.schema_version != 1
                || progress.completed_receipts == 0
                || progress.last_key.first() != Some(&15)
            {
                return Err(Error::DataCorruption(
                    "Invalid strategy locator checkpoint".into(),
                ));
            }
            experience::validate_digest(&progress.rolling_digest)
                .map_err(|_| Error::DataCorruption("Invalid migration digest".into()))?;
            let raw: StrategyCandidateReceipt = budget
                .read(self, progress.last_key.clone())?
                .ok_or_else(|| Error::DataCorruption("Migration cursor receipt missing".into()))?;
            let receipt = budget
                .with_limits(4096, 33_554_432, |budget| {
                    self.strategy_candidate_budgeted(
                        &raw.proposal.tenant,
                        &raw.proposal.namespace,
                        &raw.request.id,
                        budget,
                    )
                })?
                .ok_or_else(|| {
                    Error::DataCorruption("Migration cursor receipt disappeared".into())
                })?;
            let expected = receipt.locator();
            let actual: StrategyLocator = budget
                .read(self, locator_key(&expected.scope, &expected.strategy_id))?
                .ok_or_else(|| Error::DataCorruption("Migration cursor locator missing".into()))?;
            if progress.last_key
                != key(
                    &receipt.proposal.tenant,
                    &receipt.proposal.namespace,
                    &receipt.request.id,
                )?
                || progress.receipt_digest != registry::digest(&receipt)?
                || actual != expected
                || progress.locator_digest != registry::digest(&actual)?
            {
                return Err(Error::DataCorruption(
                    "Migration cursor binding changed".into(),
                ));
            }
            completed = progress.completed_receipts;
            rolling = progress.rolling_digest;
            iterator.seek(&progress.last_key);
            if iterator.key() != Some(progress.last_key.as_slice()) {
                return Err(Error::DataCorruption(
                    "Migration cursor key disappeared".into(),
                ));
            }
            iterator.next();
        } else {
            iterator.seek([15]);
        }
        let mut pass_count = 0usize;
        while iterator.valid() {
            let key_bytes = iterator
                .key()
                .ok_or_else(|| Error::Storage("Missing migration key".into()))?
                .to_vec();
            if key_bytes.first() != Some(&15) {
                break;
            }
            if pass_count >= 100_000 {
                return Err(Error::ExperienceReuseLimitExceeded);
            }
            budget.charge_record()?;
            let bytes = iterator
                .value()
                .ok_or_else(|| Error::Storage("Missing migration value".into()))?;
            budget.charge_bytes(bytes.len())?;
            if bytes.len() > 33_554_432 {
                return Err(Error::ExperienceReuseLimitExceeded);
            }
            let raw: StrategyCandidateReceipt = decode(bytes)?;
            if key_bytes
                != key(
                    &raw.proposal.tenant,
                    &raw.proposal.namespace,
                    &raw.request.id,
                )?
            {
                return Err(Error::DataCorruption("Strategy receipt key differs".into()));
            }
            let receipt = budget
                .with_limits(4096, 33_554_432, |budget| {
                    self.strategy_candidate_budgeted(
                        &raw.proposal.tenant,
                        &raw.proposal.namespace,
                        &raw.request.id,
                        budget,
                    )
                })?
                .ok_or_else(|| {
                    Error::DataCorruption("Strategy disappeared during migration".into())
                })?;
            let locator = receipt.locator();
            let locator_key = locator_key(&locator.scope, &locator.strategy_id);
            let existing: Option<StrategyLocator> = budget.read(self, locator_key.clone())?;
            if existing.is_some_and(|value| value != locator) {
                return Err(Error::DataCorruption("Conflicting strategy locator".into()));
            }
            completed = completed
                .checked_add(1)
                .ok_or_else(|| Error::DataCorruption("Migration receipt count overflow".into()))?;
            let receipt_digest = registry::digest(&receipt)?;
            let locator_digest = registry::digest(&locator)?;
            rolling = registry::digest(&(
                "qilbee.strategy-locator-step.v1",
                &rolling,
                &key_bytes,
                &receipt_digest,
                &locator_digest,
                completed,
            ))?;
            let progress = StrategyLocatorProgress {
                schema_version: 1,
                last_key: key_bytes,
                receipt_digest,
                locator_digest,
                rolling_digest: rolling.clone(),
                completed_receipts: completed,
            };
            let mut batch = WriteBatch::default();
            batch.put(locator_key, encode(&locator)?);
            batch.put(STRATEGY_LOCATOR_PROGRESS, encode(&progress)?);
            self.inner
                .db
                .write_opt(batch, &write_options())
                .map_err(storage_error)?;
            pass_count += 1;
            iterator.next();
        }
        iterator.status().map_err(storage_error)?;
        let mut batch = WriteBatch::default();
        batch.put(STRATEGY_LOCATOR_MARKER, encode(&1u32)?);
        batch.delete(STRATEGY_LOCATOR_PROGRESS);
        self.inner
            .db
            .write_opt(batch, &write_options())
            .map_err(storage_error)?;
        Ok(())
    }
}

pub(super) const STRATEGY_LOCATOR_PROGRESS: &[u8] = b"\0experience-strategy-locator-progress-v1";
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StrategyLocatorProgress {
    pub schema_version: u32,
    pub last_key: Vec<u8>,
    pub receipt_digest: String,
    pub locator_digest: String,
    pub rolling_digest: String,
    pub completed_receipts: u64,
}
