//! Offline demonstration with an executable deterministic evaluator, not an LLM benchmark.
//! Run: cargo run -p qilbee-memory --example learning_cycle --locked

use qilbee_memory::learning::*;

fn evaluate(case: u32, phase: EvaluationPhase) -> PairedEvaluation {
    let input = format!("  {case}  ");
    let expected = Some(i64::from(case));
    let baseline = input.parse::<i64>().ok();
    let candidate = input.trim().parse::<i64>().ok();
    PairedEvaluation {
        case_id: format!("integer-{case}"),
        phase,
        evaluator_id: "deterministic-parser-test".into(),
        evaluation_contract: "whitespace-integers-v1".into(),
        evidence_ref: format!("local-test:integer-{case}"),
        baseline_utility: if baseline == expected { 1.0 } else { 0.0 },
        candidate_utility: if candidate == expected { 1.0 } else { 0.0 },
        candidate_cost_units: 1,
        candidate_latency_ms: 1,
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::TempDir::new()?;
    let scope = LearningScope {
        tenant: "demo".into(),
        agent: "parser-agent".into(),
        environment: "rust-parser-v1".into(),
    };
    let policy = LearningPolicy {
        evaluator_id: "deterministic-parser-test".into(),
        evaluation_contract: "whitespace-integers-v1".into(),
        ..LearningPolicy::default()
    };
    let trials = policy.qualification_trials;
    let failures = policy.max_failure_streak;
    let db = LearningMemory::open(directory.path())?;
    db.propose(
        &scope,
        ProcedureProposal {
            id: "trim-before-parse-v1".into(),
            task: "parse-integer".into(),
            baseline_revision: "str-parse-v1".into(),
            instructions: "Trim leading and trailing whitespace before parsing an integer.".into(),
            source_refs: vec!["training-trace:whitespace-observation".into()],
            policy,
        },
    )?;
    assert!(
        db.select(
            &scope,
            "parse-integer",
            "str-parse-v1",
            "whitespace-integers-v1",
            1024
        )?
        .is_none()
    );
    for case in 0..trials {
        db.record_evaluation(
            &scope,
            "trim-before-parse-v1",
            evaluate(case, EvaluationPhase::Qualification),
        )?;
    }
    let active = db
        .select(
            &scope,
            "parse-integer",
            "str-parse-v1",
            "whitespace-integers-v1",
            1024,
        )?
        .expect("procedure qualified");
    println!(
        "Promoted after {} paired cases; mean utility improvement {:.3}, lower bound {:.3}.",
        active.qualification_count,
        active.mean_improvement,
        active.lower_improvement_bound.unwrap()
    );
    println!("Agent context: {}", active.proposal.instructions);
    drop(db);
    let db = LearningMemory::open(directory.path())?;
    assert!(
        db.select(
            &scope,
            "parse-integer",
            "str-parse-v1",
            "whitespace-integers-v1",
            1024
        )?
        .is_some()
    );

    // Simulate excessive runtime during deployment, reported by the evaluator.
    for case in trials..trials + failures {
        let mut report = evaluate(case, EvaluationPhase::Monitoring);
        report.candidate_latency_ms = 120_000;
        db.record_evaluation(&scope, "trim-before-parse-v1", report)?;
    }
    assert!(
        db.select(
            &scope,
            "parse-integer",
            "str-parse-v1",
            "whitespace-integers-v1",
            1024
        )?
        .is_none()
    );
    let suspended = db.get(&scope, "trim-before-parse-v1")?.unwrap();
    println!(
        "Suspended after {} budget violations; selection falls back to the baseline.",
        suspended.monitoring_count
    );
    println!(
        "Retained {} auditable state transitions across a database reopen.",
        suspended.decisions.len()
    );
    Ok(())
}
