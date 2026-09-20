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
        if let Some(existing) = self.strategy_candidate(tenant, namespace, &request.id)? {
            return if existing.request == request {
                Ok(existing)
            } else {
                Err(Error::ConstraintViolation(
                    "Strategy candidates are immutable; use a new ID".into(),
                ))
            };
        }
        if self
            .registered_procedure(tenant, namespace, &request.id)?
            .is_some()
        {
            return Err(Error::ConstraintViolation(
                "The procedure ID already belongs to another proposal".into(),
            ));
        }
        let export = self.export_experiences(tenant, namespace, request.selection.clone())?;
        let proposal =
            self.prepare_registered_proposal(tenant, namespace, request.proposal(&export)?, actor)?;
        if export.context_digest != proposal.context_digest {
            return Err(Error::ConstraintViolation(
                "Strategy observations must match the registered context".into(),
            ));
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
        let mut batch = WriteBatch::default();
        Self::put_registered_proposal(&mut batch, &receipt.proposal)?;
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

#[cfg(test)]
mod tests;
