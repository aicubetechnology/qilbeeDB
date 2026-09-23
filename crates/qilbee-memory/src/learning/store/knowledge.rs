//! Evidence-bound procedural knowledge about externally owned tools.
use super::*;
use crate::storage::platform::MemorySourceRef;
use serde::Deserialize;

fn explicit_nullable<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<String>, D::Error> {
    Option::<String>::deserialize(deserializer)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalToolIdentity {
    pub name: String,
    pub schema_revision: String,
    #[serde(deserialize_with = "explicit_nullable")]
    pub implementation_revision: Option<String>,
    #[serde(deserialize_with = "explicit_nullable")]
    pub environment_revision: Option<String>,
}
impl ExternalToolIdentity {
    fn validate(&self) -> Result<()> {
        validate_text(&self.name, "external tool name", 512)?;
        validate_text(&self.schema_revision, "external schema revision", 512)?;
        match (&self.implementation_revision, &self.environment_revision) {
            (Some(implementation), Some(environment)) => {
                validate_text(implementation, "external implementation revision", 512)?;
                validate_text(environment, "external environment revision", 512)
            }
            (None, None) => Ok(()),
            _ => Err(Error::ValidationError(
                "Implementation and environment identities must both be supplied or both be null"
                    .into(),
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalToolKnowledge {
    pub name: String,
    pub schema_revision: String,
    #[serde(deserialize_with = "explicit_nullable")]
    pub implementation_revision: Option<String>,
    #[serde(deserialize_with = "explicit_nullable")]
    pub environment_revision: Option<String>,
    pub usage_contract: String,
}
impl ExternalToolKnowledge {
    pub fn identity(&self) -> ExternalToolIdentity {
        ExternalToolIdentity {
            name: self.name.clone(),
            schema_revision: self.schema_revision.clone(),
            implementation_revision: self.implementation_revision.clone(),
            environment_revision: self.environment_revision.clone(),
        }
    }
}

/// Sort by tool name only after rejecting duplicate names. Null identity is an
/// explicit unbound declaration, never a wildcard for an implementation.
pub fn canonical_external_tool_identities(
    mut identities: Vec<ExternalToolIdentity>,
) -> Result<Vec<ExternalToolIdentity>> {
    if identities.len() > 32 {
        return Err(Error::ValidationError(
            "At most 32 external tool identities are supported".into(),
        ));
    }
    for identity in &identities {
        identity.validate()?;
    }
    identities.sort_by(|a, b| a.name.cmp(&b.name));
    if identities
        .windows(2)
        .any(|pair| pair[0].name == pair[1].name)
    {
        return Err(Error::ValidationError(
            "External tool names must be unique".into(),
        ));
    }
    Ok(identities)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeProposal {
    pub id: String,
    pub policy_id: String,
    pub context_id: String,
    pub title: String,
    pub instructions: String,
    pub memory_sources: Vec<MemorySourceRef>,
    pub external_tools: Vec<ExternalToolKnowledge>,
}
impl KnowledgeProposal {
    /// Canonicalize set-valued inputs before immutable equality and hashing.
    /// Does not resolve external references or claim current source eligibility.
    pub fn canonicalize(mut self) -> Result<Self> {
        for (label, value) in [
            ("procedure ID", &self.id),
            ("policy ID", &self.policy_id),
            ("context ID", &self.context_id),
            ("knowledge title", &self.title),
        ] {
            validate_text(value, label, 512)?;
        }
        validate_text(&self.instructions, "knowledge instructions", 32768)?;
        if self.memory_sources.is_empty()
            || self.memory_sources.len() > 16
            || self.memory_sources.iter().any(|s| s.revision == 0)
        {
            return Err(Error::ValidationError(
                "Knowledge requires 1-16 exact positive source revisions".into(),
            ));
        }
        self.memory_sources.sort_by_key(|s| s.record_id);
        if self
            .memory_sources
            .windows(2)
            .any(|p| p[0].record_id == p[1].record_id)
        {
            return Err(Error::ValidationError(
                "Knowledge source IDs must be unique".into(),
            ));
        }
        canonical_external_tool_identities(
            self.external_tools
                .iter()
                .map(ExternalToolKnowledge::identity)
                .collect(),
        )?;
        for tool in &self.external_tools {
            validate_text(&tool.usage_contract, "external usage contract", 4096)?;
        }
        self.external_tools.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(self)
    }
    pub fn external_identities(&self) -> Result<Vec<ExternalToolIdentity>> {
        canonical_external_tool_identities(
            self.external_tools
                .iter()
                .map(ExternalToolKnowledge::identity)
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeReceipt {
    pub schema_version: u32,
    pub tenant: String,
    pub namespace: String,
    pub request: KnowledgeProposal,
    pub policy_digest: String,
    pub context_digest: String,
    pub actor: String,
    pub record: ProcedureRecord,
    /// Digest of all preceding fields, excluding this digest itself.
    pub receipt_digest: String,
}
pub(super) fn key(tenant: &str, namespace: &str, id: &str) -> Result<Vec<u8>> {
    validate_text(tenant, "tenant", 512)?;
    validate_text(namespace, "authorized namespace", 4096)?;
    validate_text(id, "knowledge ID", 512)?;
    let mut key = vec![16];
    for part in [tenant, namespace, id] {
        append_component(&mut key, part);
    }
    Ok(key)
}
impl KnowledgeReceipt {
    pub(super) fn digest(&self) -> Result<String> {
        super::registry::digest(&(
            self.schema_version,
            &self.tenant,
            &self.namespace,
            &self.request,
            &self.policy_digest,
            &self.context_digest,
            &self.actor,
            &self.record,
        ))
    }
}
impl LearningMemory {
    /// The host derives tenant, namespace and actor from current authorization.
    /// Source existence is deliberately not a precondition for recording a candidate.
    pub fn propose_knowledge(
        &self,
        tenant: &str,
        namespace: &str,
        request: KnowledgeProposal,
        actor: &str,
    ) -> Result<KnowledgeReceipt> {
        let request = request.canonicalize()?;
        let key = key(tenant, namespace, &request.id)?;
        validate_text(actor, "knowledge actor", 512)?;
        let _guard = self
            .inner
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Learning mutation lock poisoned".into()))?;
        if let Some(receipt) = self.knowledge_receipt(tenant, namespace, &request.id)? {
            return if receipt.request == request {
                Ok(receipt)
            } else {
                Err(Error::ConstraintViolation(
                    "Knowledge revisions are immutable; use a new ID".into(),
                ))
            };
        }
        if self
            .registered_procedure(tenant, namespace, &request.id)?
            .is_some()
        {
            return Err(Error::ConstraintViolation(
                "Existing procedures cannot acquire a knowledge binding".into(),
            ));
        }
        let (binding, receipt) =
            self.prepare_knowledge_receipt(tenant, namespace, request, actor, None)?;
        let mut batch = WriteBatch::default();
        self.put_knowledge_locator(&mut batch, &receipt)?;
        Self::put_registered_proposal(&mut batch, &binding)?;
        batch.put(key, encode(&receipt)?);
        self.inner
            .db
            .write_opt(batch, &write_options())
            .map_err(storage_error)?;
        Ok(receipt)
    }
    /// Prepare immutable linked records without committing any of them. The
    /// caller holds the mutation lock and commits every required origin record
    /// in the same synchronous batch. Origin admission must resolve evidence first.
    pub(super) fn prepare_knowledge_receipt(
        &self,
        tenant: &str,
        namespace: &str,
        request: KnowledgeProposal,
        actor: &str,
        origin_digest: Option<&str>,
    ) -> Result<(super::bound::ProposalReceipt, KnowledgeReceipt)> {
        let context = self
            .context(tenant, &request.context_id)?
            .ok_or_else(|| Error::KeyNotFound("Evaluation context not found".into()))?;
        for tool in &request.external_tools {
            if context.payload.tools.get(&tool.name) != Some(&tool.schema_revision)
                || tool
                    .environment_revision
                    .as_ref()
                    .is_some_and(|v| v != &context.payload.environment_revision)
            {
                return Err(Error::ValidationError(
                    "External tool identity differs from evaluation context".into(),
                ));
            }
        }
        let binding_digest = super::registry::digest(&request)?;
        let mut source_refs = vec![format!("qilbee-knowledge-v2:{binding_digest}")];
        if let Some(origin_digest) = origin_digest {
            super::experience::validate_digest(origin_digest)?;
            source_refs.push(format!("qilbee-experience-knowledge-v1:{origin_digest}"));
        }
        let legacy = super::bound::RegisteredProposal {
            id: request.id.clone(),
            policy_id: request.policy_id.clone(),
            context_id: request.context_id.clone(),
            instructions: request.instructions.clone(),
            source_refs,
        };
        let binding = self.prepare_registered_proposal(tenant, namespace, legacy, actor)?;
        let mut receipt = KnowledgeReceipt {
            schema_version: 2,
            tenant: tenant.into(),
            namespace: namespace.into(),
            request,
            policy_digest: binding.policy_digest.clone(),
            context_digest: binding.context_digest.clone(),
            actor: actor.into(),
            record: binding.record.clone(),
            receipt_digest: String::new(),
        };
        receipt.receipt_digest = receipt.digest()?;
        Ok((binding, receipt))
    }
    /// Original immutable receipt only; this does not report current eligibility.
    pub fn knowledge_receipt(
        &self,
        tenant: &str,
        namespace: &str,
        id: &str,
    ) -> Result<Option<KnowledgeReceipt>> {
        if self
            .inner
            .db
            .get(super::knowledge_origin::origin_key(tenant, namespace, id)?)
            .map_err(storage_error)?
            .is_some()
        {
            // Verify stored evidence before reporting a negotiated-version conflict.
            self.combined_knowledge_receipt(tenant, namespace, id)?
                .ok_or_else(|| Error::DataCorruption("Knowledge origin disappeared".into()))?;
            return Err(Error::UnsupportedKnowledgeOrigin);
        }
        let Some(bytes) = self
            .inner
            .db
            .get(key(tenant, namespace, id)?)
            .map_err(storage_error)?
        else {
            return Ok(None);
        };
        let receipt: KnowledgeReceipt = decode(&bytes)?;
        let binding = self
            .registered_procedure(tenant, namespace, id)?
            .ok_or_else(|| {
                Error::DataCorruption("Knowledge procedure binding is missing".into())
            })?;
        Self::validate_knowledge_binding(tenant, namespace, id, &receipt, &binding)?;
        Ok(Some(receipt))
    }
    pub(super) fn validate_knowledge_binding(
        tenant: &str,
        namespace: &str,
        id: &str,
        receipt: &KnowledgeReceipt,
        binding: &super::bound::RegisteredProcedure,
    ) -> Result<()> {
        let request_digest = super::registry::digest(&receipt.request)?;
        Self::validate_knowledge_binding_sources(
            tenant,
            namespace,
            id,
            receipt,
            binding,
            &[format!("qilbee-knowledge-v2:{request_digest}")],
        )
    }

    pub(super) fn validate_knowledge_binding_sources(
        tenant: &str,
        namespace: &str,
        id: &str,
        receipt: &KnowledgeReceipt,
        binding: &super::bound::RegisteredProcedure,
        expected_sources: &[String],
    ) -> Result<()> {
        if receipt.schema_version != 2
            || receipt.tenant != tenant
            || receipt.namespace != namespace
            || receipt.request.id != id
            || receipt
                .request
                .clone()
                .canonicalize()
                .map_err(|_| Error::DataCorruption("Invalid knowledge request".into()))?
                != receipt.request
            || receipt.receipt_digest != receipt.digest()?
            || receipt.record != binding.receipt.record
            || receipt.actor != binding.receipt.actor
            || receipt.policy_digest != binding.receipt.policy_digest
            || receipt.context_digest != binding.receipt.context_digest
            || binding.receipt.request.instructions != receipt.request.instructions
            || binding.receipt.request.policy_id != receipt.request.policy_id
            || binding.receipt.request.context_id != receipt.request.context_id
            || binding.receipt.request.source_refs != expected_sources
        {
            return Err(Error::DataCorruption(
                "Knowledge receipt integrity mismatch".into(),
            ));
        }
        Ok(())
    }
}

const SOURCE_PREFIX: &str = "qilbee-knowledge-v2:";
pub(super) fn has_knowledge_binding_marker(sources: &[String]) -> bool {
    sources
        .iter()
        .any(|source| source.starts_with(SOURCE_PREFIX))
}
pub(super) fn reject_reserved_legacy_sources(sources: &[String]) -> Result<()> {
    if has_knowledge_binding_marker(sources)
        || sources
            .iter()
            .any(|source| source.starts_with("qilbee-experience-knowledge-v1:"))
    {
        Err(Error::ValidationError(
            "Reserved knowledge binding references require the v2 proposal contract".into(),
        ))
    } else {
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeInspection {
    pub receipt: KnowledgeReceipt,
    pub procedure: super::bound::RegisteredProcedure,
    pub qualification_active: bool,
    pub evidence: crate::storage::platform::MemoryEvidenceEligibility,
    pub eligible_for_knowledge_reuse: bool,
}
impl LearningMemory {
    /// Observe current qualification while source checks share one memory snapshot.
    /// This observation is not a reservation or permission for external execution.
    pub fn inspect_knowledge(
        &self,
        memory: &crate::RocksDbMemoryStorage,
        tenant: &str,
        namespace: &str,
        id: &str,
    ) -> Result<Option<KnowledgeInspection>> {
        let _guard = self
            .inner
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Learning mutation lock poisoned".into()))?;
        let Some(receipt) = self.knowledge_receipt(tenant, namespace, id)? else {
            return Ok(None);
        };
        let procedure = self
            .registered_procedure(tenant, namespace, id)?
            .ok_or_else(|| Error::DataCorruption("Knowledge procedure is missing".into()))?;
        let evidence =
            memory.inspect_memory_evidence(namespace, &receipt.request.memory_sources)?;
        let qualification_active = procedure.record.state == ProcedureState::Active;
        let eligible_for_knowledge_reuse =
            qualification_active && evidence.eligible && evidence.all_dependencies_checked;
        Ok(Some(KnowledgeInspection {
            receipt,
            procedure,
            qualification_active,
            evidence,
            eligible_for_knowledge_reuse,
        }))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeSelectRequest {
    pub policy_id: String,
    pub context_id: String,
    pub max_instruction_bytes: usize,
    pub candidate_limit: usize,
    pub external_tool_identities: Vec<ExternalToolIdentity>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum KnowledgeSelectionOutcome {
    Procedure {
        knowledge: KnowledgeInspection,
    },
    Baseline {
        baseline_revision: String,
        reason: String,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeSelectionCoverage {
    pub records_examined: usize,
    pub candidates_eligible: usize,
    pub complete: bool,
    pub stop_reason: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeSelection {
    pub selection: KnowledgeSelectionOutcome,
    pub evaluated_at_millis: i64,
    pub coverage: KnowledgeSelectionCoverage,
}
impl LearningMemory {
    /// Select only fully inspected v2 knowledge. No cursor or execution reservation.
    pub fn select_knowledge(
        &self,
        memory: &crate::RocksDbMemoryStorage,
        tenant: &str,
        namespace: &str,
        request: KnowledgeSelectRequest,
    ) -> Result<KnowledgeSelection> {
        if request.candidate_limit == 0
            || request.candidate_limit > 1000
            || request.max_instruction_bytes > 65536
        {
            return Err(Error::ValidationError("Knowledge selection requires 1-1000 candidates and at most 65536 instruction bytes".into()));
        }
        let identities = canonical_external_tool_identities(request.external_tool_identities)?;
        let _guard = self
            .inner
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Learning mutation lock poisoned".into()))?;
        let scope =
            self.registered_scope(tenant, namespace, &request.policy_id, &request.context_id)?;
        let context = self
            .context(tenant, &request.context_id)?
            .ok_or_else(|| Error::KeyNotFound("Evaluation context not found".into()))?;
        let view = memory.evidence_view();
        let prefix = scope_prefix(1, &scope);
        let mut coverage = KnowledgeSelectionCoverage {
            records_examined: 0,
            candidates_eligible: 0,
            complete: true,
            stop_reason: "complete".into(),
        };
        let mut best: Option<KnowledgeInspection> = None;
        for item in self
            .inner
            .db
            .iterator(IteratorMode::From(&prefix, Direction::Forward))
        {
            let (key, bytes) = item.map_err(storage_error)?;
            if !key.starts_with(&prefix) {
                break;
            }
            if coverage.records_examined == request.candidate_limit {
                coverage.complete = false;
                coverage.stop_reason = "candidate_limit".into();
                break;
            }
            coverage.records_examined += 1;
            let record: ProcedureRecord = decode(&bytes)?;
            if super::knowledge_origin::has_origin_marker(&record.proposal.source_refs) {
                self.combined_knowledge_receipt(tenant, namespace, &record.proposal.id)?
                    .ok_or_else(|| {
                        Error::DataCorruption("Knowledge selection origin is missing".into())
                    })?;
                continue;
            }
            if !has_knowledge_binding_marker(&record.proposal.source_refs) {
                continue;
            }
            let receipt = self
                .knowledge_receipt(tenant, namespace, &record.proposal.id)?
                .ok_or_else(|| {
                    Error::DataCorruption("Knowledge selection binding is missing".into())
                })?;
            if record.scope != scope || receipt.record.scope != scope {
                return Err(Error::DataCorruption(
                    "Knowledge selection scope mismatch".into(),
                ));
            }
            if record.state != ProcedureState::Active
                || receipt.request.instructions.len() > request.max_instruction_bytes
                || receipt.request.external_identities()? != identities
            {
                continue;
            }
            let evidence = view.inspect(namespace, &receipt.request.memory_sources)?;
            if !evidence.eligible || !evidence.all_dependencies_checked {
                continue;
            }
            coverage.candidates_eligible += 1;
            let procedure = self
                .registered_procedure(tenant, namespace, &record.proposal.id)?
                .ok_or_else(|| {
                    Error::DataCorruption("Knowledge selection procedure is missing".into())
                })?;
            if procedure.record != record {
                return Err(Error::DataCorruption(
                    "Knowledge selection ledger mismatch".into(),
                ));
            }
            let replace = best.as_ref().is_none_or(|old| {
                record.lower_improvement_bound > old.procedure.record.lower_improvement_bound
                    || (record.lower_improvement_bound
                        == old.procedure.record.lower_improvement_bound
                        && record.proposal.id < old.procedure.record.proposal.id)
            });
            if replace {
                best = Some(KnowledgeInspection {
                    receipt,
                    procedure,
                    qualification_active: true,
                    evidence,
                    eligible_for_knowledge_reuse: true,
                });
            }
        }
        let selection = if !coverage.complete {
            KnowledgeSelectionOutcome::Baseline {
                baseline_revision: context.payload.baseline_revision,
                reason: "selection_incomplete".into(),
            }
        } else if let Some(knowledge) = best {
            KnowledgeSelectionOutcome::Procedure { knowledge }
        } else {
            KnowledgeSelectionOutcome::Baseline {
                baseline_revision: context.payload.baseline_revision,
                reason: "no_eligible_bound_procedure".into(),
            }
        };
        Ok(KnowledgeSelection {
            selection,
            evaluated_at_millis: view.observed_at_millis(),
            coverage,
        })
    }
}
