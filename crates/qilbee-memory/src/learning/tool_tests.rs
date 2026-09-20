use super::*;
use tempfile::TempDir;

fn actor() -> ToolActor {
    ToolActor {
        subject_id: "developer".into(),
        credential_id: "credential-v1".into(),
    }
}
fn artifact(id: &str) -> ToolArtifactProposal {
    ToolArtifactProposal {
        id: id.into(),
        source: "def run(value):\n    return value\n".into(),
        dependency_lock: "".into(),
        runtime_image_digest: format!("sha256:{}", "a".repeat(64)),
        entrypoint: "tool:run".into(),
        input_schema: serde_json::json!({"type":"object"}),
        output_schema: serde_json::json!(true),
        source_refs: vec!["request:one".into()],
        parent_artifact_id: None,
        repair_evidence_ref: None,
    }
}
#[test]
fn tool_artifact_is_immutable_scoped_and_durable() {
    let dir = TempDir::new().unwrap();
    let store = LearningMemory::open(dir.path()).unwrap();
    let first = store
        .register_tool_artifact("tenant", "scope", artifact("one"), actor())
        .unwrap();
    assert_eq!(first.source_digest.len(), 64);
    assert_eq!(
        first.dependency_digest,
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        first,
        store
            .register_tool_artifact("tenant", "scope", artifact("one"), actor())
            .unwrap()
    );
    let mut changed = artifact("one");
    changed.source.push(' ');
    assert!(
        store
            .register_tool_artifact("tenant", "scope", changed, actor())
            .is_err()
    );
    assert!(
        store
            .tool_artifact("other", "scope", "one")
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .tool_artifact("tenant", "other", "one")
            .unwrap()
            .is_none()
    );
    drop(store);
    let store = LearningMemory::open(dir.path()).unwrap();
    assert_eq!(
        store.tool_artifact("tenant", "scope", "one").unwrap(),
        Some(first)
    );
}
#[test]
fn tool_artifact_repair_requires_local_parent_and_evidence() {
    let dir = TempDir::new().unwrap();
    let store = LearningMemory::open(dir.path()).unwrap();
    let mut repair = artifact("repair");
    repair.parent_artifact_id = Some("one".into());
    assert!(
        store
            .register_tool_artifact("tenant", "scope", repair.clone(), actor())
            .is_err()
    );
    repair.repair_evidence_ref = Some("failure:one".into());
    store
        .register_tool_artifact("tenant", "foreign", artifact("one"), actor())
        .unwrap();
    assert!(
        store
            .register_tool_artifact("tenant", "scope", repair.clone(), actor())
            .is_err()
    );
    store
        .register_tool_artifact("tenant", "scope", artifact("one"), actor())
        .unwrap();
    let record = store
        .register_tool_artifact("tenant", "scope", repair, actor())
        .unwrap();
    assert_eq!(record.proposal.parent_artifact_id.as_deref(), Some("one"));
}
#[test]
fn tool_artifact_rejects_unbounded_or_malformed_declarations() {
    let dir = TempDir::new().unwrap();
    let store = LearningMemory::open(dir.path()).unwrap();
    let mut bad = artifact("one");
    bad.runtime_image_digest = "python:latest".into();
    assert!(
        store
            .register_tool_artifact("tenant", "scope", bad, actor())
            .is_err()
    );
    let mut bad = artifact("one");
    bad.source = "x".repeat(24 * 1024 + 1);
    assert!(
        store
            .register_tool_artifact("tenant", "scope", bad, actor())
            .is_err()
    );
    let mut bad = artifact("one");
    bad.input_schema = serde_json::json!([]);
    assert!(
        store
            .register_tool_artifact("tenant", "scope", bad, actor())
            .is_err()
    );
    let mut bad = serde_json::to_value(artifact("one")).unwrap();
    bad["approved"] = true.into();
    assert!(serde_json::from_value::<ToolArtifactProposal>(bad).is_err());
}
#[test]
fn tool_artifact_concurrent_retries_have_one_original_record() {
    let dir = TempDir::new().unwrap();
    let store = LearningMemory::open(dir.path()).unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let store = store.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store
                    .register_tool_artifact("tenant", "scope", artifact("one"), actor())
                    .unwrap()
            })
        })
        .collect();
    let records: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    assert!(records.iter().all(|record| record == &records[0]));
}

fn executor(id: &str) -> ToolExecutorProfile {
    ToolExecutorProfile {
        id: id.into(),
        subject_id: "developer".into(),
        runtime_image_digest: format!("sha256:{}", "a".repeat(64)),
        environment_revision: "sandbox-v1".into(),
        permissions_revision: "network-denied-v1".into(),
        max_cost_units: 100,
        max_latency_ms: 60000,
    }
}
#[test]
fn tool_executor_profile_is_administrative_immutable_and_durable() {
    let dir = TempDir::new().unwrap();
    let store = LearningMemory::open(dir.path()).unwrap();
    let first = store
        .register_tool_executor("tenant", executor("worker-v1"), actor())
        .unwrap();
    assert_eq!(
        first,
        store
            .register_tool_executor("tenant", executor("worker-v1"), actor())
            .unwrap()
    );
    let mut changed = executor("worker-v1");
    changed.subject_id = "impostor".into();
    assert!(
        store
            .register_tool_executor("tenant", changed, actor())
            .is_err()
    );
    assert!(store.tool_executor("other", "worker-v1").unwrap().is_none());
    drop(store);
    let store = LearningMemory::open(dir.path()).unwrap();
    assert_eq!(
        store.tool_executor("tenant", "worker-v1").unwrap(),
        Some(first)
    );
}
#[test]
fn tool_executor_profile_rejects_unbounded_runtime_authority() {
    let dir = TempDir::new().unwrap();
    let store = LearningMemory::open(dir.path()).unwrap();
    for field in [
        "runtime_image_digest",
        "subject_id",
        "permissions_revision",
        "environment_revision",
    ] {
        let mut bad = serde_json::to_value(executor("worker-v1")).unwrap();
        bad[field] = "".into();
        let bad = serde_json::from_value(bad).unwrap();
        assert!(
            store
                .register_tool_executor("tenant", bad, actor())
                .is_err()
        );
    }
    let mut bad = executor("worker-v1");
    bad.max_cost_units = 0;
    assert!(
        store
            .register_tool_executor("tenant", bad, actor())
            .is_err()
    );
    let mut bad = executor("worker-v1");
    bad.max_latency_ms = 0;
    assert!(
        store
            .register_tool_executor("tenant", bad, actor())
            .is_err()
    );
}
#[test]
fn tool_executor_profile_concurrent_registration_preserves_one_actor() {
    let dir = TempDir::new().unwrap();
    let store = LearningMemory::open(dir.path()).unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
    let threads: Vec<_> = (0..8)
        .map(|i| {
            let store = store.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                let mut actor = actor();
                actor.credential_id = format!("key-{i}");
                store
                    .register_tool_executor("tenant", executor("worker-v1"), actor)
                    .unwrap()
            })
        })
        .collect();
    let records: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    assert!(records.iter().all(|record| record == &records[0]));
}

fn development(id: &str) -> ToolDevelopmentRequest {
    ToolDevelopmentRequest {
        id: id.into(),
        executor_id: "worker-v1".into(),
        objective: "Develop a deterministic identity tool and test it.".into(),
        parent_artifact_id: None,
        repair_evidence_ref: None,
    }
}
fn setup_development(store: &LearningMemory) {
    store
        .register_tool_executor("tenant", executor("worker-v1"), actor())
        .unwrap();
    store
        .create_tool_development("tenant", "scope", development("request-v1"), actor())
        .unwrap();
}
fn success(event: &str, revision: u64) -> ToolDevelopmentCommand {
    let mut candidate = artifact("candidate-v1");
    candidate.source_refs = vec!["development:request-v1".into()];
    ToolDevelopmentCommand {
        event_id: event.into(),
        expected_revision: revision,
        action: ToolDevelopmentAction::Report {
            report: ToolDevelopmentReport {
                executor_profile_digest: String::new(),
                outcome: ToolReportOutcome::Succeeded,
                evidence_ref: "test-run-v1".into(),
                detail: None,
                cost_units: Some(10),
                latency_ms: Some(100),
                artifact: Some(candidate),
            },
        },
    }
}
fn bind(store: &LearningMemory, mut command: ToolDevelopmentCommand) -> ToolDevelopmentCommand {
    if let ToolDevelopmentAction::Report { report } = &mut command.action {
        report.executor_profile_digest = store
            .tool_executor("tenant", "worker-v1")
            .unwrap()
            .unwrap()
            .profile_digest;
    }
    command
}
#[test]
fn tool_development_success_is_atomic_and_recovers_original_receipts() {
    let dir = TempDir::new().unwrap();
    let store = LearningMemory::open(dir.path()).unwrap();
    setup_development(&store);
    let original = store
        .create_tool_development("tenant", "scope", development("request-v1"), actor())
        .unwrap();
    let command = bind(&store, success("result-v1", 1));
    let event = store
        .apply_tool_development("tenant", "scope", "request-v1", command.clone(), actor())
        .unwrap();
    assert_eq!(event.record.state, ToolDevelopmentState::Succeeded);
    assert_eq!(event.record.revision, 2);
    assert_eq!(event.record.artifact_id.as_deref(), Some("candidate-v1"));
    assert!(
        store
            .tool_artifact("tenant", "scope", "candidate-v1")
            .unwrap()
            .is_some()
    );
    assert_eq!(
        original,
        store
            .create_tool_development("tenant", "scope", development("request-v1"), actor())
            .unwrap()
    );
    drop(store);
    let store = LearningMemory::open(dir.path()).unwrap();
    assert_eq!(
        event,
        store
            .apply_tool_development("tenant", "scope", "request-v1", command, actor())
            .unwrap()
    );
    assert_eq!(
        Some(event.record),
        store
            .tool_development("tenant", "scope", "request-v1")
            .unwrap()
    );
    assert!(
        store
            .tool_development("other", "scope", "request-v1")
            .unwrap()
            .is_none()
    );
}
#[test]
fn tool_development_unknown_consumption_never_registers_success() {
    let dir = TempDir::new().unwrap();
    let store = LearningMemory::open(dir.path()).unwrap();
    setup_development(&store);
    let mut unknown = bind(&store, success("unknown", 1));
    if let ToolDevelopmentAction::Report { report } = &mut unknown.action {
        report.cost_units = None;
    }
    let event = store
        .apply_tool_development("tenant", "scope", "request-v1", unknown, actor())
        .unwrap();
    assert_eq!(event.record.state, ToolDevelopmentState::PendingOrUnknown);
    assert_eq!(event.record.cost_units, None);
    assert_eq!(event.record.reason, "unknown_consumption");
    assert!(
        store
            .tool_artifact("tenant", "scope", "candidate-v1")
            .unwrap()
            .is_none()
    );
    let event = store
        .apply_tool_development(
            "tenant",
            "scope",
            "request-v1",
            bind(&store, success("reconciled", 2)),
            actor(),
        )
        .unwrap();
    assert_eq!(event.record.state, ToolDevelopmentState::Succeeded);
}
#[test]
fn tool_development_cancellation_requires_executor_confirmation() {
    let dir = TempDir::new().unwrap();
    let store = LearningMemory::open(dir.path()).unwrap();
    setup_development(&store);
    let cancel = ToolDevelopmentCommand {
        event_id: "cancel".into(),
        expected_revision: 1,
        action: ToolDevelopmentAction::RequestCancellation {
            reason: "Requester stopped the mission".into(),
        },
    };
    let event = store
        .apply_tool_development("tenant", "scope", "request-v1", cancel.clone(), actor())
        .unwrap();
    assert_eq!(
        event.record.state,
        ToolDevelopmentState::CancellationRequested
    );
    assert_eq!(event.record.cost_units, None);
    let mut report = bind(&store, success("ack-cancel", 2));
    if let ToolDevelopmentAction::Report { report } = &mut report.action {
        report.outcome = ToolReportOutcome::Cancelled;
        report.artifact = None;
        report.cost_units = None;
        report.latency_ms = None;
    }
    let confirmed = store
        .apply_tool_development("tenant", "scope", "request-v1", report, actor())
        .unwrap();
    assert_eq!(confirmed.record.state, ToolDevelopmentState::Cancelled);
    assert_eq!(confirmed.record.cost_units, None);
    assert_eq!(
        event,
        store
            .apply_tool_development("tenant", "scope", "request-v1", cancel, actor())
            .unwrap()
    );
    assert!(
        store
            .apply_tool_development(
                "tenant",
                "scope",
                "request-v1",
                bind(&store, success("late-success", 3)),
                actor()
            )
            .is_err()
    );
}
#[test]
fn tool_development_denies_wrong_actor_runtime_and_stale_revision() {
    let dir = TempDir::new().unwrap();
    let store = LearningMemory::open(dir.path()).unwrap();
    setup_development(&store);
    let mut intruder = actor();
    intruder.subject_id = "intruder".into();
    assert!(
        store
            .apply_tool_development(
                "tenant",
                "scope",
                "request-v1",
                bind(&store, success("wrong-actor", 1)),
                intruder
            )
            .is_err()
    );
    let mut command = bind(&store, success("wrong-image", 1));
    if let ToolDevelopmentAction::Report { report } = &mut command.action {
        report.artifact.as_mut().unwrap().runtime_image_digest =
            format!("sha256:{}", "b".repeat(64));
    }
    assert!(
        store
            .apply_tool_development("tenant", "scope", "request-v1", command, actor())
            .is_err()
    );
    assert!(
        store
            .apply_tool_development(
                "tenant",
                "scope",
                "request-v1",
                bind(&store, success("stale", 0)),
                actor()
            )
            .is_err()
    );
    assert_eq!(
        store
            .tool_development("tenant", "scope", "request-v1")
            .unwrap()
            .unwrap()
            .revision,
        1
    );
    assert!(
        store
            .tool_artifact("tenant", "scope", "candidate-v1")
            .unwrap()
            .is_none()
    );
}
#[test]
fn tool_development_concurrent_reports_are_idempotent_and_changed_retries_conflict() {
    let dir = TempDir::new().unwrap();
    let store = LearningMemory::open(dir.path()).unwrap();
    setup_development(&store);
    let command = bind(&store, success("result-v1", 1));
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let store = store.clone();
            let barrier = barrier.clone();
            let command = command.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store
                    .apply_tool_development("tenant", "scope", "request-v1", command, actor())
                    .unwrap()
            })
        })
        .collect();
    let events: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    assert!(events.iter().all(|e| e == &events[0]));
    let mut changed = command;
    changed.expected_revision = 2;
    assert!(
        store
            .apply_tool_development("tenant", "scope", "request-v1", changed, actor())
            .is_err()
    );
}
#[test]
fn tool_development_budget_failure_and_repair_keep_exact_parent() {
    let dir = TempDir::new().unwrap();
    let store = LearningMemory::open(dir.path()).unwrap();
    setup_development(&store);
    let mut command = bind(&store, success("over-budget", 1));
    if let ToolDevelopmentAction::Report { report } = &mut command.action {
        report.cost_units = Some(101);
    }
    let failed = store
        .apply_tool_development("tenant", "scope", "request-v1", command, actor())
        .unwrap();
    assert_eq!(failed.record.state, ToolDevelopmentState::Failed);
    assert_eq!(failed.record.reason, "resource_limit_exceeded");
    assert!(
        store
            .tool_artifact("tenant", "scope", "candidate-v1")
            .unwrap()
            .is_none()
    );
    store
        .register_tool_artifact("tenant", "scope", artifact("parent"), actor())
        .unwrap();
    let mut repair = development("repair");
    repair.parent_artifact_id = Some("parent".into());
    repair.repair_evidence_ref = Some("failure-v1".into());
    store
        .create_tool_development("tenant", "scope", repair.clone(), actor())
        .unwrap();
    repair.parent_artifact_id = Some("foreign".into());
    assert!(
        store
            .create_tool_development("tenant", "other", repair, actor())
            .is_err()
    );
}

#[test]
fn tool_development_unknown_accounting_cannot_erase_known_lower_bounds() {
    let dir = TempDir::new().unwrap();
    let store = LearningMemory::open(dir.path()).unwrap();
    setup_development(&store);
    for (event, revision, cost) in [("known", 1, Some(10)), ("unknown", 2, None)] {
        let mut command = bind(&store, success(event, revision));
        if let ToolDevelopmentAction::Report { report } = &mut command.action {
            report.outcome = ToolReportOutcome::PendingOrUnknown;
            report.artifact = None;
            report.cost_units = cost;
            report.latency_ms = None;
        }
        store
            .apply_tool_development("tenant", "scope", "request-v1", command, actor())
            .unwrap();
    }
    let mut lowered = bind(&store, success("lowered", 3));
    if let ToolDevelopmentAction::Report { report } = &mut lowered.action {
        report.cost_units = Some(5);
    }
    assert!(
        store
            .apply_tool_development("tenant", "scope", "request-v1", lowered, actor())
            .is_err()
    );
}
#[test]
fn tool_development_success_can_win_an_unconfirmed_cancellation_race() {
    let dir = TempDir::new().unwrap();
    let store = LearningMemory::open(dir.path()).unwrap();
    setup_development(&store);
    let cancel = ToolDevelopmentCommand {
        event_id: "cancel".into(),
        expected_revision: 1,
        action: ToolDevelopmentAction::RequestCancellation {
            reason: "Stop work".into(),
        },
    };
    store
        .apply_tool_development("tenant", "scope", "request-v1", cancel, actor())
        .unwrap();
    let mut pending = bind(&store, success("pending", 2));
    if let ToolDevelopmentAction::Report { report } = &mut pending.action {
        report.outcome = ToolReportOutcome::PendingOrUnknown;
        report.artifact = None;
        report.cost_units = None;
        report.latency_ms = None;
    }
    let event = store
        .apply_tool_development("tenant", "scope", "request-v1", pending, actor())
        .unwrap();
    assert_eq!(
        event.record.state,
        ToolDevelopmentState::CancellationRequested
    );
    let event = store
        .apply_tool_development(
            "tenant",
            "scope",
            "request-v1",
            bind(&store, success("complete", 3)),
            actor(),
        )
        .unwrap();
    assert_eq!(event.record.state, ToolDevelopmentState::Succeeded);
}
#[test]
fn tool_development_competing_events_use_compare_and_swap() {
    let dir = TempDir::new().unwrap();
    let store = LearningMemory::open(dir.path()).unwrap();
    setup_development(&store);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let threads: Vec<_> = (0..2)
        .map(|i| {
            let store = store.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let mut command = bind(&store, success(&format!("failure-{i}"), 1));
                if let ToolDevelopmentAction::Report { report } = &mut command.action {
                    report.outcome = ToolReportOutcome::Failed;
                    report.artifact = None;
                    report.cost_units = None;
                }
                barrier.wait();
                store.apply_tool_development("tenant", "scope", "request-v1", command, actor())
            })
        })
        .collect();
    let results: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    let record = store
        .tool_development("tenant", "scope", "request-v1")
        .unwrap()
        .unwrap();
    assert_eq!(record.state, ToolDevelopmentState::Failed);
    assert_eq!(record.cost_units, None);
    assert_eq!(record.revision, 2);
}
