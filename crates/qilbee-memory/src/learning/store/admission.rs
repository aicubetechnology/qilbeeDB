//! Durable admission outcomes without fabricated measurements.
use super::*;
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvaluationActor {
    pub subject_id: String,
    pub credential_id: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubmissionStatus {
    Complete,
    Rejected,
    Incomplete,
    Cancelled,
    PendingOrUnknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionOutcome {
    Accepted,
    Rejected,
    Incomplete,
    Cancelled,
    PendingOrUnknown,
    UnknownConsumption,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvaluationSubmission {
    pub case_id: String,
    pub phase: EvaluationPhase,
    pub policy_id: String,
    pub context_id: String,
    pub baseline_revision: String,
    pub evidence_ref: String,
    pub status: SubmissionStatus,
    pub baseline_utility: Option<f64>,
    pub candidate_utility: Option<f64>,
    pub candidate_cost_units: Option<u64>,
    pub candidate_latency_ms: Option<u64>,
    pub detail: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdmissionReceipt {
    pub schema_version: u32,
    pub tenant: String,
    pub namespace: String,
    pub procedure_id: String,
    pub actor: EvaluationActor,
    pub submission: EvaluationSubmission,
    pub outcome: AdmissionOutcome,
    pub reason: String,
    pub state_after: ProcedureState,
    pub evaluation: Option<EvaluationReceipt>,
    pub recorded_at_millis: i64,
}
fn admission_key(tenant: &str, namespace: &str, id: &str, case: &str) -> Vec<u8> {
    let mut key = vec![7];
    for part in [tenant, namespace, id, case] {
        append_component(&mut key, part);
    }
    key
}
impl EvaluationSubmission {
    fn validate(&self) -> Result<()> {
        for (name, value) in [
            ("case ID", &self.case_id),
            ("policy ID", &self.policy_id),
            ("context ID", &self.context_id),
            ("baseline revision", &self.baseline_revision),
        ] {
            validate_text(value, name, 512)?;
        }
        validate_text(&self.evidence_ref, "evidence reference", 2048)?;
        if let Some(detail) = &self.detail {
            validate_text(detail, "outcome detail", 2048)?;
        }
        if [self.baseline_utility, self.candidate_utility]
            .into_iter()
            .flatten()
            .any(|v| !v.is_finite())
        {
            return Err(Error::ValidationError(
                "Measurements must be finite or explicitly unknown".into(),
            ));
        }
        Ok(())
    }
}
impl LearningMemory {
    /// HTTP must authorize the scope and derive actor from the live credential.
    pub fn admit_evaluation(
        &self,
        tenant: &str,
        namespace: &str,
        id: &str,
        submission: EvaluationSubmission,
        actor: EvaluationActor,
    ) -> Result<AdmissionReceipt> {
        submission.validate()?;
        validate_text(&actor.subject_id, "evaluator subject", 512)?;
        validate_text(&actor.credential_id, "evaluator credential", 512)?;
        let _guard = self
            .inner
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Learning mutation lock poisoned".into()))?;
        let bound = self
            .registered_procedure(tenant, namespace, id)?
            .ok_or_else(|| Error::KeyNotFound("Registered procedure not found".into()))?;
        if actor.subject_id != bound.record.proposal.policy.evaluator_id {
            return Err(Error::Unauthorized(
                "Evaluator subject does not match the registered policy".into(),
            ));
        }
        if let Some(existing) = self.admission(tenant, namespace, id, &submission.case_id)? {
            return if existing.submission == submission
                && existing.actor.subject_id == actor.subject_id
            {
                Ok(existing)
            } else {
                Err(Error::ConstraintViolation(
                    "Evaluation case already has a different submission".into(),
                ))
            };
        }
        let mut batch = WriteBatch::default();
        let mut evaluation = None;
        let (outcome, reason) = if submission.policy_id != bound.receipt.request.policy_id
            || submission.context_id != bound.receipt.request.context_id
            || submission.baseline_revision != bound.record.proposal.baseline_revision
        {
            (AdmissionOutcome::Rejected, "contract_mismatch")
        } else {
            match submission.status {
                SubmissionStatus::Rejected => (AdmissionOutcome::Rejected, "evaluator_rejected"),
                SubmissionStatus::Incomplete => {
                    (AdmissionOutcome::Incomplete, "incomplete_evidence")
                }
                SubmissionStatus::Cancelled => {
                    (AdmissionOutcome::Cancelled, "evaluator_reported_cancelled")
                }
                SubmissionStatus::PendingOrUnknown => {
                    (AdmissionOutcome::PendingOrUnknown, "outcome_unknown")
                }
                SubmissionStatus::Complete => {
                    if submission.candidate_cost_units.is_none()
                        || submission.candidate_latency_ms.is_none()
                    {
                        (AdmissionOutcome::UnknownConsumption, "consumption_unknown")
                    } else if let (Some(baseline_utility), Some(candidate_utility)) =
                        (submission.baseline_utility, submission.candidate_utility)
                    {
                        let paired = PairedEvaluation {
                            case_id: submission.case_id.clone(),
                            phase: submission.phase,
                            evaluator_id: actor.subject_id.clone(),
                            evaluation_contract: bound
                                .record
                                .proposal
                                .policy
                                .evaluation_contract
                                .clone(),
                            evidence_ref: submission.evidence_ref.clone(),
                            baseline_utility,
                            candidate_utility,
                            candidate_cost_units: submission.candidate_cost_units.unwrap(),
                            candidate_latency_ms: submission.candidate_latency_ms.unwrap(),
                        };
                        match self.prepare_evaluation(&bound.record.scope, id, paired) {
                            Ok((result, prepared)) => {
                                batch = prepared.unwrap_or_default();
                                evaluation = Some(result.receipt);
                                (AdmissionOutcome::Accepted, "accepted")
                            }
                            Err(
                                Error::ValidationError(_)
                                | Error::ConstraintViolation(_)
                                | Error::MemoryOperation(_),
                            ) => (AdmissionOutcome::Rejected, "evidence_or_phase_rejected"),
                            Err(error) => return Err(error),
                        }
                    } else {
                        (AdmissionOutcome::Incomplete, "utility_unknown")
                    }
                }
            }
        };
        if evaluation.is_none()
            && self
                .evaluation(&bound.record.scope, id, &submission.case_id)?
                .is_some()
        {
            return Err(Error::ConstraintViolation(
                "A direct evaluation already occupies this case; admission cannot hide it".into(),
            ));
        }
        let receipt = AdmissionReceipt {
            schema_version: 1,
            tenant: tenant.into(),
            namespace: namespace.into(),
            procedure_id: id.into(),
            actor,
            submission,
            outcome,
            reason: reason.into(),
            state_after: evaluation
                .as_ref()
                .map_or(bound.record.state, |e| e.state_after),
            evaluation,
            recorded_at_millis: chrono::Utc::now().timestamp_millis(),
        };
        batch.put(
            admission_key(tenant, namespace, id, &receipt.submission.case_id),
            encode(&receipt)?,
        );
        self.inner
            .db
            .write_opt(batch, &write_options())
            .map_err(storage_error)?;
        Ok(receipt)
    }
    pub fn admission(
        &self,
        tenant: &str,
        namespace: &str,
        id: &str,
        case: &str,
    ) -> Result<Option<AdmissionReceipt>> {
        validate_text(case, "case ID", 512)?;
        let bound = self
            .registered_procedure(tenant, namespace, id)?
            .ok_or_else(|| Error::KeyNotFound("Registered procedure not found".into()))?;
        let Some(bytes) = self
            .inner
            .db
            .get(admission_key(tenant, namespace, id, case))
            .map_err(storage_error)?
        else {
            return Ok(None);
        };
        let receipt: AdmissionReceipt = decode(&bytes)?;
        if receipt.schema_version != 1
            || receipt.tenant != tenant
            || receipt.namespace != namespace
            || receipt.procedure_id != id
            || receipt.submission.case_id != case
            || (receipt.outcome == AdmissionOutcome::Accepted) != receipt.evaluation.is_some()
            || self.evaluation(&bound.record.scope, id, case)? != receipt.evaluation
        {
            return Err(Error::DataCorruption(
                "Admission identity, version or evaluation mismatch".into(),
            ));
        }
        receipt.verify_contract(&bound)?;
        Ok(Some(receipt))
    }
}
impl AdmissionReceipt {
    fn verify_contract(&self, bound: &bound::RegisteredProcedure) -> Result<()> {
        let corrupt = || {
            Error::DataCorruption(
                "Admission actor, submission or historical outcome is inconsistent".into(),
            )
        };
        self.submission.validate().map_err(|_| corrupt())?;
        validate_text(&self.actor.subject_id, "evaluator subject", 512).map_err(|_| corrupt())?;
        validate_text(&self.actor.credential_id, "evaluator credential", 512)
            .map_err(|_| corrupt())?;
        if self.actor.subject_id != bound.record.proposal.policy.evaluator_id {
            return Err(corrupt());
        }
        let s = &self.submission;
        let expected = if s.policy_id != bound.receipt.request.policy_id
            || s.context_id != bound.receipt.request.context_id
            || s.baseline_revision != bound.record.proposal.baseline_revision
        {
            (AdmissionOutcome::Rejected, "contract_mismatch")
        } else {
            match s.status {
                SubmissionStatus::Rejected => (AdmissionOutcome::Rejected, "evaluator_rejected"),
                SubmissionStatus::Incomplete => {
                    (AdmissionOutcome::Incomplete, "incomplete_evidence")
                }
                SubmissionStatus::Cancelled => {
                    (AdmissionOutcome::Cancelled, "evaluator_reported_cancelled")
                }
                SubmissionStatus::PendingOrUnknown => {
                    (AdmissionOutcome::PendingOrUnknown, "outcome_unknown")
                }
                SubmissionStatus::Complete
                    if s.candidate_cost_units.is_none() || s.candidate_latency_ms.is_none() =>
                {
                    (AdmissionOutcome::UnknownConsumption, "consumption_unknown")
                }
                SubmissionStatus::Complete
                    if s.baseline_utility.is_none() || s.candidate_utility.is_none() =>
                {
                    (AdmissionOutcome::Incomplete, "utility_unknown")
                }
                SubmissionStatus::Complete if self.evaluation.is_some() => {
                    (AdmissionOutcome::Accepted, "accepted")
                }
                SubmissionStatus::Complete => {
                    (AdmissionOutcome::Rejected, "evidence_or_phase_rejected")
                }
            }
        };
        if (self.outcome, self.reason.as_str()) != expected {
            return Err(corrupt());
        }
        if let Some(e) = &self.evaluation {
            e.evaluation.validate().map_err(|_| corrupt())?;
            let p = &e.evaluation;
            if s.status != SubmissionStatus::Complete
                || self.state_after != e.state_after
                || p.case_id != s.case_id
                || p.phase != s.phase
                || p.evaluator_id != self.actor.subject_id
                || p.evaluation_contract != bound.record.proposal.policy.evaluation_contract
                || p.evidence_ref != s.evidence_ref
                || Some(p.baseline_utility) != s.baseline_utility
                || Some(p.candidate_utility) != s.candidate_utility
                || Some(p.candidate_cost_units) != s.candidate_cost_units
                || Some(p.candidate_latency_ms) != s.candidate_latency_ms
            {
                return Err(corrupt());
            }
        }
        // A rejected submission has no paired transition receipt. Its historical
        // state must not be compared with the procedure's later current state.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::learning::*;
    fn setup(db: &LearningMemory) {
        db.register_policy(
            "tenant",
            "policy",
            PolicyDefinition {
                algorithm: PolicyAlgorithm::FixedBudgetHoeffdingV1,
                parameters: LearningPolicy {
                    evaluator_id: "evaluator".into(),
                    evaluation_contract: "rubric".into(),
                    qualification_trials: 32,
                    ..Default::default()
                },
            },
            "admin",
        )
        .unwrap();
        db.register_context(
            "tenant",
            "context",
            EvaluationContext {
                task: "task".into(),
                baseline_revision: "baseline".into(),
                model_provider: "provider".into(),
                model_revision: "model".into(),
                tools: Default::default(),
                environment_revision: "environment".into(),
                evaluation_contract: "rubric".into(),
                dataset_revision: "dataset".into(),
                harness_revision: "harness".into(),
                permissions_revision: "permissions".into(),
            },
            "admin",
        )
        .unwrap();
        db.propose_registered(
            "tenant",
            "namespace",
            RegisteredProposal {
                id: "candidate".into(),
                policy_id: "policy".into(),
                context_id: "context".into(),
                instructions: "Check evidence".into(),
                source_refs: vec!["training".into()],
            },
            "proposer",
        )
        .unwrap();
    }
    fn actor() -> EvaluationActor {
        EvaluationActor {
            subject_id: "evaluator".into(),
            credential_id: "credential-1".into(),
        }
    }
    fn submission(case: &str) -> EvaluationSubmission {
        EvaluationSubmission {
            case_id: case.into(),
            phase: EvaluationPhase::Qualification,
            policy_id: "policy".into(),
            context_id: "context".into(),
            baseline_revision: "baseline".into(),
            evidence_ref: format!("held-out:{case}"),
            status: SubmissionStatus::Complete,
            baseline_utility: Some(0.0),
            candidate_utility: Some(1.0),
            candidate_cost_units: Some(1),
            candidate_latency_ms: Some(1),
            detail: None,
        }
    }
    #[test]
    fn admission_refuses_to_hide_an_existing_direct_evaluation() {
        let dir = tempfile::TempDir::new().unwrap();
        let db = LearningMemory::open(dir.path()).unwrap();
        setup(&db);
        let scope = db
            .registered_procedure("tenant", "namespace", "candidate")
            .unwrap()
            .unwrap()
            .record
            .scope;
        db.record_evaluation(
            &scope,
            "candidate",
            PairedEvaluation {
                case_id: "direct".into(),
                phase: EvaluationPhase::Qualification,
                evaluator_id: "evaluator".into(),
                evaluation_contract: "rubric".into(),
                evidence_ref: "direct-evidence".into(),
                baseline_utility: 0.0,
                candidate_utility: 1.0,
                candidate_cost_units: 1,
                candidate_latency_ms: 1,
            },
        )
        .unwrap();
        let mut request = submission("direct");
        request.status = SubmissionStatus::Incomplete;
        assert!(matches!(
            db.admit_evaluation("tenant", "namespace", "candidate", request, actor()),
            Err(qilbee_core::Error::ConstraintViolation(_))
        ));
        assert!(
            db.admission("tenant", "namespace", "candidate", "direct")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn admission_unknown_and_incomplete_outcomes_never_inflate_evidence() {
        let dir = tempfile::TempDir::new().unwrap();
        let db = LearningMemory::open(dir.path()).unwrap();
        setup(&db);
        for (n, status, expected) in [
            (
                0,
                SubmissionStatus::Incomplete,
                AdmissionOutcome::Incomplete,
            ),
            (1, SubmissionStatus::Rejected, AdmissionOutcome::Rejected),
            (2, SubmissionStatus::Cancelled, AdmissionOutcome::Cancelled),
            (
                3,
                SubmissionStatus::PendingOrUnknown,
                AdmissionOutcome::PendingOrUnknown,
            ),
            (
                4,
                SubmissionStatus::Complete,
                AdmissionOutcome::UnknownConsumption,
            ),
        ] {
            let mut request = submission(&n.to_string());
            request.status = status;
            request.candidate_cost_units = None;
            let receipt = db
                .admit_evaluation("tenant", "namespace", "candidate", request.clone(), actor())
                .unwrap();
            assert_eq!(receipt.outcome, expected);
            assert_eq!(receipt.submission.candidate_cost_units, None);
            assert!(receipt.evaluation.is_none());
            assert_eq!(
                db.admission("tenant", "namespace", "candidate", &request.case_id)
                    .unwrap(),
                Some(receipt)
            );
        }
        assert_eq!(
            db.registered_procedure("tenant", "namespace", "candidate")
                .unwrap()
                .unwrap()
                .record
                .qualification_count,
            0
        );
    }
    #[test]
    fn admission_rejects_mismatched_evidence_and_unauthorized_evaluator() {
        let dir = tempfile::TempDir::new().unwrap();
        let db = LearningMemory::open(dir.path()).unwrap();
        setup(&db);
        let mut wrong = actor();
        wrong.subject_id = "proposer".into();
        assert!(matches!(
            db.admit_evaluation(
                "tenant",
                "namespace",
                "candidate",
                submission("wrong-actor"),
                wrong
            ),
            Err(qilbee_core::Error::Unauthorized(_))
        ));
        for (n, field) in ["context", "policy", "baseline", "training"]
            .iter()
            .enumerate()
        {
            let mut request = submission(&n.to_string());
            match *field {
                "context" => request.context_id = "other".into(),
                "policy" => request.policy_id = "other".into(),
                "baseline" => request.baseline_revision = "other".into(),
                _ => request.evidence_ref = "training".into(),
            }
            assert_eq!(
                db.admit_evaluation("tenant", "namespace", "candidate", request, actor())
                    .unwrap()
                    .outcome,
                AdmissionOutcome::Rejected
            );
        }
        assert_eq!(
            db.registered_procedure("tenant", "namespace", "candidate")
                .unwrap()
                .unwrap()
                .record
                .qualification_count,
            0
        );
    }
    #[test]
    fn admission_and_decision_replay_remain_atomic_and_durable() {
        let dir = tempfile::TempDir::new().unwrap();
        let db = LearningMemory::open(dir.path()).unwrap();
        setup(&db);
        let first = db
            .admit_evaluation("tenant", "namespace", "candidate", submission("0"), actor())
            .unwrap();
        assert_eq!(first.outcome, AdmissionOutcome::Accepted);
        for n in 1..32 {
            db.admit_evaluation(
                "tenant",
                "namespace",
                "candidate",
                submission(&n.to_string()),
                actor(),
            )
            .unwrap();
        }
        assert_eq!(
            db.registered_procedure("tenant", "namespace", "candidate")
                .unwrap()
                .unwrap()
                .record
                .state,
            ProcedureState::Active
        );
        assert_eq!(
            db.admit_evaluation("tenant", "namespace", "candidate", submission("0"), actor())
                .unwrap(),
            first
        );
        let mut changed = submission("0");
        changed.candidate_utility = Some(0.0);
        assert!(
            db.admit_evaluation("tenant", "namespace", "candidate", changed, actor())
                .is_err()
        );
        drop(db);
        let db = LearningMemory::open(dir.path()).unwrap();
        assert_eq!(
            db.admission("tenant", "namespace", "candidate", "0")
                .unwrap(),
            Some(first)
        );
        assert_eq!(
            db.registered_procedure("tenant", "namespace", "candidate")
                .unwrap()
                .unwrap()
                .record
                .qualification_count,
            32
        );
    }
}
