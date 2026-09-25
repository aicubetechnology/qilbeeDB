//! Knowledge derived by an external agent from exact experiences and memories.
//! Admission does not qualify a candidate or authorize tool execution.
use super::{
    experience::validate_digest,
    experience_export::ExperienceExportRequest,
    knowledge::{ExternalToolKnowledge, KnowledgeProposal},
    strategies::StrategyExtractor,
    *,
};
use crate::storage::platform::MemorySourceRef;
use serde::Deserialize;

/// Separate input shape preserves legacy knowledge's explicit-null contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OriginToolInput {
    pub name: String,
    pub schema_revision: String,
    #[serde(default)]
    pub implementation_revision: Option<String>,
    #[serde(default)]
    pub environment_revision: Option<String>,
    pub usage_contract: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OriginKnowledgeInput {
    pub id: String,
    pub policy_id: String,
    pub context_id: String,
    pub title: String,
    pub instructions: String,
    pub memory_sources: Vec<MemorySourceRef>,
    pub external_tools: Vec<OriginToolInput>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeInference {
    pub extractor: StrategyExtractor,
    pub preconditions: Vec<String>,
    pub counterexamples: Vec<String>,
}
impl KnowledgeInference {
    fn validate(&self) -> Result<()> {
        for value in [
            &self.extractor.provider,
            &self.extractor.model,
            &self.extractor.model_revision,
            &self.extractor.prompt_revision,
        ] {
            validate_text(value, "knowledge extractor identity", 512)?;
        }
        validate_text(
            &self.extractor.evidence_ref,
            "knowledge extractor evidence",
            2048,
        )?;
        if self.preconditions.is_empty()
            || self.preconditions.len() > 16
            || self.counterexamples.len() > 16
        {
            return Err(Error::ValidationError(
                "Knowledge inference requires 1-16 preconditions and at most 16 counterexamples"
                    .into(),
            ));
        }
        for text in self.preconditions.iter().chain(&self.counterexamples) {
            validate_text(text, "knowledge inference condition", 1024)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CombinedKnowledgeInput {
    pub knowledge: OriginKnowledgeInput,
    pub experience_selection: ExperienceExportRequest,
    pub expected_export_digest: String,
    pub inference: KnowledgeInference,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CombinedKnowledgeProposal {
    pub knowledge: KnowledgeProposal,
    pub experience_selection: ExperienceExportRequest,
    pub expected_export_digest: String,
    pub inference: KnowledgeInference,
}

impl CombinedKnowledgeInput {
    pub fn canonicalize(self) -> Result<CombinedKnowledgeProposal> {
        let input = self.knowledge;
        CombinedKnowledgeProposal {
            knowledge: KnowledgeProposal {
                id: input.id,
                policy_id: input.policy_id,
                context_id: input.context_id,
                title: input.title,
                instructions: input.instructions,
                memory_sources: input.memory_sources,
                external_tools: input
                    .external_tools
                    .into_iter()
                    .map(|tool| ExternalToolKnowledge {
                        name: tool.name,
                        schema_revision: tool.schema_revision,
                        implementation_revision: tool.implementation_revision,
                        environment_revision: tool.environment_revision,
                        usage_contract: tool.usage_contract,
                    })
                    .collect(),
            },
            experience_selection: self.experience_selection,
            expected_export_digest: self.expected_export_digest,
            inference: self.inference,
        }
        .canonicalize()
    }
}

impl CombinedKnowledgeProposal {
    /// Canonicalizes sets only. Condition order and Unicode bytes are preserved.
    /// Persisted proposals must also pass exact canonical equality on readback.
    pub fn canonicalize(mut self) -> Result<Self> {
        self.knowledge = self.knowledge.canonicalize()?;
        self.inference.validate()?;
        validate_digest(&self.expected_export_digest)?;
        validate_digest(&self.experience_selection.context_digest)?;
        validate_text(
            &self.experience_selection.accounting_unit,
            "accounting unit",
            512,
        )?;
        let events = &mut self.experience_selection.events;
        if !(1..=16).contains(&events.len()) {
            return Err(Error::ValidationError(
                "Combined knowledge requires 1-16 distinct attempts".into(),
            ));
        }
        for event in events.iter() {
            validate_text(&event.attempt_id, "experience attempt ID", 512)?;
            validate_text(&event.event_id, "experience event ID", 512)?;
            validate_digest(&event.event_digest)?;
        }
        events.sort_by(|left, right| left.attempt_id.as_bytes().cmp(right.attempt_id.as_bytes()));
        if events
            .windows(2)
            .any(|pair| pair[0].attempt_id == pair[1].attempt_id)
        {
            return Err(Error::ValidationError(
                "An attempt may occur only once in knowledge origin".into(),
            ));
        }
        Ok(self)
    }
}

/// Server-resolved immutable origin. Tenant and namespace come from authorization.
/// This is not an HTTP admission input and does not confer qualification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct KnowledgeOriginPayload {
    pub schema_version: u32,
    pub method_version: String,
    pub tenant: String,
    pub namespace: String,
    pub request: CombinedKnowledgeProposal,
    pub export_digest: String,
    pub summary: super::experience_export::ExperienceExportSummary,
}

impl KnowledgeOriginPayload {
    pub(super) fn digest(&self) -> Result<String> {
        super::knowledge_origin_hash::digest(
            super::knowledge_origin_hash::OriginHashDomain::Origin,
            self,
        )
    }
}

impl LearningMemory {
    /// Resolve exact immutable events through the existing integrity-checking
    /// reader. The caller must commit this origin with its linked procedure and
    /// receipt under the learning mutation lock; this function writes nothing.
    pub(super) fn resolve_knowledge_origin(
        &self,
        tenant: &str,
        namespace: &str,
        request: CombinedKnowledgeProposal,
    ) -> Result<KnowledgeOriginPayload> {
        validate_text(tenant, "tenant", 512)?;
        validate_text(namespace, "authorized namespace", 4096)?;
        let request = request.canonicalize()?;
        let export =
            self.export_experiences(tenant, namespace, request.experience_selection.clone())?;
        if export.export_digest != request.expected_export_digest {
            return Err(Error::ConstraintViolation(
                "Knowledge origin export differs from the expected immutable cohort".into(),
            ));
        }
        Ok(KnowledgeOriginPayload {
            schema_version: 1,
            method_version: "qilbee.experience-knowledge.v1".into(),
            tenant: tenant.into(),
            namespace: namespace.into(),
            request,
            export_digest: export.export_digest,
            summary: export.summary,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CombinedKnowledgeReceipt {
    pub schema_version: u32,
    pub method_version: String,
    pub tenant: String,
    pub namespace: String,
    pub request: CombinedKnowledgeProposal,
    pub export_digest: String,
    pub summary: super::experience_export::ExperienceExportSummary,
    pub origin_digest: String,
    pub knowledge_receipt: super::knowledge::KnowledgeReceipt,
    pub receipt_digest: String,
}

/// The new receipt links the unchanged legacy digest, not its float-bearing JSON.
#[derive(Serialize)]
struct ReceiptHashPayload<'a> {
    schema_version: u32,
    method_version: &'a str,
    tenant: &'a str,
    namespace: &'a str,
    origin_digest: &'a str,
    knowledge_receipt_digest: &'a str,
}

impl CombinedKnowledgeReceipt {
    pub(super) fn digest(&self) -> Result<String> {
        super::knowledge_origin_hash::digest(
            super::knowledge_origin_hash::OriginHashDomain::Receipt,
            &ReceiptHashPayload {
                schema_version: self.schema_version,
                method_version: &self.method_version,
                tenant: &self.tenant,
                namespace: &self.namespace,
                origin_digest: &self.origin_digest,
                knowledge_receipt_digest: &self.knowledge_receipt.receipt_digest,
            },
        )
    }

    /// Checks immutable linked contents only. Live memory eligibility, registered
    /// procedure binding and caller permissions still require their own checks.
    pub(super) fn validate_origin_link(
        &self,
        tenant: &str,
        namespace: &str,
        id: &str,
        resolved: &KnowledgeOriginPayload,
    ) -> Result<()> {
        let knowledge = &self.knowledge_receipt;
        if self.schema_version != 1
            || self.method_version != "qilbee.experience-knowledge.v1"
            || self.tenant != tenant
            || self.namespace != namespace
            || self.request.knowledge.id != id
            || resolved.schema_version != self.schema_version
            || resolved.method_version != self.method_version
            || resolved.tenant != self.tenant
            || resolved.namespace != self.namespace
            || resolved.request != self.request
            || resolved.export_digest != self.export_digest
            || resolved.summary != self.summary
            || self.origin_digest != resolved.digest()?
            || knowledge.schema_version != 2
            || knowledge.tenant != tenant
            || knowledge.namespace != namespace
            || knowledge.context_digest != self.request.experience_selection.context_digest
            || knowledge.request != self.request.knowledge
            || knowledge.receipt_digest != knowledge.digest()?
            || self.receipt_digest != self.digest()?
        {
            return Err(Error::DataCorruption(
                "Combined knowledge origin integrity mismatch".into(),
            ));
        }
        let request_digest = super::registry::digest(&knowledge.request)?;
        let expected = vec![
            format!("qilbee-knowledge-v2:{request_digest}"),
            format!("qilbee-experience-knowledge-v1:{}", self.origin_digest),
        ];
        if knowledge.record.proposal.source_refs != expected {
            return Err(Error::DataCorruption(
                "Combined knowledge source references mismatch".into(),
            ));
        }
        Ok(())
    }
}

impl LearningMemory {
    /// Preparation only: caller must reject existing identities, hold the writer
    /// lock and atomically persist the complete linked set with its projections.
    pub(super) fn prepare_combined_knowledge_receipt(
        &self, tenant: &str, namespace: &str, request: CombinedKnowledgeProposal,
        actor: &str, budget: &mut super::experience_withdrawal::ExperienceReuseBudget,
    ) -> Result<(super::bound::ProposalReceipt, CombinedKnowledgeReceipt)> {
        let export = self.strategy_export_budgeted(tenant, namespace, &request.experience_selection, budget)?;
        if export.export_digest != request.expected_export_digest {
            return Err(Error::ConstraintViolation("Combined export differs".into()));
        }
        if self.combined_withdrawn_budgeted(tenant, namespace, &request.experience_selection.events, budget)? {
            return Err(Error::ExperienceEvidenceWithdrawn);
        }
        let origin = KnowledgeOriginPayload { schema_version: 1, method_version: "qilbee.experience-knowledge.v1".into(),
            tenant: tenant.into(), namespace: namespace.into(), request, export_digest: export.export_digest, summary: export.summary };
        let origin_digest = origin.digest()?;
        let request = origin.request.knowledge.clone();
        let (_, context) = self.contracts_budgeted(tenant, namespace, &request.policy_id, &request.context_id, budget)?;
        for tool in &request.external_tools {
            if context.payload.tools.get(&tool.name) != Some(&tool.schema_revision)
                || tool.environment_revision.as_ref().is_some_and(|v| v != &context.payload.environment_revision) {
                return Err(Error::ValidationError("External tool identity differs from evaluation context".into()));
            }
        }
        let binding = self.prepare_registered_proposal_budgeted(tenant, namespace,
            super::bound::RegisteredProposal { id: request.id.clone(), policy_id: request.policy_id.clone(),
                context_id: request.context_id.clone(), instructions: request.instructions.clone(),
                source_refs: vec![format!("qilbee-knowledge-v2:{}", super::registry::digest(&request)?),
                    format!("qilbee-experience-knowledge-v1:{origin_digest}")] }, actor, budget)?;
        let mut knowledge_receipt = super::knowledge::KnowledgeReceipt {
            schema_version: 2, tenant: tenant.into(), namespace: namespace.into(), request,
            policy_digest: binding.policy_digest.clone(), context_digest: binding.context_digest.clone(),
            actor: actor.into(), record: binding.record.clone(), receipt_digest: String::new() };
        knowledge_receipt.receipt_digest = knowledge_receipt.digest()?;
        if knowledge_receipt.context_digest != origin.request.experience_selection.context_digest {
            return Err(Error::ConstraintViolation(
                "Knowledge and experience evidence must use the same registered context".into(),
            ));
        }
        let mut receipt = CombinedKnowledgeReceipt {
            schema_version: origin.schema_version,
            method_version: origin.method_version.clone(),
            tenant: origin.tenant.clone(),
            namespace: origin.namespace.clone(),
            request: origin.request.clone(),
            export_digest: origin.export_digest.clone(),
            summary: origin.summary.clone(),
            origin_digest,
            knowledge_receipt,
            receipt_digest: String::new(),
        };
        receipt.receipt_digest = receipt.digest()?;
        receipt.validate_origin_link(tenant, namespace, &receipt.request.knowledge.id, &origin)?;
        Ok((binding, receipt))
    }
}

pub(super) fn origin_key(tenant: &str, namespace: &str, id: &str) -> Result<Vec<u8>> {
    super::tools::tool_key(17, tenant, namespace, id)
}

impl LearningMemory {
    /// Read and verify the full protected receipt. HTTP callers must separately
    /// enforce both memory and experience access in the exact authorized scope.
    pub fn combined_knowledge_receipt(
        &self,
        tenant: &str,
        namespace: &str,
        id: &str,
    ) -> Result<Option<CombinedKnowledgeReceipt>> {
        let Some(bytes) = self
            .inner
            .db
            .get(origin_key(tenant, namespace, id)?)
            .map_err(storage_error)?
        else {
            if let Some(linked) = self.inner.db.get(super::knowledge::key(tenant, namespace, id)?).map_err(storage_error)? {
                let linked: super::knowledge::KnowledgeReceipt = decode(&linked)?;
                if has_origin_marker(&linked.record.proposal.source_refs) {
                    return Err(Error::DataCorruption("Mandatory knowledge origin is missing".into()));
                }
            }
            if let Some(procedure) = self.registered_procedure(tenant, namespace, id)? {
                if has_origin_marker(&procedure.record.proposal.source_refs) {
                    return Err(Error::DataCorruption("Mandatory procedure origin is missing".into()));
                }
            }
            return Ok(None);
        };
        let receipt: CombinedKnowledgeReceipt = decode(&bytes)?;
        let origin = self
            .resolve_knowledge_origin(tenant, namespace, receipt.request.clone())
            .map_err(|_| {
                Error::DataCorruption(
                    "Combined knowledge experience origin cannot be verified".into(),
                )
            })?;
        receipt.validate_origin_link(tenant, namespace, id, &origin)?;
        let bytes = self
            .inner
            .db
            .get(super::knowledge::key(tenant, namespace, id)?)
            .map_err(storage_error)?
            .ok_or_else(|| {
                Error::DataCorruption("Combined knowledge linked receipt is missing".into())
            })?;
        let linked: super::knowledge::KnowledgeReceipt = decode(&bytes)?;
        if linked != receipt.knowledge_receipt {
            return Err(Error::DataCorruption(
                "Combined knowledge linked receipt differs".into(),
            ));
        }
        let binding = self
            .registered_procedure(tenant, namespace, id)?
            .ok_or_else(|| {
                Error::DataCorruption("Combined knowledge procedure is missing".into())
            })?;
        let request_digest = super::registry::digest(&linked.request)?;
        Self::validate_knowledge_binding_sources(
            tenant,
            namespace,
            id,
            &linked,
            &binding,
            &[
                format!("qilbee-knowledge-v2:{request_digest}"),
                format!("qilbee-experience-knowledge-v1:{}", receipt.origin_digest),
            ],
        )?;
        Ok(Some(receipt))
    }
}

impl LearningMemory {
    /// Atomically admits a candidate and its exact immutable evidence linkage.
    /// The host must authorize proposal, memory and experience access first.
    pub fn propose_combined_knowledge(
        &self,
        memory: &crate::RocksDbMemoryStorage,
        tenant: &str,
        namespace: &str,
        request: CombinedKnowledgeInput,
        actor: &str,
    ) -> Result<CombinedKnowledgeReceipt> {
        let request = request.canonicalize()?;
        validate_text(actor, "knowledge actor", 512)?;
        let origin_key = origin_key(tenant, namespace, &request.knowledge.id)?;
        let knowledge_key = super::knowledge::key(tenant, namespace, &request.knowledge.id)?;
        let _guard = self
            .inner
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Learning mutation lock poisoned".into()))?;
        let mut budget = super::experience_withdrawal::ExperienceReuseBudget::new(4096, 32 * 1024 * 1024);
        if let Some(receipt) =
            self.combined_receipt_budgeted(tenant, namespace, &request.knowledge.id, &mut budget)?
        {
            return if receipt.request == request {
                Ok(receipt)
            } else {
                Err(Error::ConstraintViolation(
                    "Knowledge revisions are immutable; use a new ID".into(),
                ))
            };
        }
        let linked: Option<super::knowledge::KnowledgeReceipt> = budget.read(self, knowledge_key.clone())?;
        if linked.is_some() || self.registered_procedure_budgeted(tenant, namespace, &request.knowledge.id, &mut budget)?.is_some() {
            return Err(Error::ConstraintViolation("Existing knowledge cannot acquire origin binding".into()));
        }
        let evidence = self.combined_memory_budgeted(memory, namespace, &request.knowledge.memory_sources, &mut budget)?;
        if !evidence.eligible || !evidence.all_dependencies_checked {
            return Err(Error::ConstraintViolation("Combined memory evidence is not eligible".into()));
        }
        let (binding, receipt) = self.prepare_combined_knowledge_receipt(tenant, namespace, request, actor, &mut budget)?;
        let mut batch = WriteBatch::default();
        Self::put_registered_proposal(&mut batch, &binding)?;
        self.put_knowledge_locator(&mut batch, &receipt.knowledge_receipt)?;
        batch.put(knowledge_key, encode(&receipt.knowledge_receipt)?);
        batch.put(origin_key, encode(&receipt)?);
        self.inner
            .db
            .write_opt(batch, &write_options())
            .map_err(storage_error)?;
        Ok(receipt)
    }
}

pub(super) fn has_origin_marker(sources: &[String]) -> bool {
    sources
        .iter()
        .any(|source| source.starts_with("qilbee-experience-knowledge-v1:"))
}

impl LearningMemory {
    /// Internal verified receipt reader for maintenance across both projections.
    /// Public legacy routes must continue using their negotiated ordinary reader.
    pub(super) fn knowledge_receipt_any_origin(
        &self,
        tenant: &str,
        namespace: &str,
        id: &str,
    ) -> Result<Option<super::knowledge::KnowledgeReceipt>> {
        let bytes = self
            .inner
            .db
            .get(super::knowledge::key(tenant, namespace, id)?)
            .map_err(storage_error)?;
        let origin_exists = self
            .inner
            .db
            .get(origin_key(tenant, namespace, id)?)
            .map_err(storage_error)?
            .is_some();
        let Some(bytes) = bytes else {
            if origin_exists {
                return Err(Error::DataCorruption(
                    "Knowledge origin has no linked receipt".into(),
                ));
            }
            return Ok(None);
        };
        let receipt: super::knowledge::KnowledgeReceipt = decode(&bytes)?;
        if origin_exists || has_origin_marker(&receipt.record.proposal.source_refs) {
            return self
                .combined_knowledge_receipt(tenant, namespace, id)?
                .map(|combined| Some(combined.knowledge_receipt))
                .ok_or_else(|| {
                    Error::DataCorruption("Mandatory knowledge origin is missing".into())
                });
        }
        self.knowledge_receipt(tenant, namespace, id)
    }
}

/// Minimal verified origin; intentionally excludes experience IDs and accounting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum KnowledgeOriginDescriptor {
    MemoryOnly,
    ExperienceMemory {
        method_version: String,
        origin_digest: String,
    },
}

impl LearningMemory {
    /// Current source inspection with minimal origin disclosure. The caller must
    /// authorize memory access; full experience evidence remains a separate read.
    pub fn inspect_knowledge_with_origin(
        &self,
        memory: &crate::RocksDbMemoryStorage,
        tenant: &str,
        namespace: &str,
        id: &str,
    ) -> Result<
        Option<(
            super::knowledge::KnowledgeInspection,
            KnowledgeOriginDescriptor,
        )>,
    > {
        let _guard = self
            .inner
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Learning mutation lock poisoned".into()))?;
        let mut budget = super::experience_withdrawal::ExperienceReuseBudget::new(512, 16 * 1024 * 1024);
        let combined = self.combined_receipt_budgeted(tenant, namespace, id, &mut budget)?;
        let withdrawn = match &combined {
            Some(value) => self.combined_withdrawn_budgeted(tenant, namespace, &value.request.experience_selection.events, &mut budget)?,
            None => false,
        };
        let receipt = match combined {
            Some(value) => Some(value.knowledge_receipt),
            None => budget.read(self, super::knowledge::key(tenant, namespace, id)?)?,
        };
        let Some(receipt) = receipt else {
            return Ok(None);
        };
        let origin = if has_origin_marker(&receipt.record.proposal.source_refs) {
            // The any-origin reader verified both records and their exact source
            // references; expose only the authenticated digest reference.
            let reference = receipt
                .record
                .proposal
                .source_refs
                .get(1)
                .and_then(|s| s.strip_prefix("qilbee-experience-knowledge-v1:"))
                .ok_or_else(|| {
                    Error::DataCorruption("Mandatory origin reference is missing".into())
                })?;
            KnowledgeOriginDescriptor::ExperienceMemory {
                method_version: "qilbee.experience-knowledge.v1".into(),
                origin_digest: reference.into(),
            }
        } else {
            KnowledgeOriginDescriptor::MemoryOnly
        };
        let procedure = self
            .registered_procedure_budgeted(tenant, namespace, id, &mut budget)?
            .ok_or_else(|| Error::DataCorruption("Knowledge procedure is missing".into()))?;
        if matches!(origin, KnowledgeOriginDescriptor::MemoryOnly) {
            Self::validate_knowledge_binding(tenant, namespace, id, &receipt, &procedure)?;
        }
        let evidence =
            self.combined_memory_budgeted(memory, namespace, &receipt.request.memory_sources, &mut budget)?;
        let qualification_active = procedure.record.state == ProcedureState::Active;
        let eligible_for_knowledge_reuse =
            qualification_active && !withdrawn && evidence.eligible && evidence.all_dependencies_checked;
        Ok(Some((
            super::knowledge::KnowledgeInspection {
                receipt,
                procedure,
                qualification_active,
                evidence,
                eligible_for_knowledge_reuse,
            },
            origin,
        )))
    }
}

/// Only call after validating the authoritative receipt and all mandatory links.
pub(super) fn descriptor_from_verified_receipt(
    receipt: &super::knowledge::KnowledgeReceipt,
) -> Result<KnowledgeOriginDescriptor> {
    if !has_origin_marker(&receipt.record.proposal.source_refs) {
        return Ok(KnowledgeOriginDescriptor::MemoryOnly);
    }
    let digest = receipt
        .record
        .proposal
        .source_refs
        .get(1)
        .and_then(|value| value.strip_prefix("qilbee-experience-knowledge-v1:"))
        .ok_or_else(|| Error::DataCorruption("Mandatory origin reference is missing".into()))?;
    Ok(KnowledgeOriginDescriptor::ExperienceMemory {
        method_version: "qilbee.experience-knowledge.v1".into(),
        origin_digest: digest.into(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OriginBudgetStop {
    Records,
    Bytes,
}

pub(super) struct OriginReadBudget {
    pub records_examined: usize,
    pub bytes_examined: usize,
    pub lookahead_bytes: usize,
    pub stop: Option<OriginBudgetStop>,
    record_limit: usize,
    byte_limit: usize,
}
impl OriginReadBudget {
    pub fn new(record_limit: usize, byte_limit: usize) -> Self {
        Self {
            records_examined: 0,
            bytes_examined: 0,
            lookahead_bytes: 0,
            stop: None,
            record_limit,
            byte_limit,
        }
    }
    pub fn read_optional<T: serde::de::DeserializeOwned>(
        &mut self,
        store: &LearningMemory,
        key: Vec<u8>,
    ) -> Result<Option<T>> {
        if self.stop.is_some() {
            return Ok(None);
        }
        if self.records_examined >= self.record_limit {
            self.stop = Some(OriginBudgetStop::Records);
            return Ok(None);
        }
        self.records_examined += 1;
        let Some(bytes) = store.inner.db.get(key).map_err(storage_error)? else {
            return Ok(None);
        };
        if bytes.len() > self.byte_limit.saturating_sub(self.bytes_examined) {
            self.lookahead_bytes = bytes.len();
            self.stop = Some(OriginBudgetStop::Bytes);
            return Ok(None);
        }
        self.bytes_examined += bytes.len();
        decode(&bytes).map(Some)
    }
    pub fn read_record<T: serde::de::DeserializeOwned>(
        &mut self,
        store: &LearningMemory,
        key: Vec<u8>,
    ) -> Result<Option<T>> {
        let value = self.read_optional(store, key)?;
        if value.is_none() && self.stop.is_none() {
            return Err(Error::DataCorruption(
                "Mandatory origin evidence record is missing".into(),
            ));
        }
        Ok(value)
    }
}

impl LearningMemory {
    pub(super) fn validate_combined_origin_bounded(
        &self,
        tenant: &str,
        namespace: &str,
        id: &str,
        linked: &super::knowledge::KnowledgeReceipt,
        procedure: &super::bound::RegisteredProcedure,
        context: &super::registry::RegistryEntry<super::registry::EvaluationContext>,
        budget: &mut OriginReadBudget,
    ) -> Result<Option<(KnowledgeOriginDescriptor, bool)>> {
        use super::experience::{ExperienceEvent, ExperienceRecord};
        let Some(receipt): Option<CombinedKnowledgeReceipt> =
            budget.read_record(self, origin_key(tenant, namespace, id)?)?
        else {
            return Ok(None);
        };
        if &receipt.knowledge_receipt != linked {
            return Err(Error::DataCorruption(
                "Combined linked knowledge differs".into(),
            ));
        }
        let request = receipt
            .request
            .clone()
            .canonicalize()
            .map_err(|_| Error::DataCorruption("Invalid combined origin request".into()))?;
        if request != receipt.request {
            return Err(Error::DataCorruption("Noncanonical combined origin".into()));
        }
        let mut events = Vec::new();
        for reference in &request.experience_selection.events {
            let Some(event): Option<ExperienceEvent> = budget.read_record(
                self,
                super::experience::event_key(
                    tenant,
                    namespace,
                    &reference.attempt_id,
                    &reference.event_id,
                )?,
            )?
            else {
                return Ok(None);
            };
            let Some(current): Option<ExperienceRecord> = budget.read_record(
                self,
                super::experience::key(12, tenant, namespace, &reference.attempt_id)?,
            )?
            else {
                return Ok(None);
            };
            let alternate_context;
            let event_context = if event.record.receipt.request.context_id == context.id {
                context
            } else {
                let Some(loaded): Option<super::registry::RegistryEntry<super::registry::EvaluationContext>> = budget.read_record(
                    self, super::registry::registry_key(5, tenant, &event.record.receipt.request.context_id),
                )? else { return Ok(None); };
                if loaded.schema_version != 1 || loaded.tenant != tenant
                    || loaded.id != event.record.receipt.request.context_id
                    || loaded.payload_digest != super::registry::digest(&loaded.payload)? {
                    return Err(Error::DataCorruption("Experience context registry mismatch".into()));
                }
                alternate_context = loaded;
                &alternate_context
            };
            Self::validate_experience_receipt_context(
                tenant,
                namespace,
                &reference.attempt_id,
                &event.record.receipt,
                event_context,
            )?;
            Self::validate_experience_receipt_context(
                tenant,
                namespace,
                &reference.attempt_id,
                &current.receipt,
                event_context,
            )?;
            Self::validate_experience_event_record(&reference.event_id, &event, &current)?;
            if event.event_digest != reference.event_digest
                || event.record.receipt.context_digest
                    != request.experience_selection.context_digest
                || event.record.receipt.request.accounting_unit
                    != request.experience_selection.accounting_unit
            {
                return Err(Error::DataCorruption(
                    "Combined experience selection mismatch".into(),
                ));
            }
            events.push(event);
        }
        let export = Self::export_from_verified_events(
            request.experience_selection.context_digest.clone(),
            request.experience_selection.accounting_unit.clone(),
            events,
        )?;
        if export.export_digest != request.expected_export_digest {
            return Err(Error::DataCorruption(
                "Combined export digest mismatch".into(),
            ));
        }
        let origin = KnowledgeOriginPayload {
            schema_version: 1,
            method_version: "qilbee.experience-knowledge.v1".into(),
            tenant: tenant.into(),
            namespace: namespace.into(),
            request,
            export_digest: export.export_digest,
            summary: export.summary,
        };
        receipt.validate_origin_link(tenant, namespace, id, &origin)?;
        let digest = super::registry::digest(&linked.request)?;
        Self::validate_knowledge_binding_sources(
            tenant,
            namespace,
            id,
            linked,
            procedure,
            &[
                format!("qilbee-knowledge-v2:{digest}"),
                format!("qilbee-experience-knowledge-v1:{}", receipt.origin_digest),
            ],
        )?;
        let mut status_budget = super::experience_withdrawal::ExperienceReuseBudget::new(
            budget.record_limit.saturating_sub(budget.records_examined),
            budget.byte_limit.saturating_sub(budget.bytes_examined));
        let withdrawn = self.combined_withdrawn_budgeted(tenant, namespace, &origin.request.experience_selection.events, &mut status_budget);
        budget.records_examined += status_budget.work.learning_records_examined;
        budget.bytes_examined += status_budget.work.learning_bytes_inspected;
        budget.lookahead_bytes = status_budget.work.lookahead_bytes;
        let withdrawn = match withdrawn {
            Ok(value) => value,
            Err(Error::ExperienceReuseLimitExceeded) => {
                budget.stop = Some(if status_budget.work.lookahead_bytes > 0 { OriginBudgetStop::Bytes } else { OriginBudgetStop::Records });
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        Ok(Some((descriptor_from_verified_receipt(linked)?, withdrawn)))
    }
}

impl LearningMemory {
    fn combined_receipt_budgeted(
        &self, tenant: &str, namespace: &str, id: &str,
        budget: &mut super::experience_withdrawal::ExperienceReuseBudget,
    ) -> Result<Option<CombinedKnowledgeReceipt>> {
        let stored: Option<CombinedKnowledgeReceipt> = budget.read(self, origin_key(tenant, namespace, id)?)?;
        let Some(receipt) = stored else {
            let linked: Option<super::knowledge::KnowledgeReceipt> = budget.read(self, super::knowledge::key(tenant, namespace, id)?)?;
            let procedure = self.registered_procedure_budgeted(tenant, namespace, id, budget)?;
            if linked.as_ref().is_some_and(|r| has_origin_marker(&r.record.proposal.source_refs))
                || procedure.as_ref().is_some_and(|r| has_origin_marker(&r.record.proposal.source_refs)) {
                return Err(Error::DataCorruption("Mandatory combined origin missing".into()));
            }
            return Ok(None);
        };
        let request = receipt.request.clone().canonicalize().map_err(|_| Error::DataCorruption("Invalid combined historical request".into()))?;
        let export = self.strategy_export_budgeted(tenant, namespace, &request.experience_selection, budget)
            .map_err(|error| match error {
                Error::ExperienceReuseLimitExceeded => error,
                _ => Error::DataCorruption("Combined historical origin cannot be verified".into()),
            })?;
        if export.export_digest != request.expected_export_digest {
            return Err(Error::DataCorruption("Combined historical export differs".into()));
        }
        let origin = KnowledgeOriginPayload { schema_version: 1,
            method_version: "qilbee.experience-knowledge.v1".into(), tenant: tenant.into(), namespace: namespace.into(),
            request, export_digest: export.export_digest, summary: export.summary };
        receipt.validate_origin_link(tenant, namespace, id, &origin)?;
        let linked: super::knowledge::KnowledgeReceipt = budget.read(self, super::knowledge::key(tenant, namespace, id)?)?
            .ok_or_else(|| Error::DataCorruption("Combined linked receipt missing".into()))?;
        if linked != receipt.knowledge_receipt { return Err(Error::DataCorruption("Combined linked receipt differs".into())); }
        let procedure = self.registered_procedure_budgeted(tenant, namespace, id, budget)?
            .ok_or_else(|| Error::DataCorruption("Combined procedure missing".into()))?;
        Self::validate_knowledge_binding_sources(tenant, namespace, id, &linked, &procedure,
            &[format!("qilbee-knowledge-v2:{}", super::registry::digest(&linked.request)?),
              format!("qilbee-experience-knowledge-v1:{}", receipt.origin_digest)])?;
        Ok(Some(receipt))
    }

    fn combined_withdrawn_budgeted(&self, tenant: &str, namespace: &str,
        references: &[super::experience_export::ExperienceExportRef],
        budget: &mut super::experience_withdrawal::ExperienceReuseBudget,
    ) -> Result<bool> {
        let mut withdrawn = false;
        for reference in references {
            // Historical integrity is verified separately; this reads current status only.
            withdrawn |= self.withdrawal_with_budget(tenant, namespace, reference, budget)?.is_some();
        }
        Ok(withdrawn)
    }

    fn combined_memory_budgeted(&self, memory: &crate::RocksDbMemoryStorage, namespace: &str,
        sources: &[crate::storage::platform::MemorySourceRef],
        budget: &mut super::experience_withdrawal::ExperienceReuseBudget,
    ) -> Result<crate::storage::platform::MemoryEvidenceEligibility> {
        let records = budget.remaining_records().min(4096);
        let bytes = budget.remaining_bytes().min(16 * 1024 * 1024);
        if records == 0 || bytes == 0 { return Err(Error::ExperienceReuseLimitExceeded); }
        let view = memory.evidence_view().with_work_limits(records, bytes)?;
        let result = view.inspect(namespace, sources);
        let (work, lookahead, stop) = view.bounded_work();
        for _ in 0..work.records_examined { budget.charge_record()?; }
        budget.charge_bytes(work.bytes_examined)?;
        if stop.is_some() { budget.work.lookahead_bytes = lookahead; return Err(Error::ExperienceReuseLimitExceeded); }
        result
    }
}
