use super::*;
use crate::learning::*;
use std::{
    collections::BTreeMap,
    sync::{Arc, Barrier},
};
use tempfile::TempDir;

fn fixture(store: &LearningMemory) -> StrategyCandidateRequest {
    store
        .register_policy(
            "tenant",
            "policy",
            PolicyDefinition {
                algorithm: PolicyAlgorithm::FixedBudgetHoeffdingV1,
                parameters: LearningPolicy {
                    qualification_trials: 32,
                    max_failure_streak: 1,
                    evaluator_id: "judge".into(),
                    evaluation_contract: "rubric".into(),
                    ..Default::default()
                },
            },
            "admin",
        )
        .unwrap();
    let context = store
        .register_context(
            "tenant",
            "context",
            EvaluationContext {
                task: "task".into(),
                baseline_revision: "baseline".into(),
                model_provider: "external".into(),
                model_revision: "model-v1".into(),
                tools: BTreeMap::new(),
                environment_revision: "env-v1".into(),
                evaluation_contract: "rubric".into(),
                dataset_revision: "partitioned-data-v1".into(),
                harness_revision: "harness-v1".into(),
                permissions_revision: "scope-v1".into(),
            },
            "admin",
        )
        .unwrap();
    let actor = ExperienceActor {
        subject_id: "observer".into(),
        credential_id: "reporter-key".into(),
    };
    let mut events = Vec::new();
    for (id, outcome) in [
        ("success", ExperienceOutcome::Succeeded),
        ("failure", ExperienceOutcome::Failed),
        ("unknown", ExperienceOutcome::Unknown),
    ] {
        store
            .create_experience(
                "tenant",
                "scope",
                ExperienceRequest {
                    id: id.into(),
                    context_id: "context".into(),
                    reporter_subject_id: "observer".into(),
                    accounting_unit: "tokens".into(),
                    input: ExperienceEvidence {
                        reference: format!("fixture:input:{id}"),
                        sha256: "a".repeat(64),
                    },
                    parent: None,
                },
                actor.clone(),
            )
            .unwrap();
        let event = store
            .observe_experience(
                "tenant",
                "scope",
                id,
                ExperienceCommand {
                    event_id: "observed".into(),
                    expected_revision: 1,
                    context_digest: context.payload_digest.clone(),
                    outcome,
                    evidence: ExperienceEvidence {
                        reference: format!("fixture:evidence:{id}"),
                        sha256: "b".repeat(64),
                    },
                    cost_units: None,
                    latency_ms: Some(10),
                },
                actor.clone(),
            )
            .unwrap();
        events.push(ExperienceExportRef {
            attempt_id: id.into(),
            event_id: "observed".into(),
            event_digest: event.event_digest,
        });
    }
    StrategyCandidateRequest {
        id: "strategy".into(),
        policy_id: "policy".into(),
        context_id: "context".into(),
        instructions: "Check a reported failure against the current evidence before retrying."
            .into(),
        preconditions: vec!["The external operation exposes a status receipt.".into()],
        counterexamples: vec!["An unknown response does not prove failure.".into()],
        extractor: StrategyExtractor {
            provider: "external".into(),
            model: "extractor".into(),
            model_revision: "v1".into(),
            prompt_revision: "v1".into(),
            evidence_ref: "fixture:extraction".into(),
        },
        selection: ExperienceExportRequest {
            context_digest: context.payload_digest,
            accounting_unit: "tokens".into(),
            events,
        },
    }
}

#[test]
fn strategy_pins_failure_and_unknown_evidence_atomically_and_survives_reopen() {
    let dir = TempDir::new().unwrap();
    let store = LearningMemory::open(dir.path()).unwrap();
    let request = fixture(&store);
    let receipt = store
        .propose_strategy("tenant", "scope", request.clone(), "extractor-key")
        .unwrap();
    assert_eq!(receipt.summary.succeeded, 1);
    assert_eq!(receipt.summary.failed, 1);
    assert_eq!(receipt.summary.unknown, 1);
    assert_eq!(receipt.summary.cost_units.reported_total, None);
    assert_eq!(receipt.proposal.record.state, ProcedureState::Candidate);
    assert_eq!(receipt.proposal.record.qualification_count, 0);
    assert!(
        store
            .select_registered("tenant", "scope", "policy", "context", 64 * 1024)
            .unwrap()
            .is_none()
    );
    let mut command = store
        .experience_event("tenant", "scope", "unknown", "observed")
        .unwrap()
        .unwrap()
        .command;
    command.event_id = "completed-later".into();
    command.expected_revision = 2;
    command.outcome = ExperienceOutcome::Succeeded;
    command.cost_units = Some(7);
    store
        .observe_experience(
            "tenant",
            "scope",
            "unknown",
            command,
            ExperienceActor {
                subject_id: "observer".into(),
                credential_id: "new-key".into(),
            },
        )
        .unwrap();
    assert_eq!(
        receipt,
        store
            .propose_strategy("tenant", "scope", request.clone(), "rotated-extractor")
            .unwrap()
    );
    let mut changed = request.clone();
    changed.counterexamples.push("Another condition.".into());
    assert!(
        store
            .propose_strategy("tenant", "scope", changed, "actor")
            .is_err()
    );
    assert!(
        store
            .strategy_candidate("other", "scope", "strategy")
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .strategy_candidate("tenant", "private:other", "strategy")
            .unwrap()
            .is_none()
    );
    drop(store);
    let store = LearningMemory::open(dir.path()).unwrap();
    assert_eq!(
        Some(receipt),
        store
            .strategy_candidate("tenant", "scope", "strategy")
            .unwrap()
    );
}

#[test]
fn invalid_strategy_evidence_leaves_no_candidate_or_binding() {
    let dir = TempDir::new().unwrap();
    let store = LearningMemory::open(dir.path()).unwrap();
    let request = fixture(&store);
    let mut cases = Vec::new();
    let mut bad = request.clone();
    bad.selection.events[0].event_digest = "f".repeat(64);
    cases.push(bad);
    let mut bad = request.clone();
    bad.selection.events.push(bad.selection.events[0].clone());
    cases.push(bad);
    let mut bad = request.clone();
    bad.selection.context_digest = "f".repeat(64);
    cases.push(bad);
    let mut bad = request.clone();
    bad.selection.accounting_unit = "wrong".into();
    cases.push(bad);
    let mut bad = request.clone();
    bad.selection.events[0].attempt_id = "absent".into();
    cases.push(bad);
    let mut bad = request.clone();
    bad.preconditions.clear();
    cases.push(bad);
    let mut bad = request.clone();
    bad.counterexamples = vec!["x".into(); 17];
    cases.push(bad);
    for bad in cases {
        assert!(
            store
                .propose_strategy("tenant", "scope", bad, "actor")
                .is_err()
        );
        assert!(
            store
                .strategy_candidate("tenant", "scope", "strategy")
                .unwrap()
                .is_none()
        );
        assert!(
            store
                .registered_procedure("tenant", "scope", "strategy")
                .unwrap()
                .is_none()
        );
    }
    store
        .propose_registered(
            "tenant",
            "scope",
            request
                .proposal(
                    &store
                        .export_experiences("tenant", "scope", request.selection.clone())
                        .unwrap(),
                )
                .unwrap(),
            "ordinary",
        )
        .unwrap();
    assert!(
        store
            .propose_strategy("tenant", "scope", request, "actor")
            .is_err()
    );
    assert!(
        store
            .strategy_candidate("tenant", "scope", "strategy")
            .unwrap()
            .is_none()
    );
}

#[test]
fn strategy_uses_existing_qualification_and_suspension_authority() {
    let dir = TempDir::new().unwrap();
    let store = LearningMemory::open(dir.path()).unwrap();
    let request = fixture(&store);
    let receipt = store
        .propose_strategy("tenant", "scope", request.clone(), "extractor")
        .unwrap();
    let actor = EvaluationActor {
        subject_id: "judge".into(),
        credential_id: "judge-key".into(),
    };
    let submission = EvaluationSubmission {
        case_id: "incomplete".into(),
        phase: EvaluationPhase::Qualification,
        policy_id: "policy".into(),
        context_id: "context".into(),
        baseline_revision: "baseline".into(),
        evidence_ref: "fixture:held-out:incomplete".into(),
        status: SubmissionStatus::Complete,
        baseline_utility: Some(0.0),
        candidate_utility: Some(1.0),
        candidate_cost_units: None,
        candidate_latency_ms: Some(10),
        detail: None,
    };
    assert_eq!(
        store
            .admit_evaluation(
                "tenant",
                "scope",
                "strategy",
                submission.clone(),
                actor.clone()
            )
            .unwrap()
            .outcome,
        AdmissionOutcome::UnknownConsumption
    );
    assert_eq!(
        store
            .registered_procedure("tenant", "scope", "strategy")
            .unwrap()
            .unwrap()
            .record
            .qualification_count,
        0
    );
    for (n, reference) in [
        "fixture:evidence:success",
        "fixture:input:failure",
        "fixture:extraction",
    ]
    .into_iter()
    .enumerate()
    {
        let mut recycled = submission.clone();
        recycled.case_id = format!("recycled-{n}");
        recycled.evidence_ref = reference.into();
        recycled.candidate_cost_units = Some(5);
        assert_eq!(
            store
                .admit_evaluation("tenant", "scope", "strategy", recycled, actor.clone())
                .unwrap()
                .outcome,
            AdmissionOutcome::Rejected
        );
    }
    for n in 0..32 {
        let mut trial = submission.clone();
        trial.case_id = format!("held-out-{n}");
        trial.evidence_ref = format!("fixture:held-out:{n}");
        trial.candidate_cost_units = Some(5);
        let outcome = store
            .admit_evaluation("tenant", "scope", "strategy", trial, actor.clone())
            .unwrap();
        assert_eq!(
            outcome.state_after,
            if n == 31 {
                ProcedureState::Active
            } else {
                ProcedureState::Candidate
            }
        );
    }
    let selected = store
        .select_registered("tenant", "scope", "policy", "context", 64 * 1024)
        .unwrap()
        .unwrap();
    let instructions: serde_json::Value =
        serde_json::from_str(&selected.record.proposal.instructions).unwrap();
    assert_eq!(
        instructions["counterexamples"][0],
        request.counterexamples[0]
    );
    let mut monitoring = submission;
    monitoring.case_id = "monitor".into();
    monitoring.evidence_ref = "fixture:monitor".into();
    monitoring.phase = EvaluationPhase::Monitoring;
    monitoring.candidate_utility = Some(0.0);
    monitoring.candidate_cost_units = Some(5);
    assert_eq!(
        store
            .admit_evaluation("tenant", "scope", "strategy", monitoring, actor)
            .unwrap()
            .state_after,
        ProcedureState::Suspended
    );
    assert!(
        store
            .select_registered("tenant", "scope", "policy", "context", 64 * 1024)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        receipt,
        store
            .propose_strategy("tenant", "scope", request, "extractor")
            .unwrap()
    );
}

#[test]
fn concurrent_strategy_versions_have_one_winner_and_detect_corruption() {
    let dir = TempDir::new().unwrap();
    let store = LearningMemory::open(dir.path()).unwrap();
    let request = fixture(&store);
    let barrier = Arc::new(Barrier::new(2));
    let joins: Vec<_> = (0..2)
        .map(|n| {
            let s = store.clone();
            let b = barrier.clone();
            let mut r = request.clone();
            r.extractor.prompt_revision = format!("v{n}");
            std::thread::spawn(move || {
                b.wait();
                s.propose_strategy("tenant", "scope", r, "actor")
            })
        })
        .collect();
    assert_eq!(
        joins
            .into_iter()
            .filter_map(|j| j.join().unwrap().ok())
            .count(),
        1
    );
    let mut receipt = store
        .strategy_candidate("tenant", "scope", "strategy")
        .unwrap()
        .unwrap();
    receipt.summary.failed = 0;
    store
        .inner
        .db
        .put(
            key("tenant", "scope", "strategy").unwrap(),
            encode(&receipt).unwrap(),
        )
        .unwrap();
    assert!(matches!(
        store.strategy_candidate("tenant", "scope", "strategy"),
        Err(Error::DataCorruption(_))
    ));
}
