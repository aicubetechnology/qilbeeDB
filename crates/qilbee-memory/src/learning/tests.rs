use super::*;
use std::sync::{Arc, Barrier};
use tempfile::TempDir;

fn scope() -> LearningScope {
    LearningScope {
        tenant: "tenant-a".into(),
        agent: "agent-a".into(),
        environment: "model-tools-v1".into(),
    }
}

fn proposal() -> ProcedureProposal {
    ProcedureProposal {
        id: "procedure-1".into(),
        task: "resolve-task".into(),
        baseline_revision: "baseline-v1".into(),
        instructions: "Read the observed state before choosing an action.".into(),
        source_refs: vec!["episode:training-1".into()],
        policy: LearningPolicy {
            qualification_trials: 32,
            evaluator_id: "test-harness".into(),
            evaluation_contract: "held-out-suite-v1".into(),
            ..LearningPolicy::default()
        },
    }
}

fn evaluation(case: usize, phase: EvaluationPhase) -> PairedEvaluation {
    PairedEvaluation {
        case_id: format!("case-{case}"),
        phase,
        evaluator_id: "test-harness".into(),
        evaluation_contract: "held-out-suite-v1".into(),
        evidence_ref: format!("trace:held-out-{case}"),
        baseline_utility: 0.0,
        candidate_utility: 1.0,
        candidate_cost_units: 10,
        candidate_latency_ms: 100,
    }
}

fn database() -> (LearningMemory, TempDir) {
    let dir = TempDir::new().unwrap();
    (LearningMemory::open(dir.path()).unwrap(), dir)
}

fn qualify(db: &LearningMemory) {
    for case in 0..32 {
        db.record_evaluation(
            &scope(),
            "procedure-1",
            evaluation(case, EvaluationPhase::Qualification),
        )
        .unwrap();
    }
}

#[test]
fn learning_fixed_budget_promotes_only_after_all_cases_and_survives_restart() {
    let (db, dir) = database();
    db.propose(&scope(), proposal()).unwrap();
    for case in 0..31 {
        let result = db
            .record_evaluation(
                &scope(),
                "procedure-1",
                evaluation(case, EvaluationPhase::Qualification),
            )
            .unwrap();
        assert_eq!(result.receipt.state_after, ProcedureState::Candidate);
    }
    assert!(
        db.select(
            &scope(),
            "resolve-task",
            "baseline-v1",
            "held-out-suite-v1",
            4096
        )
        .unwrap()
        .is_none()
    );
    db.record_evaluation(
        &scope(),
        "procedure-1",
        evaluation(31, EvaluationPhase::Qualification),
    )
    .unwrap();
    let selected = db
        .select(
            &scope(),
            "resolve-task",
            "baseline-v1",
            "held-out-suite-v1",
            4096,
        )
        .unwrap()
        .unwrap();
    assert_eq!(selected.state, ProcedureState::Active);
    assert!(selected.lower_improvement_bound.unwrap() > 0.05);
    drop(db);
    let restored = LearningMemory::open(dir.path()).unwrap();
    assert_eq!(
        restored.get(&scope(), "procedure-1").unwrap().unwrap(),
        selected
    );
    assert!(
        restored
            .evaluation(&scope(), "procedure-1", "case-31")
            .unwrap()
            .is_some()
    );
}

#[test]
fn learning_duplicate_delivery_is_idempotent_and_conflicting_feedback_fails() {
    let (db, _dir) = database();
    db.propose(&scope(), proposal()).unwrap();
    let input = evaluation(0, EvaluationPhase::Qualification);
    let first = db
        .record_evaluation(&scope(), "procedure-1", input.clone())
        .unwrap();
    let duplicate = db
        .record_evaluation(&scope(), "procedure-1", input.clone())
        .unwrap();
    assert!(duplicate.duplicate);
    assert_eq!(first.receipt, duplicate.receipt);
    let mut conflict = input;
    conflict.candidate_utility = 0.0;
    assert!(
        db.record_evaluation(&scope(), "procedure-1", conflict)
            .is_err()
    );
    assert_eq!(
        db.get(&scope(), "procedure-1")
            .unwrap()
            .unwrap()
            .qualification_count,
        1
    );
}

#[test]
fn learning_regression_suspends_and_returns_to_baseline_without_losing_evidence() {
    let (db, _dir) = database();
    db.propose(&scope(), proposal()).unwrap();
    qualify(&db);
    for case in 32..35 {
        let mut input = evaluation(case, EvaluationPhase::Monitoring);
        input.baseline_utility = 1.0;
        input.candidate_utility = 0.0;
        db.record_evaluation(&scope(), "procedure-1", input)
            .unwrap();
    }
    let record = db.get(&scope(), "procedure-1").unwrap().unwrap();
    assert_eq!(record.state, ProcedureState::Suspended);
    assert_eq!(record.decisions.len(), 2);
    assert_eq!(record.qualification_count, 32);
    assert_eq!(record.monitoring_count, 3);
    assert!(
        db.select(
            &scope(),
            "resolve-task",
            "baseline-v1",
            "held-out-suite-v1",
            4096
        )
        .unwrap()
        .is_none()
    );
    assert!(
        db.evaluation(&scope(), "procedure-1", "case-34")
            .unwrap()
            .is_some()
    );
    assert!(
        db.record_evaluation(
            &scope(),
            "procedure-1",
            evaluation(35, EvaluationPhase::Monitoring)
        )
        .is_err()
    );
    // A retry still returns the original receipt after state transitions.
    let retry = db
        .record_evaluation(
            &scope(),
            "procedure-1",
            evaluation(0, EvaluationPhase::Qualification),
        )
        .unwrap();
    assert!(retry.duplicate);
    assert_eq!(retry.receipt.state_after, ProcedureState::Candidate);
}

#[test]
fn learning_rejects_no_gain_or_excessive_cost_at_fixed_budget() {
    for over_budget in [false, true] {
        let (db, _dir) = database();
        db.propose(&scope(), proposal()).unwrap();
        for case in 0..32 {
            let mut input = evaluation(case, EvaluationPhase::Qualification);
            if over_budget {
                input.candidate_cost_units = 10_001;
            } else {
                input.baseline_utility = 1.0;
            }
            db.record_evaluation(&scope(), "procedure-1", input)
                .unwrap();
        }
        assert_eq!(
            db.get(&scope(), "procedure-1").unwrap().unwrap().state,
            ProcedureState::Rejected
        );
        assert!(
            db.record_evaluation(
                &scope(),
                "procedure-1",
                evaluation(33, EvaluationPhase::Qualification)
            )
            .is_err()
        );
    }
}

#[test]
fn learning_scope_environment_baseline_and_context_budget_isolation() {
    let (db, _dir) = database();
    db.propose(&scope(), proposal()).unwrap();
    qualify(&db);
    for field in 0..3 {
        let mut other = scope();
        match field {
            0 => other.tenant.push('b'),
            1 => other.agent.push('b'),
            _ => other.environment.push('b'),
        }
        assert!(db.get(&other, "procedure-1").unwrap().is_none());
        assert!(
            db.select(
                &other,
                "resolve-task",
                "baseline-v1",
                "held-out-suite-v1",
                4096
            )
            .unwrap()
            .is_none()
        );
        assert!(
            db.record_evaluation(
                &other,
                "procedure-1",
                evaluation(35, EvaluationPhase::Monitoring)
            )
            .is_err()
        );
    }
    assert!(
        db.select(
            &scope(),
            "resolve-task",
            "baseline-v2",
            "held-out-suite-v1",
            4096
        )
        .unwrap()
        .is_none()
    );
    assert!(
        db.select(
            &scope(),
            "resolve-task",
            "baseline-v1",
            "held-out-suite-v1",
            1
        )
        .unwrap()
        .is_none()
    );
}

#[test]
fn learning_rejects_invalid_scores_contracts_phases_and_mutation() {
    let (db, _dir) = database();
    db.propose(&scope(), proposal()).unwrap();
    let mut changed = proposal();
    changed.instructions = "Replace after seeing test results".into();
    assert!(db.propose(&scope(), changed).is_err());
    for score in [f64::NAN, f64::INFINITY, -0.01, 1.01] {
        let mut input = evaluation(0, EvaluationPhase::Qualification);
        input.candidate_utility = score;
        assert!(
            db.record_evaluation(&scope(), "procedure-1", input)
                .is_err()
        );
    }
    let mut input = evaluation(0, EvaluationPhase::Qualification);
    input.evaluator_id = "unregistered".into();
    assert!(
        db.record_evaluation(&scope(), "procedure-1", input)
            .is_err()
    );
    let mut input = evaluation(0, EvaluationPhase::Qualification);
    input.evaluation_contract = "different-rubric".into();
    assert!(
        db.record_evaluation(&scope(), "procedure-1", input)
            .is_err()
    );
    let mut input = evaluation(0, EvaluationPhase::Qualification);
    input.evidence_ref = "episode:training-1".into();
    assert!(
        db.record_evaluation(&scope(), "procedure-1", input)
            .is_err()
    );
    assert!(
        db.record_evaluation(
            &scope(),
            "procedure-1",
            evaluation(0, EvaluationPhase::Monitoring)
        )
        .is_err()
    );
    assert_eq!(
        db.get(&scope(), "procedure-1")
            .unwrap()
            .unwrap()
            .qualification_count,
        0
    );
}

#[test]
fn learning_concurrent_feedback_counts_each_case_once() {
    let (db, _dir) = database();
    db.propose(&scope(), proposal()).unwrap();
    let db = Arc::new(db);
    let barrier = Arc::new(Barrier::new(8));
    let workers: Vec<_> = (0..8)
        .map(|_| {
            let db = db.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                for case in 0..32 {
                    db.record_evaluation(
                        &scope(),
                        "procedure-1",
                        evaluation(case, EvaluationPhase::Qualification),
                    )
                    .unwrap();
                }
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
    let record = db.get(&scope(), "procedure-1").unwrap().unwrap();
    assert_eq!(record.qualification_count, 32);
    assert_eq!(record.decisions.len(), 1);
    assert_eq!(record.state, ProcedureState::Active);
}

#[test]
fn learning_success_resets_failure_streak_and_latency_is_a_hard_gate() {
    let (db, _dir) = database();
    db.propose(&scope(), proposal()).unwrap();
    qualify(&db);
    for case in 32..38 {
        let mut report = evaluation(case, EvaluationPhase::Monitoring);
        if case != 34 {
            report.candidate_latency_ms = 60_001;
        }
        db.record_evaluation(&scope(), "procedure-1", report)
            .unwrap();
        let record = db.get(&scope(), "procedure-1").unwrap().unwrap();
        assert_eq!(
            record.state,
            if case == 37 {
                ProcedureState::Suspended
            } else {
                ProcedureState::Active
            }
        );
        if case == 34 {
            assert_eq!(record.failure_streak, 0);
        }
    }
}

#[test]
fn learning_rejects_high_relative_gain_with_low_absolute_quality() {
    let (db, _dir) = database();
    db.propose(&scope(), proposal()).unwrap();
    for case in 0..32 {
        let mut report = evaluation(case, EvaluationPhase::Qualification);
        report.candidate_utility = 0.65;
        db.record_evaluation(&scope(), "procedure-1", report)
            .unwrap();
    }
    let record = db.get(&scope(), "procedure-1").unwrap().unwrap();
    assert!(record.lower_improvement_bound.unwrap() > record.proposal.policy.min_improvement);
    assert_eq!(record.state, ProcedureState::Rejected);
}

#[test]
fn learning_promotion_bound_accounts_for_paired_range() {
    let (db, _dir) = database();
    db.propose(&scope(), proposal()).unwrap();
    for case in 0..32 {
        let mut report = evaluation(case, EvaluationPhase::Qualification);
        if case < 8 {
            report.baseline_utility = 1.0;
            report.candidate_utility = 0.0;
        }
        db.record_evaluation(&scope(), "procedure-1", report)
            .unwrap();
    }
    let record = db.get(&scope(), "procedure-1").unwrap().unwrap();
    assert!((record.mean_improvement - 0.5).abs() < 1e-12);
    // sqrt(2 * ln(100) / 32) = 0.5364915065723368.
    assert!((record.lower_improvement_bound.unwrap() + 0.0364915065723368).abs() < 1e-12);
    assert_eq!(record.state, ProcedureState::Rejected);
}

#[test]
fn learning_policy_validation_and_retry_of_immutable_proposals() {
    let (db, _dir) = database();
    for delta in [0.0, 0.5, f64::NAN, f64::INFINITY, -1.0] {
        let mut proposed = proposal();
        proposed.policy.confidence_delta = delta;
        assert!(db.propose(&scope(), proposed).is_err());
    }
    for count in [0, 1, 1_000_001] {
        let mut proposed = proposal();
        proposed.policy.qualification_trials = count;
        assert!(db.propose(&scope(), proposed).is_err());
    }
    let mut proposed = proposal();
    proposed.source_refs.clear();
    assert!(db.propose(&scope(), proposed).is_err());
    let first = db.propose(&scope(), proposal()).unwrap();
    assert_eq!(first, db.propose(&scope(), proposal()).unwrap());
}

#[test]
fn learning_length_prefixed_scopes_do_not_alias_and_selection_is_deterministic() {
    let (db, _dir) = database();
    let one = LearningScope {
        tenant: "a:b".into(),
        agent: "c".into(),
        environment: "日本語".into(),
    };
    let two = LearningScope {
        tenant: "a".into(),
        agent: "b:c".into(),
        environment: "日本語".into(),
    };
    db.propose(&one, proposal()).unwrap();
    assert!(db.get(&two, "procedure-1").unwrap().is_none());
    for id in ["b", "a"] {
        let mut proposed = proposal();
        proposed.id = id.into();
        proposed.instructions = "日本語".into();
        db.propose(&scope(), proposed).unwrap();
        for case in 0..32 {
            db.record_evaluation(
                &scope(),
                id,
                evaluation(case, EvaluationPhase::Qualification),
            )
            .unwrap();
        }
    }
    assert!(
        db.select(
            &scope(),
            "resolve-task",
            "baseline-v1",
            "held-out-suite-v1",
            8
        )
        .unwrap()
        .is_none()
    );
    assert_eq!(
        db.select(
            &scope(),
            "resolve-task",
            "baseline-v1",
            "held-out-suite-v1",
            9
        )
        .unwrap()
        .unwrap()
        .proposal
        .id,
        "a"
    );
}

#[test]
fn learning_unknown_schema_and_unrelated_database_are_not_overwritten() {
    for versioned in [false, true] {
        let dir = TempDir::new().unwrap();
        {
            let db = rocksdb::DB::open_default(dir.path()).unwrap();
            if versioned {
                db.put(b"\0qilbee-learning-schema", b"999").unwrap();
            } else {
                db.put(b"unrelated", b"preserve").unwrap();
            }
        }
        assert!(LearningMemory::open(dir.path()).is_err());
        let db = rocksdb::DB::open_default(dir.path()).unwrap();
        if !versioned {
            assert_eq!(db.get(b"unrelated").unwrap().unwrap(), b"preserve");
        }
    }
}

#[test]
fn learning_selection_never_mixes_evaluation_contracts() {
    let (db, _dir) = database();
    db.propose(&scope(), proposal()).unwrap();
    qualify(&db);
    assert!(
        db.select(
            &scope(),
            "resolve-task",
            "baseline-v1",
            "incompatible-rubric",
            4096
        )
        .unwrap()
        .is_none()
    );
}

#[test]
fn learning_same_evidence_cannot_inflate_sample_count_under_another_case_id() {
    let (db, _dir) = database();
    db.propose(&scope(), proposal()).unwrap();
    let report = evaluation(0, EvaluationPhase::Qualification);
    db.record_evaluation(&scope(), "procedure-1", report.clone())
        .unwrap();
    let mut alias = report;
    alias.case_id = "different-case-same-evidence".into();
    assert!(
        db.record_evaluation(&scope(), "procedure-1", alias)
            .is_err()
    );
    assert_eq!(
        db.get(&scope(), "procedure-1")
            .unwrap()
            .unwrap()
            .qualification_count,
        1
    );
}
