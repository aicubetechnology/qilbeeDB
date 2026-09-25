//! Atomic proposal bindings to immutable administrative contracts.
use super::registry::{EvaluationContext, PolicyDefinition, RegistryEntry, digest};
use super::*;
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegisteredProposal {
    pub id: String,
    pub policy_id: String,
    pub context_id: String,
    pub instructions: String,
    pub source_refs: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposalReceipt {
    pub schema_version: u32,
    pub tenant: String,
    pub namespace: String,
    pub request: RegisteredProposal,
    pub policy_digest: String,
    pub context_digest: String,
    pub actor: String,
    /// Original candidate state. Subsequent reads expose current state separately.
    pub record: ProcedureRecord,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegisteredProcedure {
    pub receipt: ProposalReceipt,
    pub record: ProcedureRecord,
}
pub(super) fn binding_key(tenant: &str, namespace: &str, id: &str) -> Vec<u8> {
    let mut key = vec![6];
    for part in [tenant, namespace, id] {
        append_component(&mut key, part);
    }
    key
}
fn validate_namespace(tenant: &str, namespace: &str) -> Result<()> {
    validate_text(tenant, "tenant", 512)?;
    validate_text(namespace, "authorized namespace", 4096)
}
impl LearningMemory {
    /// Select while holding the learning mutation lock so current binding and state agree.
    pub fn select_registered(
        &self,
        tenant: &str,
        namespace: &str,
        policy_id: &str,
        context_id: &str,
        max_instruction_bytes: usize,
    ) -> Result<Option<RegisteredProcedure>> {
        let _guard = self
            .inner
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Learning mutation lock poisoned".into()))?;
        let mut budget = experience_withdrawal::ExperienceReuseBudget::new(4096, 16_777_216);
        let (policy, context) =
            self.contracts_budgeted(tenant, namespace, policy_id, context_id, &mut budget)?;
        let scope = Self::bound_scope(tenant, namespace, &policy, &context)?;
        self.select_locked(
            &scope,
            &context.payload.task,
            &context.payload.baseline_revision,
            &context.payload.evaluation_contract,
            max_instruction_bytes,
            &mut budget,
        )?
        .map(|selected| {
            let bound = self
                .registered_procedure_budgeted(
                    tenant,
                    namespace,
                    &selected.proposal.id,
                    &mut budget,
                )?
                .ok_or_else(|| {
                    Error::DataCorruption(
                        "Selected procedure is missing its immutable binding".into(),
                    )
                })?;
            if bound.record != selected {
                return Err(Error::DataCorruption(
                    "Selected procedure differs from its registered contract".into(),
                ));
            }
            Ok(bound)
        })
        .transpose()
    }

    /// Derive an exact compatible partition. Namespace must come from authorization.
    pub fn registered_scope(
        &self,
        tenant: &str,
        namespace: &str,
        policy_id: &str,
        context_id: &str,
    ) -> Result<LearningScope> {
        let (policy, context) = self.contracts(tenant, namespace, policy_id, context_id)?;
        Self::bound_scope(tenant, namespace, &policy, &context)
    }
    pub(super) fn contracts(
        &self,
        tenant: &str,
        namespace: &str,
        policy_id: &str,
        context_id: &str,
    ) -> Result<(
        RegistryEntry<PolicyDefinition>,
        RegistryEntry<EvaluationContext>,
    )> {
        validate_namespace(tenant, namespace)?;
        let policy = self
            .policy(tenant, policy_id)?
            .ok_or_else(|| Error::KeyNotFound("Registered policy not found".into()))?;
        let context = self
            .context(tenant, context_id)?
            .ok_or_else(|| Error::KeyNotFound("Registered context not found".into()))?;
        if policy.payload.parameters.evaluation_contract != context.payload.evaluation_contract {
            return Err(Error::ValidationError(
                "Policy and context evaluation contracts differ".into(),
            ));
        }
        Ok((policy, context))
    }
    pub(super) fn bound_scope(
        tenant: &str,
        namespace: &str,
        policy: &RegistryEntry<PolicyDefinition>,
        context: &RegistryEntry<EvaluationContext>,
    ) -> Result<LearningScope> {
        Ok(LearningScope {
            tenant: tenant.into(),
            agent: format!("scope-sha256:{}", digest(&namespace)?),
            environment: format!(
                "contract-sha256:{}",
                digest(&(
                    &policy.id,
                    &policy.payload_digest,
                    &context.id,
                    &context.payload_digest
                ))?
            ),
        })
    }
    /// Store original binding receipt and candidate in one synchronous WAL batch.
    pub fn propose_registered(
        &self,
        tenant: &str,
        namespace: &str,
        request: RegisteredProposal,
        actor: &str,
    ) -> Result<ProposalReceipt> {
        super::knowledge::reject_reserved_legacy_sources(&request.source_refs)?;
        validate_namespace(tenant, namespace)?;
        validate_text(&request.id, "procedure ID", 512)?;
        validate_text(actor, "proposal actor", 512)?;
        let _guard = self
            .inner
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Learning mutation lock poisoned".into()))?;
        if let Some(existing) = self.registered_procedure(tenant, namespace, &request.id)? {
            return if existing.receipt.request == request {
                Ok(existing.receipt)
            } else {
                Err(Error::ConstraintViolation(
                    "Procedure binding is immutable; use a new ID".into(),
                ))
            };
        }
        let receipt = self.prepare_registered_proposal(tenant, namespace, request, actor)?;
        let mut batch = WriteBatch::default();
        Self::put_registered_proposal(&mut batch, &receipt)?;
        self.inner
            .db
            .write_opt(batch, &write_options())
            .map_err(storage_error)?;
        Ok(receipt)
    }
    // The caller holds the shared mutation lock and has checked receipt identity.
    pub(super) fn prepare_registered_proposal(
        &self,
        tenant: &str,
        namespace: &str,
        request: RegisteredProposal,
        actor: &str,
    ) -> Result<ProposalReceipt> {
        validate_namespace(tenant, namespace)?;
        validate_text(&request.id, "procedure ID", 512)?;
        validate_text(actor, "proposal actor", 512)?;
        let (policy, context) =
            self.contracts(tenant, namespace, &request.policy_id, &request.context_id)?;
        let scope = Self::bound_scope(tenant, namespace, &policy, &context)?;
        let proposal = ProcedureProposal {
            id: request.id.clone(),
            task: context.payload.task,
            baseline_revision: context.payload.baseline_revision,
            instructions: request.instructions.clone(),
            source_refs: request.source_refs.clone(),
            policy: policy.payload.parameters,
        };
        proposal.validate()?;
        if self.get(&scope, &request.id)?.is_some() {
            return Err(Error::DataCorruption(
                "Procedure exists without its registration receipt".into(),
            ));
        }
        let record = new_procedure_record(&scope, proposal);
        Ok(ProposalReceipt {
            schema_version: 1,
            tenant: tenant.into(),
            namespace: namespace.into(),
            request,
            policy_digest: policy.payload_digest,
            context_digest: context.payload_digest,
            actor: actor.into(),
            record,
        })
    }
    pub(super) fn put_registered_proposal(
        batch: &mut WriteBatch,
        receipt: &ProposalReceipt,
    ) -> Result<()> {
        batch.put(
            procedure_key(&receipt.record.scope, &receipt.request.id),
            encode(&receipt.record)?,
        );
        batch.put(
            binding_key(&receipt.tenant, &receipt.namespace, &receipt.request.id),
            encode(receipt)?,
        );
        Ok(())
    }
    pub fn registered_procedure(
        &self,
        tenant: &str,
        namespace: &str,
        id: &str,
    ) -> Result<Option<RegisteredProcedure>> {
        validate_namespace(tenant, namespace)?;
        validate_text(id, "procedure ID", 512)?;
        let Some(bytes) = self
            .inner
            .db
            .get(binding_key(tenant, namespace, id))
            .map_err(storage_error)?
        else {
            return Ok(None);
        };
        let receipt: ProposalReceipt = decode(&bytes)?;
        let (policy, context) = self
            .contracts(
                tenant,
                namespace,
                &receipt.request.policy_id,
                &receipt.request.context_id,
            )
            .map_err(|_| {
                Error::DataCorruption(
                    "Registered procedure contracts are missing or inconsistent".into(),
                )
            })?;
        let scope = Self::bound_scope(tenant, namespace, &policy, &context)?;
        let record = self
            .get(&scope, id)?
            .ok_or_else(|| Error::DataCorruption("Registered procedure is missing".into()))?;
        Self::validate_registered_binding(
            tenant, namespace, id, &receipt, &record, &policy, &context,
        )?;
        Ok(Some(RegisteredProcedure { receipt, record }))
    }
    pub(super) fn validate_registered_binding(
        tenant: &str,
        namespace: &str,
        id: &str,
        receipt: &ProposalReceipt,
        record: &ProcedureRecord,
        policy: &RegistryEntry<PolicyDefinition>,
        context: &RegistryEntry<EvaluationContext>,
    ) -> Result<()> {
        let scope = Self::bound_scope(tenant, namespace, &policy, &context)?;
        let expected = ProcedureProposal {
            id: id.into(),
            task: context.payload.task.clone(),
            baseline_revision: context.payload.baseline_revision.clone(),
            instructions: receipt.request.instructions.clone(),
            source_refs: receipt.request.source_refs.clone(),
            policy: policy.payload.parameters.clone(),
        };
        if receipt.schema_version != 1
            || receipt.tenant != tenant
            || receipt.namespace != namespace
            || receipt.request.id != id
            || receipt.policy_digest != policy.payload_digest
            || receipt.context_digest != context.payload_digest
            || receipt.record.scope != scope
            || receipt.record.proposal != expected
            || receipt.record.state != ProcedureState::Candidate
        {
            return Err(Error::DataCorruption(
                "Procedure binding identity, version or contract mismatch".into(),
            ));
        }
        if record.scope != scope
            || record.proposal != receipt.record.proposal
            || record.created_at_millis != receipt.record.created_at_millis
        {
            return Err(Error::DataCorruption(
                "Procedure differs from its immutable binding".into(),
            ));
        }
        Ok(())
    }
}

impl LearningMemory {
    pub(super) fn contracts_budgeted(
        &self,
        tenant: &str,
        namespace: &str,
        policy_id: &str,
        context_id: &str,
        budget: &mut experience_withdrawal::ExperienceReuseBudget,
    ) -> Result<(
        RegistryEntry<PolicyDefinition>,
        RegistryEntry<EvaluationContext>,
    )> {
        validate_namespace(tenant, namespace)?;
        let policy: RegistryEntry<PolicyDefinition> =
            self.registry_budgeted(4, tenant, policy_id, budget)?;
        let context: RegistryEntry<EvaluationContext> =
            self.registry_budgeted(5, tenant, context_id, budget)?;
        if policy.payload.parameters.evaluation_contract != context.payload.evaluation_contract {
            return Err(Error::ValidationError("Policy and context differ".into()));
        }
        Ok((policy, context))
    }
    pub(super) fn registry_budgeted<T: Serialize + DeserializeOwned>(
        &self,
        kind: u8,
        tenant: &str,
        id: &str,
        budget: &mut experience_withdrawal::ExperienceReuseBudget,
    ) -> Result<RegistryEntry<T>> {
        validate_text(id, "registry ID", 512)?;
        let entry: RegistryEntry<T> = budget
            .read(self, registry::registry_key(kind, tenant, id))?
            .ok_or_else(|| Error::KeyNotFound("Registered contract".into()))?;
        if entry.schema_version != 1
            || entry.tenant != tenant
            || entry.id != id
            || entry.payload_digest != digest(&entry.payload)?
        {
            return Err(Error::DataCorruption("Invalid registered contract".into()));
        }
        Ok(entry)
    }
    pub(super) fn registered_procedure_budgeted(
        &self,
        tenant: &str,
        namespace: &str,
        id: &str,
        budget: &mut experience_withdrawal::ExperienceReuseBudget,
    ) -> Result<Option<RegisteredProcedure>> {
        validate_namespace(tenant, namespace)?;
        validate_text(id, "procedure ID", 512)?;
        let Some(receipt): Option<ProposalReceipt> =
            budget.read(self, binding_key(tenant, namespace, id))?
        else {
            return Ok(None);
        };
        let (policy, context) = self.contracts_budgeted(
            tenant,
            namespace,
            &receipt.request.policy_id,
            &receipt.request.context_id,
            budget,
        )?;
        let scope = Self::bound_scope(tenant, namespace, &policy, &context)?;
        let record: ProcedureRecord = budget
            .read(self, procedure_key(&scope, id))?
            .ok_or_else(|| Error::DataCorruption("Registered procedure missing".into()))?;
        Self::validate_registered_binding(
            tenant, namespace, id, &receipt, &record, &policy, &context,
        )?;
        Ok(Some(RegisteredProcedure { receipt, record }))
    }
    pub(super) fn prepare_registered_proposal_budgeted(
        &self,
        tenant: &str,
        namespace: &str,
        request: RegisteredProposal,
        actor: &str,
        budget: &mut experience_withdrawal::ExperienceReuseBudget,
    ) -> Result<ProposalReceipt> {
        validate_namespace(tenant, namespace)?;
        validate_text(&request.id, "procedure ID", 512)?;
        validate_text(actor, "proposal actor", 512)?;
        let (policy, context) = self.contracts_budgeted(
            tenant,
            namespace,
            &request.policy_id,
            &request.context_id,
            budget,
        )?;
        let scope = Self::bound_scope(tenant, namespace, &policy, &context)?;
        let proposal = ProcedureProposal {
            id: request.id.clone(),
            task: context.payload.task,
            baseline_revision: context.payload.baseline_revision,
            instructions: request.instructions.clone(),
            source_refs: request.source_refs.clone(),
            policy: policy.payload.parameters,
        };
        proposal.validate()?;
        if budget
            .read::<ProcedureRecord>(self, procedure_key(&scope, &request.id))?
            .is_some()
        {
            return Err(Error::DataCorruption(
                "Procedure exists without binding".into(),
            ));
        }
        Ok(ProposalReceipt {
            schema_version: 1,
            tenant: tenant.into(),
            namespace: namespace.into(),
            request,
            policy_digest: policy.payload_digest,
            context_digest: context.payload_digest,
            actor: actor.into(),
            record: new_procedure_record(&scope, proposal),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::learning::*;
    use std::{
        collections::BTreeMap,
        sync::{Arc, Barrier},
    };
    use tempfile::TempDir;
    fn setup(db: &LearningMemory, tenant: &str) {
        db.register_policy(
            tenant,
            "policy-v1",
            PolicyDefinition {
                algorithm: PolicyAlgorithm::FixedBudgetHoeffdingV1,
                parameters: LearningPolicy {
                    evaluator_id: "evaluator".into(),
                    evaluation_contract: "contract-v1".into(),
                    ..Default::default()
                },
            },
            "admin",
        )
        .unwrap();
        db.register_context(
            tenant,
            "context-v1",
            EvaluationContext {
                task: "task".into(),
                baseline_revision: "baseline".into(),
                model_provider: "provider".into(),
                model_revision: "model".into(),
                tools: BTreeMap::new(),
                environment_revision: "env".into(),
                evaluation_contract: "contract-v1".into(),
                dataset_revision: "data".into(),
                harness_revision: "harness".into(),
                permissions_revision: "permissions".into(),
            },
            "admin",
        )
        .unwrap();
    }
    fn input() -> RegisteredProposal {
        RegisteredProposal {
            id: "candidate".into(),
            policy_id: "policy-v1".into(),
            context_id: "context-v1".into(),
            instructions: "Check source evidence.".into(),
            source_refs: vec!["training-trace".into()],
        }
    }
    #[test]
    fn bound_proposal_resolves_authority_and_preserves_receipt_after_restart() {
        let dir = TempDir::new().unwrap();
        let db = LearningMemory::open(dir.path()).unwrap();
        setup(&db, "tenant");
        let receipt = db
            .propose_registered("tenant", "namespace", input(), "proposer")
            .unwrap();
        assert_eq!(receipt.record.proposal.policy.evaluator_id, "evaluator");
        assert_eq!(receipt.record.proposal.baseline_revision, "baseline");
        assert_eq!(
            receipt,
            db.propose_registered("tenant", "namespace", input(), "other-proposer")
                .unwrap()
        );
        assert!(
            db.registered_procedure("tenant", "other", "candidate")
                .unwrap()
                .is_none()
        );
        assert!(
            db.registered_procedure("other", "namespace", "candidate")
                .unwrap()
                .is_none()
        );
        drop(db);
        let db = LearningMemory::open(dir.path()).unwrap();
        assert_eq!(
            db.registered_procedure("tenant", "namespace", "candidate")
                .unwrap()
                .unwrap()
                .receipt,
            receipt
        );
    }
    #[test]
    fn bound_proposal_rejects_missing_mismatched_and_changed_contracts() {
        let dir = TempDir::new().unwrap();
        let db = LearningMemory::open(dir.path()).unwrap();
        setup(&db, "tenant");
        assert!(
            db.propose_registered("other", "namespace", input(), "proposer")
                .is_err()
        );
        let mut c = db.context("tenant", "context-v1").unwrap().unwrap().payload;
        c.evaluation_contract = "other-contract".into();
        db.register_context("tenant", "wrong", c, "admin").unwrap();
        let mut bad = input();
        bad.context_id = "wrong".into();
        assert!(
            db.propose_registered("tenant", "namespace", bad, "proposer")
                .is_err()
        );
        db.propose_registered("tenant", "namespace", input(), "proposer")
            .unwrap();
        let mut bad = input();
        bad.instructions = "Changed instructions".into();
        assert!(
            db.propose_registered("tenant", "namespace", bad, "proposer")
                .is_err()
        );
        let c = db.context("tenant", "context-v1").unwrap().unwrap().payload;
        db.register_context("tenant", "same-content-new-id", c, "admin")
            .unwrap();
        let mut bad = input();
        bad.context_id = "same-content-new-id".into();
        assert!(
            db.propose_registered("tenant", "namespace", bad, "proposer")
                .is_err()
        );
    }
    #[test]
    fn bound_proposal_detects_a_missing_or_modified_candidate() {
        let dir = TempDir::new().unwrap();
        let db = LearningMemory::open(dir.path()).unwrap();
        setup(&db, "tenant");
        let receipt = db
            .propose_registered("tenant", "namespace", input(), "proposer")
            .unwrap();
        let key = super::super::procedure_key(&receipt.record.scope, "candidate");
        let mut altered = receipt.record.clone();
        altered.proposal.instructions = "replaced".into();
        db.inner
            .db
            .put(&key, super::super::encode(&altered).unwrap())
            .unwrap();
        assert!(matches!(
            db.registered_procedure("tenant", "namespace", "candidate"),
            Err(qilbee_core::Error::DataCorruption(_))
        ));
        db.inner.db.delete(key).unwrap();
        assert!(matches!(
            db.registered_procedure("tenant", "namespace", "candidate"),
            Err(qilbee_core::Error::DataCorruption(_))
        ));
    }

    #[test]
    fn bound_selection_rejects_a_direct_record_from_a_different_contract() {
        let dir = TempDir::new().unwrap();
        let db = LearningMemory::open(dir.path()).unwrap();
        setup(&db, "tenant");
        let receipt = db
            .propose_registered("tenant", "namespace", input(), "proposer")
            .unwrap();
        let mut context = db.context("tenant", "context-v1").unwrap().unwrap().payload;
        context.model_revision = "model-v2".into();
        db.register_context("tenant", "context-v2", context, "admin")
            .unwrap();
        let scope = db
            .registered_scope("tenant", "namespace", "policy-v1", "context-v2")
            .unwrap();
        db.propose(&scope, receipt.record.proposal).unwrap();
        for n in 0..128 {
            db.record_evaluation(
                &scope,
                "candidate",
                PairedEvaluation {
                    case_id: n.to_string(),
                    phase: EvaluationPhase::Qualification,
                    evaluator_id: "evaluator".into(),
                    evaluation_contract: "contract-v1".into(),
                    evidence_ref: format!("evaluation-{n}"),
                    baseline_utility: 0.0,
                    candidate_utility: 1.0,
                    candidate_cost_units: 1,
                    candidate_latency_ms: 1,
                },
            )
            .unwrap();
        }
        assert!(matches!(
            db.select_registered("tenant", "namespace", "policy-v1", "context-v2", 4096),
            Err(qilbee_core::Error::DataCorruption(_))
        ));
    }

    #[test]
    fn bound_concurrent_proposals_have_one_original_receipt() {
        let dir = TempDir::new().unwrap();
        let db = LearningMemory::open(dir.path()).unwrap();
        setup(&db, "tenant");
        let barrier = Arc::new(Barrier::new(4));
        let threads: Vec<_> = (0..4)
            .map(|n| {
                let db = db.clone();
                let b = barrier.clone();
                std::thread::spawn(move || {
                    b.wait();
                    db.propose_registered("tenant", "namespace", input(), &format!("actor-{n}"))
                        .unwrap()
                })
            })
            .collect();
        let results: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
        assert!(results.iter().all(|r| r == &results[0]));
    }
}
