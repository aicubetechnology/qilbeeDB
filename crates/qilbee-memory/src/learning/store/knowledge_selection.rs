//! Explicit, bounded selection contract for evidence-bound knowledge.
//! Selection provides knowledge; it never authorizes application actions.
use super::knowledge::{ExternalToolIdentity, canonical_external_tool_identities};
use super::*;
use serde::Deserialize;

pub const KNOWLEDGE_SELECTION_VERSION: &str = "active_knowledge_bound_v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeSelectionWorkLimits {
    pub candidate_records: usize,
    pub index_entries: usize,
    pub learning_bytes: usize,
    pub dependency_records: usize,
    pub dependency_bytes: usize,
}
impl KnowledgeSelectionWorkLimits {
    pub fn validate(&self) -> Result<()> {
        for (name, value, maximum) in [
            ("candidate_records", self.candidate_records, 1000),
            ("index_entries", self.index_entries, 4096),
            ("learning_bytes", self.learning_bytes, 16_777_216),
            ("dependency_records", self.dependency_records, 4096),
            ("dependency_bytes", self.dependency_bytes, 16_777_216),
        ] {
            if value == 0 || value > maximum {
                return Err(Error::ValidationError(format!(
                    "{name} must be between 1 and {maximum}"
                )));
            }
        }
        Ok(())
    }
}

/// Native request; the HTTP adapter supplies the authenticated storage namespace.
/// All work limits are explicit. There is no implicit v2 downgrade.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeSelectRequestV3 {
    pub selection_version: String,
    pub policy_id: String,
    pub context_id: String,
    pub max_instruction_bytes: usize,
    pub external_tool_identities: Vec<ExternalToolIdentity>,
    pub work_limits: KnowledgeSelectionWorkLimits,
}
impl KnowledgeSelectRequestV3 {
    pub fn canonicalize(mut self) -> Result<Self> {
        if self.selection_version != KNOWLEDGE_SELECTION_VERSION {
            return Err(Error::ValidationError(
                "Unsupported knowledge selection version".into(),
            ));
        }
        validate_text(&self.policy_id, "policy ID", 512)?;
        validate_text(&self.context_id, "context ID", 512)?;
        if self.max_instruction_bytes > 65_536 {
            return Err(Error::ValidationError(
                "Instruction budget must not exceed 65536 bytes".into(),
            ));
        }
        self.work_limits.validate()?;
        self.external_tool_identities =
            canonical_external_tool_identities(self.external_tool_identities)?;
        Ok(self)
    }
}

/// Request-local inspection accounting, not physical disk-I/O measurements.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeSelectionWork {
    pub index_entries_examined: usize,
    pub candidate_records_examined: usize,
    pub learning_bytes_inspected: usize,
    pub learning_lookahead_bytes: usize,
    pub dependency_records_examined: usize,
    pub dependency_bytes_inspected: usize,
    pub dependency_lookahead_bytes: usize,
    pub eligible_candidates: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum KnowledgeSelectionOutcomeV3 {
    Procedure {
        knowledge: super::knowledge::KnowledgeInspection,
    },
    Baseline {
        baseline_revision: String,
        reason: String,
    },
    Incomplete {
        baseline_revision: String,
        reason: String,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeSelectionCoverageV3 {
    pub decision_complete: bool,
    pub index_exhausted: bool,
    pub stop_reason: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeSelectionV3 {
    pub selection_version: String,
    pub evaluated_at_millis: i64,
    pub index_generation: uuid::Uuid,
    pub result: KnowledgeSelectionOutcomeV3,
    pub coverage: KnowledgeSelectionCoverageV3,
    pub work: KnowledgeSelectionWork,
}
impl KnowledgeSelectionWork {
    fn admit_learning_bytes(&mut self, bytes: usize, limit: usize) -> bool {
        if bytes > limit.saturating_sub(self.learning_bytes_inspected) {
            self.learning_lookahead_bytes = bytes;
            false
        } else {
            self.learning_bytes_inspected += bytes;
            true
        }
    }
}
impl LearningMemory {
    fn selection_record<T: DeserializeOwned>(
        &self,
        key: Vec<u8>,
        work: &mut KnowledgeSelectionWork,
        limit: usize,
    ) -> Result<Option<T>> {
        let bytes = self
            .inner
            .db
            .get(key)
            .map_err(storage_error)?
            .ok_or_else(|| Error::DataCorruption("Selection binding record is missing".into()))?;
        if !work.admit_learning_bytes(bytes.len(), limit) {
            return Ok(None);
        }
        decode(&bytes).map(Some)
    }

    /// Return the first fully verified candidate in the immutable server-owned order.
    pub fn select_knowledge_v3(
        &self,
        memory: &crate::RocksDbMemoryStorage,
        tenant: &str,
        namespace: &str,
        request: KnowledgeSelectRequestV3,
    ) -> Result<KnowledgeSelectionV3> {
        use super::bound::{ProposalReceipt, RegisteredProcedure};
        use super::knowledge::{KnowledgeInspection, KnowledgeReceipt};
        use crate::storage::platform::{DependencyBudgetStop, MemoryEligibilityReason};
        let request = request.canonicalize()?;
        let _guard = self
            .inner
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Learning mutation lock poisoned".into()))?;
        let (policy, context) =
            self.contracts(tenant, namespace, &request.policy_id, &request.context_id)?;
        let scope = Self::bound_scope(tenant, namespace, &policy, &context)?;
        let view = memory.evidence_view().with_work_limits(
            request.work_limits.dependency_records,
            request.work_limits.dependency_bytes,
        )?;
        let prefix = self.selection_index_prefix(
            tenant,
            namespace,
            &request.policy_id,
            &request.context_id,
            &request.external_tool_identities,
        )?;
        let mut work = KnowledgeSelectionWork::default();
        let mut stop = "index_exhausted";
        let mut selected = None;
        let mut iterator = self.inner.db.raw_iterator();
        iterator.seek(&prefix);
        while iterator.valid() {
            let key = iterator
                .key()
                .ok_or_else(|| Error::DataCorruption("Missing index key".into()))?;
            if !key.starts_with(&prefix) {
                break;
            }
            if work.index_entries_examined == request.work_limits.index_entries {
                stop = "index_entry_limit";
                break;
            }
            if work.candidate_records_examined == request.work_limits.candidate_records {
                stop = "candidate_record_limit";
                break;
            }
            work.index_entries_examined += 1;
            let bytes = iterator
                .value()
                .ok_or_else(|| Error::DataCorruption("Missing index value".into()))?;
            if !work.admit_learning_bytes(bytes.len(), request.work_limits.learning_bytes) {
                stop = "learning_byte_limit";
                break;
            }
            let indexed: ProcedureRecord = decode(bytes)?;
            work.candidate_records_examined += 1;
            if indexed.scope != scope || indexed.state != ProcedureState::Active {
                return Err(Error::DataCorruption(
                    "Active index scope or state mismatch".into(),
                ));
            }
            let id = &indexed.proposal.id;
            macro_rules! read {
                ($key:expr, $ty:ty) => {
                    match self.selection_record::<$ty>(
                        $key,
                        &mut work,
                        request.work_limits.learning_bytes,
                    )? {
                        Some(value) => value,
                        None => {
                            stop = "learning_byte_limit";
                            break;
                        }
                    }
                };
            }
            let current = read!(procedure_key(&scope, id), ProcedureRecord);
            let binding = read!(
                super::bound::binding_key(tenant, namespace, id),
                ProposalReceipt
            );
            let receipt = read!(
                super::knowledge::key(tenant, namespace, id)?,
                KnowledgeReceipt
            );
            Self::validate_registered_binding(
                tenant, namespace, id, &binding, &current, &policy, &context,
            )?;
            let procedure = RegisteredProcedure {
                receipt: binding,
                record: current,
            };
            Self::validate_knowledge_binding(tenant, namespace, id, &receipt, &procedure)?;
            if procedure.record != indexed
                || self.active_key(&receipt, &indexed)?.as_slice() != key
                || receipt.request.policy_id != request.policy_id
                || receipt.request.context_id != request.context_id
                || receipt.request.external_identities()? != request.external_tool_identities
            {
                return Err(Error::DataCorruption(
                    "Knowledge index differs from authoritative bindings".into(),
                ));
            }
            if receipt.request.instructions.len() > request.max_instruction_bytes {
                iterator.next();
                continue;
            }
            let observed = view.inspect(namespace, &receipt.request.memory_sources);
            let (dependency_work, lookahead, exhausted) = view.bounded_work();
            work.dependency_records_examined = dependency_work.records_examined;
            work.dependency_bytes_inspected = dependency_work.bytes_examined;
            work.dependency_lookahead_bytes = lookahead;
            if let Some(exhausted) = exhausted {
                stop = match exhausted {
                    DependencyBudgetStop::Records => "dependency_record_limit",
                    DependencyBudgetStop::Bytes => "dependency_byte_limit",
                };
                break;
            }
            let evidence = observed?;
            if let Some(failure) = &evidence.first_failure {
                match failure.reason {
                    MemoryEligibilityReason::DepthLimit => {
                        stop = "dependency_depth_limit";
                        break;
                    }
                    MemoryEligibilityReason::NodeLimit => {
                        stop = "dependency_node_limit";
                        break;
                    }
                    MemoryEligibilityReason::Eligible => {
                        return Err(Error::DataCorruption("Invalid evidence failure".into()));
                    }
                    _ => {
                        iterator.next();
                        continue;
                    }
                }
            }
            if !evidence.eligible || !evidence.all_dependencies_checked {
                return Err(Error::DataCorruption(
                    "Evidence completion flags are inconsistent".into(),
                ));
            }
            work.eligible_candidates = 1;
            selected = Some(KnowledgeInspection {
                receipt,
                procedure,
                qualification_active: true,
                eligible_for_knowledge_reuse: true,
                evidence,
            });
            stop = "first_eligible";
            break;
        }
        iterator.status().map_err(storage_error)?;
        let result = if let Some(knowledge) = selected {
            KnowledgeSelectionOutcomeV3::Procedure { knowledge }
        } else if stop == "index_exhausted" {
            KnowledgeSelectionOutcomeV3::Baseline {
                baseline_revision: context.payload.baseline_revision,
                reason: "no_eligible_bound_procedure".into(),
            }
        } else {
            KnowledgeSelectionOutcomeV3::Incomplete {
                baseline_revision: context.payload.baseline_revision,
                reason: stop.into(),
            }
        };
        Ok(KnowledgeSelectionV3 {
            selection_version: request.selection_version,
            evaluated_at_millis: view.observed_at_millis(),
            index_generation: self.inner.knowledge_generation,
            result,
            work,
            coverage: KnowledgeSelectionCoverageV3 {
                decision_complete: stop == "first_eligible" || stop == "index_exhausted",
                index_exhausted: stop == "index_exhausted",
                stop_reason: stop.into(),
            },
        })
    }
}

/// Explicit origin admission, canonically ordered without ranking preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeOriginKind {
    MemoryOnly,
    ExperienceMemory,
}
pub fn canonical_origin_kinds(
    mut kinds: Vec<KnowledgeOriginKind>,
) -> Result<Vec<KnowledgeOriginKind>> {
    if kinds.is_empty() || kinds.len() > 2 {
        return Err(Error::ValidationError(
            "Accept one or two origin kinds".into(),
        ));
    }
    kinds.sort();
    if kinds.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(Error::ValidationError("Duplicate origin kind".into()));
    }
    Ok(kinds)
}
pub const KNOWLEDGE_ORIGIN_SELECTION_VERSION: &str = "active_knowledge_origin_bound_v1";
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeSelectRequestV4 {
    pub selection_version: String,
    pub accepted_origin_kinds: Vec<KnowledgeOriginKind>,
    pub policy_id: String,
    pub context_id: String,
    pub max_instruction_bytes: usize,
    pub external_tool_identities: Vec<ExternalToolIdentity>,
    pub work_limits: KnowledgeSelectionWorkLimits,
}
impl KnowledgeSelectRequestV4 {
    fn canonicalize(mut self) -> Result<Self> {
        if self.selection_version != KNOWLEDGE_ORIGIN_SELECTION_VERSION {
            return Err(Error::ValidationError(
                "Unsupported knowledge selection version".into(),
            ));
        }
        self.accepted_origin_kinds = canonical_origin_kinds(self.accepted_origin_kinds)?;
        let canonical = KnowledgeSelectRequestV3 {
            selection_version: KNOWLEDGE_SELECTION_VERSION.into(),
            policy_id: self.policy_id,
            context_id: self.context_id,
            max_instruction_bytes: self.max_instruction_bytes,
            external_tool_identities: self.external_tool_identities,
            work_limits: self.work_limits,
        }
        .canonicalize()?;
        self.policy_id = canonical.policy_id;
        self.context_id = canonical.context_id;
        self.external_tool_identities = canonical.external_tool_identities;
        self.work_limits = canonical.work_limits;
        Ok(self)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum KnowledgeSelectionOutcomeV4 {
    Procedure {
        knowledge: super::knowledge::KnowledgeInspection,
        origin: super::knowledge_origin::KnowledgeOriginDescriptor,
    },
    ExperienceKnowledge {
        knowledge: super::knowledge::KnowledgeInspection,
        origin: super::knowledge_origin::KnowledgeOriginDescriptor,
    },
    Baseline {
        baseline_revision: String,
        reason: String,
    },
    Incomplete {
        baseline_revision: String,
        reason: String,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeSelectionV4 {
    pub selection_version: String,
    pub accepted_origin_kinds: Vec<KnowledgeOriginKind>,
    pub evaluated_at_millis: i64,
    pub index_generation: uuid::Uuid,
    pub result: KnowledgeSelectionOutcomeV4,
    pub coverage: KnowledgeSelectionCoverageV3,
    pub work: KnowledgeSelectionWork,
}

impl LearningMemory {
    /// Return the first fully verified candidate in the immutable server-owned order.
    pub fn select_knowledge_v4(
        &self,
        memory: &crate::RocksDbMemoryStorage,
        tenant: &str,
        namespace: &str,
        request: KnowledgeSelectRequestV4,
    ) -> Result<KnowledgeSelectionV4> {
        use super::bound::{ProposalReceipt, RegisteredProcedure};
        use super::knowledge::{KnowledgeInspection, KnowledgeReceipt};
        use crate::storage::platform::{DependencyBudgetStop, MemoryEligibilityReason};
        let request = request.canonicalize()?;
        let _guard = self
            .inner
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Learning mutation lock poisoned".into()))?;
        let (policy, context) =
            self.contracts(tenant, namespace, &request.policy_id, &request.context_id)?;
        let scope = Self::bound_scope(tenant, namespace, &policy, &context)?;
        let view = memory.evidence_view().with_work_limits(
            request.work_limits.dependency_records,
            request.work_limits.dependency_bytes,
        )?;
        let ordinary_prefix = self.selection_index_prefix(
            tenant,
            namespace,
            &request.policy_id,
            &request.context_id,
            &request.external_tool_identities,
        )?;
        let mut work = KnowledgeSelectionWork::default();
        // As in v3, fixed verified policy/context lookup precedes candidate
        // inspection. These counters bound candidate work, not physical I/O.
        let mut stop = "index_exhausted";
        let mut selected = None;
        let mut prefixes = Vec::new();
        for kind in &request.accepted_origin_kinds {
            prefixes.push(self.origin_selection_index_prefix(ordinary_prefix.clone(), *kind));
        }
        let mut iterators: Vec<_> = prefixes
            .iter()
            .map(|prefix| {
                let mut iterator = self.inner.db.raw_iterator();
                iterator.seek(prefix);
                iterator
            })
            .collect();
        let mut admitted_heads = vec![false; iterators.len()];
        'selection: while stop == "index_exhausted" {
            // Compare only the shared ordering suffix; projection kind never ranks.
            let mut next: Option<usize> = None;
            for (position, iterator) in iterators.iter().enumerate() {
                if !iterator.valid() {
                    iterator.status().map_err(storage_error)?;
                    continue;
                }
                let key = iterator
                    .key()
                    .ok_or_else(|| Error::DataCorruption("Missing index key".into()))?;
                if !key.starts_with(&prefixes[position]) {
                    continue;
                }
                if !admitted_heads[position] {
                    if work.index_entries_examined == request.work_limits.index_entries {
                        stop = "index_entry_limit";
                        break 'selection;
                    }
                    work.index_entries_examined += 1;
                    admitted_heads[position] = true;
                }
                if let Some(previous) = next {
                    let previous_key = iterators[previous].key().unwrap();
                    let suffix = &key[prefixes[position].len()..];
                    let previous_suffix = &previous_key[prefixes[previous].len()..];
                    if suffix == previous_suffix {
                        return Err(Error::DataCorruption(
                            "Knowledge occurs in multiple origin projections".into(),
                        ));
                    }
                    if suffix < previous_suffix {
                        next = Some(position);
                    }
                } else {
                    next = Some(position);
                }
            }
            let Some(position) = next else {
                break;
            };
            let iterator = &mut iterators[position];
            let key = iterator
                .key()
                .ok_or_else(|| Error::DataCorruption("Missing index key".into()))?;
            if work.candidate_records_examined == request.work_limits.candidate_records {
                stop = "candidate_record_limit";
                break;
            }
            let bytes = iterator
                .value()
                .ok_or_else(|| Error::DataCorruption("Missing index value".into()))?;
            if !work.admit_learning_bytes(bytes.len(), request.work_limits.learning_bytes) {
                stop = "learning_byte_limit";
                break;
            }
            let indexed: ProcedureRecord = decode(bytes)?;
            work.candidate_records_examined += 1;
            if indexed.scope != scope || indexed.state != ProcedureState::Active {
                return Err(Error::DataCorruption(
                    "Active index scope or state mismatch".into(),
                ));
            }
            let id = &indexed.proposal.id;
            macro_rules! read {
                ($key:expr, $ty:ty) => {{
                    if work.candidate_records_examined == request.work_limits.candidate_records {
                        stop = "candidate_record_limit";
                        break;
                    }
                    work.candidate_records_examined += 1;
                    match self.selection_record::<$ty>(
                        $key,
                        &mut work,
                        request.work_limits.learning_bytes,
                    )? {
                        Some(value) => value,
                        None => {
                            stop = "learning_byte_limit";
                            break;
                        }
                    }
                }};
            }
            let current = read!(procedure_key(&scope, id), ProcedureRecord);
            let binding = read!(
                super::bound::binding_key(tenant, namespace, id),
                ProposalReceipt
            );
            let receipt = read!(
                super::knowledge::key(tenant, namespace, id)?,
                KnowledgeReceipt
            );
            Self::validate_registered_binding(
                tenant, namespace, id, &binding, &current, &policy, &context,
            )?;
            let procedure = RegisteredProcedure {
                receipt: binding,
                record: current,
            };
            let mut experience_withdrawn = false;
            let origin = if request.accepted_origin_kinds[position]
                == KnowledgeOriginKind::ExperienceMemory
            {
                let mut origin_budget = super::knowledge_origin::OriginReadBudget::new(
                    request
                        .work_limits
                        .candidate_records
                        .saturating_sub(work.candidate_records_examined),
                    request
                        .work_limits
                        .learning_bytes
                        .saturating_sub(work.learning_bytes_inspected),
                );
                let verified = self.validate_combined_origin_bounded(
                    tenant,
                    namespace,
                    id,
                    &receipt,
                    &procedure,
                    &context,
                    &mut origin_budget,
                )?;
                work.candidate_records_examined += origin_budget.records_examined;
                work.learning_bytes_inspected += origin_budget.bytes_examined;
                work.learning_lookahead_bytes = origin_budget.lookahead_bytes;
                match verified {
                    Some((origin, withdrawn)) => { experience_withdrawn = withdrawn; origin },
                    None => {
                        stop = match origin_budget.stop {
                            Some(super::knowledge_origin::OriginBudgetStop::Records) => {
                                "candidate_record_limit"
                            }
                            Some(super::knowledge_origin::OriginBudgetStop::Bytes) => {
                                "learning_byte_limit"
                            }
                            None => {
                                return Err(Error::DataCorruption(
                                    "Origin validation stopped without a budget reason".into(),
                                ));
                            }
                        };
                        break;
                    }
                }
            } else {
                let mut origin_budget = super::knowledge_origin::OriginReadBudget::new(
                    request
                        .work_limits
                        .candidate_records
                        .saturating_sub(work.candidate_records_examined),
                    request
                        .work_limits
                        .learning_bytes
                        .saturating_sub(work.learning_bytes_inspected),
                );
                let unexpected: Option<super::knowledge_origin::CombinedKnowledgeReceipt> =
                    origin_budget.read_optional(
                        self,
                        super::knowledge_origin::origin_key(tenant, namespace, id)?,
                    )?;
                work.candidate_records_examined += origin_budget.records_examined;
                work.learning_bytes_inspected += origin_budget.bytes_examined;
                work.learning_lookahead_bytes = origin_budget.lookahead_bytes;
                if let Some(exhausted) = origin_budget.stop {
                    stop = match exhausted {
                        super::knowledge_origin::OriginBudgetStop::Records => {
                            "candidate_record_limit"
                        }
                        super::knowledge_origin::OriginBudgetStop::Bytes => "learning_byte_limit",
                    };
                    break;
                }
                if unexpected.is_some() {
                    return Err(Error::DataCorruption(
                        "Ordinary projection has an unexpected combined origin".into(),
                    ));
                }
                Self::validate_knowledge_binding(tenant, namespace, id, &receipt, &procedure)?;
                super::knowledge_origin::KnowledgeOriginDescriptor::MemoryOnly
            };
            if procedure.record != indexed
                || self.active_key(&receipt, &indexed)?.as_slice() != key
                || receipt.request.policy_id != request.policy_id
                || receipt.request.context_id != request.context_id
                || receipt.request.external_identities()? != request.external_tool_identities
            {
                return Err(Error::DataCorruption(
                    "Knowledge index differs from authoritative bindings".into(),
                ));
            }
            if experience_withdrawn {
                iterator.next();
                admitted_heads[position] = false;
                continue;
            }
            if receipt.request.instructions.len() > request.max_instruction_bytes {
                iterator.next();
                admitted_heads[position] = false;
                continue;
            }
            let observed = view.inspect(namespace, &receipt.request.memory_sources);
            let (dependency_work, lookahead, exhausted) = view.bounded_work();
            work.dependency_records_examined = dependency_work.records_examined;
            work.dependency_bytes_inspected = dependency_work.bytes_examined;
            work.dependency_lookahead_bytes = lookahead;
            if let Some(exhausted) = exhausted {
                stop = match exhausted {
                    DependencyBudgetStop::Records => "dependency_record_limit",
                    DependencyBudgetStop::Bytes => "dependency_byte_limit",
                };
                break;
            }
            let evidence = observed?;
            if let Some(failure) = &evidence.first_failure {
                match failure.reason {
                    MemoryEligibilityReason::DepthLimit => {
                        stop = "dependency_depth_limit";
                        break;
                    }
                    MemoryEligibilityReason::NodeLimit => {
                        stop = "dependency_node_limit";
                        break;
                    }
                    MemoryEligibilityReason::Eligible => {
                        return Err(Error::DataCorruption("Invalid evidence failure".into()));
                    }
                    _ => {
                        iterator.next();
                        admitted_heads[position] = false;
                        continue;
                    }
                }
            }
            if !evidence.eligible || !evidence.all_dependencies_checked {
                return Err(Error::DataCorruption(
                    "Evidence completion flags are inconsistent".into(),
                ));
            }
            work.eligible_candidates = 1;
            selected = Some((
                KnowledgeInspection {
                    receipt,
                    procedure,
                    qualification_active: true,
                    eligible_for_knowledge_reuse: true,
                    evidence,
                },
                origin,
            ));
            stop = "first_eligible";
            break;
        }
        for iterator in &iterators {
            iterator.status().map_err(storage_error)?;
        }
        let result = if let Some((knowledge, origin)) = selected {
            match origin {
                super::knowledge_origin::KnowledgeOriginDescriptor::MemoryOnly => {
                    KnowledgeSelectionOutcomeV4::Procedure { knowledge, origin }
                }
                super::knowledge_origin::KnowledgeOriginDescriptor::ExperienceMemory { .. } => {
                    KnowledgeSelectionOutcomeV4::ExperienceKnowledge { knowledge, origin }
                }
            }
        } else if stop == "index_exhausted" {
            KnowledgeSelectionOutcomeV4::Baseline {
                baseline_revision: context.payload.baseline_revision,
                reason: "no_eligible_bound_procedure".into(),
            }
        } else {
            KnowledgeSelectionOutcomeV4::Incomplete {
                baseline_revision: context.payload.baseline_revision,
                reason: stop.into(),
            }
        };
        Ok(KnowledgeSelectionV4 {
            selection_version: request.selection_version,
            accepted_origin_kinds: request.accepted_origin_kinds,
            evaluated_at_millis: view.observed_at_millis(),
            index_generation: self.inner.knowledge_generation,
            result,
            work,
            coverage: KnowledgeSelectionCoverageV3 {
                decision_complete: stop == "first_eligible" || stop == "index_exhausted",
                index_exhausted: stop == "index_exhausted",
                stop_reason: stop.into(),
            },
        })
    }
}
