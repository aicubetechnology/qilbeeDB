use super::*;
use qilbee_core::Error;
use serde_json::json;
use std::sync::{Arc, Barrier};
use tempfile::TempDir;

fn setup() -> (TempDir, LearningMemory) {
    let dir = TempDir::new().unwrap();
    let db = LearningMemory::open(dir.path()).unwrap();
    let context = serde_json::from_value(json!({
        "task":"task-v1","baseline_revision":"baseline-v1","model_provider":"external",
        "model_revision":"model-v1","tools":{"run":"artifact-v1"},
        "environment_revision":"env-v1","evaluation_contract":"verifier-v1",
        "dataset_revision":"data-v1","harness_revision":"harness-v1",
        "permissions_revision":"permissions-v1"
    }))
    .unwrap();
    db.register_context("tenant", "context-v1", context, "operator")
        .unwrap();
    (dir, db)
}
fn actor(subject: &str) -> ExperienceActor {
    ExperienceActor {
        subject_id: subject.into(),
        credential_id: format!("credential-{subject}"),
    }
}
fn input(id: &str) -> ExperienceRequest {
    serde_json::from_value(json!({"id":id,"context_id":"context-v1",
        "reporter_subject_id":"observer","accounting_unit":"test-credit-v1",
        "input":{"reference":"fixture:input","sha256":"a".repeat(64)},"parent":null}))
    .unwrap()
}
fn command(id: &str, revision: u64, digest: &str, outcome: &str) -> ExperienceCommand {
    serde_json::from_value(json!({"event_id":id,"expected_revision":revision,
        "context_digest":digest,"outcome":outcome,
        "evidence":{"reference":"fixture:observation","sha256":"b".repeat(64)},
        "cost_units":null,"latency_ms":null}))
    .unwrap()
}
fn create(db: &LearningMemory) -> ExperienceReceipt {
    db.create_experience("tenant", "scope", input("attempt"), actor("writer"))
        .unwrap()
}

#[test]
fn experience_preserves_unknown_accounting_and_original_receipts_after_reopen() {
    let (dir, db) = setup();
    let initial = create(&db);
    assert_eq!(
        db.experience("tenant", "scope", "attempt")
            .unwrap()
            .unwrap()
            .outcome,
        None
    );
    let pending = command("pending", 1, &initial.context_digest, "unknown");
    let first = db
        .observe_experience(
            "tenant",
            "scope",
            "attempt",
            pending.clone(),
            actor("observer"),
        )
        .unwrap();
    let done = command("done", 2, &initial.context_digest, "succeeded");
    let final_event = db
        .observe_experience("tenant", "scope", "attempt", done, actor("observer"))
        .unwrap();
    assert_eq!(final_event.record.reported_cost_units, None);
    assert_eq!(final_event.record.observed_cost_units, None);
    drop(db);
    let db = LearningMemory::open(dir.path()).unwrap();
    assert_eq!(initial, create(&db));
    assert_eq!(
        first,
        db.observe_experience("tenant", "scope", "attempt", pending, actor("observer"))
            .unwrap()
    );
    assert_eq!(
        final_event.record,
        db.experience("tenant", "scope", "attempt")
            .unwrap()
            .unwrap()
    );
    assert_eq!(
        first,
        db.experience_event("tenant", "scope", "attempt", "pending")
            .unwrap()
            .unwrap()
    );
}

#[test]
fn experience_conflicts_do_not_erase_known_consumption_or_write_events() {
    let (_dir, db) = setup();
    let receipt = create(&db);
    let mut pending = command("pending", 1, &receipt.context_digest, "unknown");
    pending.cost_units = Some(9);
    pending.latency_ms = Some(50);
    db.observe_experience("tenant", "scope", "attempt", pending, actor("observer"))
        .unwrap();
    for (id, cost, latency) in [
        ("lower-cost", Some(8), None),
        ("lower-time", None, Some(49)),
    ] {
        let mut bad = command(id, 2, &receipt.context_digest, "succeeded");
        bad.cost_units = cost;
        bad.latency_ms = latency;
        assert!(
            db.observe_experience("tenant", "scope", "attempt", bad, actor("observer"))
                .is_err()
        );
        assert!(
            db.experience_event("tenant", "scope", "attempt", id)
                .unwrap()
                .is_none()
        );
    }
    let done = db
        .observe_experience(
            "tenant",
            "scope",
            "attempt",
            command("done", 2, &receipt.context_digest, "failed"),
            actor("observer"),
        )
        .unwrap();
    assert_eq!(
        (
            done.record.reported_cost_units,
            done.record.reported_latency_ms
        ),
        (None, None)
    );
    assert_eq!(
        (
            done.record.observed_cost_units,
            done.record.observed_latency_ms
        ),
        (Some(9), Some(50))
    );
    assert_eq!(done.record.revision, 3);
}

#[test]
fn experience_requires_bound_subject_context_and_expected_revision() {
    let (_dir, db) = setup();
    let receipt = create(&db);
    let good = command("event", 1, &receipt.context_digest, "cancelled");
    assert!(matches!(
        db.observe_experience("tenant", "scope", "attempt", good.clone(), actor("other")),
        Err(Error::Unauthorized(_))
    ));
    let mut wrong = good.clone();
    wrong.context_digest = "c".repeat(64);
    assert!(
        db.observe_experience("tenant", "scope", "attempt", wrong, actor("observer"))
            .is_err()
    );
    let mut stale = good.clone();
    stale.expected_revision = 2;
    assert!(matches!(
        db.observe_experience("tenant", "scope", "attempt", stale, actor("observer")),
        Err(Error::TransactionConflict(_))
    ));
    let event = db
        .observe_experience(
            "tenant",
            "scope",
            "attempt",
            good.clone(),
            actor("observer"),
        )
        .unwrap();
    let mut rotated = actor("observer");
    rotated.credential_id = "rotated".into();
    assert_eq!(
        event,
        db.observe_experience("tenant", "scope", "attempt", good.clone(), rotated)
            .unwrap()
    );
    let mut changed = good;
    changed.cost_units = Some(1);
    assert!(
        db.observe_experience("tenant", "scope", "attempt", changed, actor("observer"))
            .is_err()
    );
    assert!(
        db.observe_experience(
            "tenant",
            "scope",
            "attempt",
            command("new", 2, &receipt.context_digest, "succeeded"),
            actor("observer")
        )
        .is_err()
    );
    assert_eq!(
        event.record,
        db.experience("tenant", "scope", "attempt")
            .unwrap()
            .unwrap()
    );
}

#[test]
fn experience_binds_existing_parent_events_without_rewriting_history() {
    let (_dir, db) = setup();
    let parent = create(&db);
    let event = db
        .observe_experience(
            "tenant",
            "scope",
            "attempt",
            command("pending", 1, &parent.context_digest, "unknown"),
            actor("observer"),
        )
        .unwrap();
    let mut child = input("child");
    child.parent = Some(ExperienceParent {
        attempt_id: "attempt".into(),
        event_id: "pending".into(),
    });
    let receipt = db
        .create_experience("tenant", "scope", child.clone(), actor("writer"))
        .unwrap();
    assert_eq!(
        receipt.parent_event_digest.as_deref(),
        Some(event.event_digest.as_str())
    );
    db.observe_experience(
        "tenant",
        "scope",
        "attempt",
        command("done", 2, &parent.context_digest, "failed"),
        actor("observer"),
    )
    .unwrap();
    assert_eq!(
        receipt,
        db.create_experience("tenant", "scope", child.clone(), actor("writer"))
            .unwrap()
    );
    assert!(
        db.create_experience("tenant", "other-scope", child.clone(), actor("writer"))
            .is_err()
    );
    child.id = "attempt".into();
    assert!(
        db.create_experience("tenant", "scope", child, actor("writer"))
            .is_err()
    );
}

#[test]
fn experience_registration_is_immutable_scoped_and_context_bound() {
    let (_dir, db) = setup();
    let receipt = create(&db);
    assert!(
        db.experience("foreign", "scope", "attempt")
            .unwrap()
            .is_none()
    );
    assert!(
        db.experience("tenant", "foreign", "attempt")
            .unwrap()
            .is_none()
    );
    assert!(
        db.create_experience("foreign", "scope", input("attempt"), actor("writer"))
            .is_err()
    );
    assert!(
        db.create_experience("tenant", "scope", input("attempt"), actor("other"))
            .is_err()
    );
    let mut changed = input("attempt");
    changed.reporter_subject_id = "other".into();
    assert!(
        db.create_experience("tenant", "scope", changed, actor("writer"))
            .is_err()
    );
    let mut missing = input("new");
    missing.context_id = "missing".into();
    assert!(
        db.create_experience("tenant", "scope", missing, actor("writer"))
            .is_err()
    );
    assert_eq!(
        receipt.context_digest,
        db.context("tenant", "context-v1")
            .unwrap()
            .unwrap()
            .payload_digest
    );
}

#[test]
fn experience_concurrent_retries_have_one_receipt_and_revision() {
    let (_dir, db) = setup();
    let receipt = create(&db);
    let barrier = Arc::new(Barrier::new(8));
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let db = db.clone();
            let b = barrier.clone();
            let digest = receipt.context_digest.clone();
            std::thread::spawn(move || {
                b.wait();
                db.observe_experience(
                    "tenant",
                    "scope",
                    "attempt",
                    command("same", 1, &digest, "succeeded"),
                    actor("observer"),
                )
                .unwrap()
            })
        })
        .collect();
    let values: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    assert!(values.iter().all(|v| v == &values[0]));
    assert_eq!(values[0].record.revision, 2);
}

#[test]
fn experience_concurrent_competing_reports_have_one_winner() {
    let (_dir, db) = setup();
    let receipt = create(&db);
    let barrier = Arc::new(Barrier::new(8));
    let threads: Vec<_> = (0..8)
        .map(|i| {
            let db = db.clone();
            let b = barrier.clone();
            let digest = receipt.context_digest.clone();
            std::thread::spawn(move || {
                b.wait();
                db.observe_experience(
                    "tenant",
                    "scope",
                    "attempt",
                    command(&format!("event-{i}"), 1, &digest, "unknown"),
                    actor("observer"),
                )
            })
        })
        .collect();
    let results: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, Err(Error::TransactionConflict(_))))
            .count(),
        7
    );
}

#[test]
fn experience_rejects_invalid_references_and_unknown_json_fields() {
    let (_dir, db) = setup();
    for digest in ["".into(), "A".repeat(64), "a".repeat(63), "é".repeat(32)] {
        let mut bad = input("bad");
        bad.input.sha256 = digest;
        assert!(
            db.create_experience("tenant", "scope", bad, actor("writer"))
                .is_err()
        );
    }
    let mut extra = serde_json::to_value(input("bad")).unwrap();
    extra["actor"] = json!("forged");
    assert!(serde_json::from_value::<ExperienceRequest>(extra).is_err());
}

#[test]
fn experience_can_resolve_late_consumption_without_changing_a_terminal_outcome() {
    let (_dir, db) = setup();
    let receipt = create(&db);
    let first = db
        .observe_experience(
            "tenant",
            "scope",
            "attempt",
            command("done", 1, &receipt.context_digest, "succeeded"),
            actor("observer"),
        )
        .unwrap();
    let mut accounting = command("accounting", 2, &receipt.context_digest, "succeeded");
    accounting.cost_units = Some(12);
    accounting.latency_ms = Some(100);
    let settled = db
        .observe_experience("tenant", "scope", "attempt", accounting, actor("observer"))
        .unwrap();
    assert_eq!(settled.record.reported_cost_units, Some(12));
    assert_eq!(settled.record.revision, 3);
    assert_eq!(
        first,
        db.experience_event("tenant", "scope", "attempt", "done")
            .unwrap()
            .unwrap()
    );
    for outcome in ["failed", "cancelled", "unknown"] {
        assert!(
            db.observe_experience(
                "tenant",
                "scope",
                "attempt",
                command(outcome, 3, &receipt.context_digest, outcome),
                actor("observer")
            )
            .is_err()
        );
    }
}

#[test]
fn experience_history_is_bounded_fenced_and_recovers_without_an_index_migration() {
    let (dir, db) = setup();
    let receipt = create(&db);
    for (number, name) in ["z", "aa", "b"].iter().enumerate() {
        db.observe_experience(
            "tenant",
            "scope",
            "attempt",
            command(name, number as u64 + 1, &receipt.context_digest, "unknown"),
            actor("observer"),
        )
        .unwrap();
    }
    let query = ExperienceHistoryQuery {
        limit: 1,
        cursor: None,
    };
    let first = db
        .experience_history("tenant", "scope", "attempt", query)
        .unwrap();
    assert_eq!(first.through_revision, 4);
    assert_eq!(first.events[0].command.event_id, "b");
    assert_eq!(first.scanned_events, 1);
    assert!(!first.complete);
    // This event sorts between existing keys, but lies beyond the immutable fence.
    db.observe_experience(
        "tenant",
        "scope",
        "attempt",
        command("c", 4, &receipt.context_digest, "unknown"),
        actor("observer"),
    )
    .unwrap();
    drop(db);
    let db = LearningMemory::open(dir.path()).unwrap();
    let mut cursor = first.next_cursor;
    let mut ids = vec!["b".to_string()];
    let mut empty_continuation = false;
    loop {
        let page = db
            .experience_history(
                "tenant",
                "scope",
                "attempt",
                ExperienceHistoryQuery { limit: 1, cursor },
            )
            .unwrap();
        assert_eq!(page.through_revision, 4);
        assert!(page.scanned_events <= 1);
        empty_continuation |= page.events.is_empty() && !page.complete;
        ids.extend(page.events.into_iter().map(|e| e.command.event_id));
        cursor = page.next_cursor;
        if page.complete {
            break;
        }
    }
    assert_eq!(ids, ["b", "z", "aa"]);
    assert!(empty_continuation);
}

#[test]
fn experience_history_rejects_foreign_cursors_and_invalid_bounds() {
    let (_dir, db) = setup();
    let receipt = create(&db);
    db.observe_experience(
        "tenant",
        "scope",
        "attempt",
        command("event", 1, &receipt.context_digest, "unknown"),
        actor("observer"),
    )
    .unwrap();
    for limit in [0, 65, usize::MAX] {
        assert!(
            db.experience_history(
                "tenant",
                "scope",
                "attempt",
                ExperienceHistoryQuery {
                    limit,
                    cursor: None
                }
            )
            .is_err()
        );
    }
    let page = db
        .experience_history(
            "tenant",
            "scope",
            "attempt",
            ExperienceHistoryQuery {
                limit: 1,
                cursor: None,
            },
        )
        .unwrap();
    db.create_experience("tenant", "other", input("attempt"), actor("writer"))
        .unwrap();
    assert!(matches!(
        db.experience_history(
            "tenant",
            "other",
            "attempt",
            ExperienceHistoryQuery {
                limit: 1,
                cursor: page.next_cursor.clone()
            }
        ),
        Err(Error::ConstraintViolation(_))
    ));
    let mut cursor = page.next_cursor.unwrap();
    cursor.after_event_id = "absent".into();
    assert!(
        db.experience_history(
            "tenant",
            "scope",
            "attempt",
            ExperienceHistoryQuery {
                limit: 1,
                cursor: Some(cursor)
            }
        )
        .is_err()
    );
    let empty = db
        .experience_history(
            "tenant",
            "other",
            "attempt",
            ExperienceHistoryQuery {
                limit: 64,
                cursor: None,
            },
        )
        .unwrap();
    assert!(empty.complete && empty.events.is_empty());
    assert_eq!(empty.through_revision, 1);
}

fn artifact(db: &LearningMemory) -> ToolArtifact {
    db.register_tool_artifact(
        "tenant",
        "scope",
        serde_json::from_value(json!({
            "id":"tool-v1","source":"def run(): return 1","dependency_lock":"",
            "runtime_image_digest":format!("sha256:{}", "c".repeat(64)),"entrypoint":"run",
            "input_schema":true,"output_schema":true,"source_refs":["fixture:request"],
            "parent_artifact_id":null,"repair_evidence_ref":null
        }))
        .unwrap(),
        ToolActor {
            subject_id: "developer".into(),
            credential_id: "key".into(),
        },
    )
    .unwrap()
}
fn binding_request(artifact: &ToolArtifact) -> ExperienceArtifactRequest {
    ExperienceArtifactRequest {
        id: "binding".into(),
        event_id: "event".into(),
        artifact_id: artifact.proposal.id.clone(),
        artifact_digest: artifact.artifact_digest.clone(),
        role: ExperienceArtifactRole::Candidate,
    }
}
#[test]
fn experience_artifact_bindings_verify_bytes_and_survive_rotation_and_restart() {
    let (dir, db) = setup();
    let receipt = create(&db);
    let artifact = artifact(&db);
    let event = db
        .observe_experience(
            "tenant",
            "scope",
            "attempt",
            command("event", 1, &receipt.context_digest, "failed"),
            actor("observer"),
        )
        .unwrap();
    let binding = db
        .bind_experience_artifact(
            "tenant",
            "scope",
            "attempt",
            binding_request(&artifact),
            actor("observer"),
        )
        .unwrap();
    assert_eq!(binding.source_digest, artifact.source_digest);
    assert_eq!(binding.event_digest, event.event_digest);
    assert_eq!(binding.receipt_digest, receipt.receipt_digest);
    // The link does not update the observation, execution result or qualification state.
    assert_eq!(
        db.experience("tenant", "scope", "attempt")
            .unwrap()
            .unwrap(),
        event.record
    );
    drop(db);
    let db = LearningMemory::open(dir.path()).unwrap();
    let mut rotated = actor("observer");
    rotated.credential_id = "rotated".into();
    assert_eq!(
        db.bind_experience_artifact(
            "tenant",
            "scope",
            "attempt",
            binding_request(&artifact),
            rotated
        )
        .unwrap(),
        binding
    );
    assert_eq!(
        db.experience_artifact_binding("tenant", "scope", "attempt", "binding")
            .unwrap(),
        Some(binding)
    );
}
#[test]
fn experience_artifact_bindings_reject_forged_content_authority_and_conflicts() {
    let (_dir, db) = setup();
    let receipt = create(&db);
    let artifact = artifact(&db);
    db.observe_experience(
        "tenant",
        "scope",
        "attempt",
        command("event", 1, &receipt.context_digest, "unknown"),
        actor("observer"),
    )
    .unwrap();
    let mut wrong = binding_request(&artifact);
    wrong.artifact_digest = "f".repeat(64);
    assert!(matches!(
        db.bind_experience_artifact("tenant", "scope", "attempt", wrong, actor("observer")),
        Err(Error::ConstraintViolation(_))
    ));
    assert!(
        db.experience_artifact_binding("tenant", "scope", "attempt", "binding")
            .unwrap()
            .is_none()
    );
    assert!(matches!(
        db.bind_experience_artifact(
            "tenant",
            "scope",
            "attempt",
            binding_request(&artifact),
            actor("writer")
        ),
        Err(Error::Unauthorized(_))
    ));
    db.bind_experience_artifact(
        "tenant",
        "scope",
        "attempt",
        binding_request(&artifact),
        actor("observer"),
    )
    .unwrap();
    let mut changed = binding_request(&artifact);
    changed.role = ExperienceArtifactRole::Output;
    assert!(matches!(
        db.bind_experience_artifact("tenant", "scope", "attempt", changed, actor("observer")),
        Err(Error::ConstraintViolation(_))
    ));
    for (tenant, scope) in [("other", "scope"), ("tenant", "other")] {
        assert!(
            db.experience_artifact_binding(tenant, scope, "attempt", "binding")
                .unwrap()
                .is_none()
        );
        assert!(
            db.bind_experience_artifact(
                tenant,
                scope,
                "attempt",
                binding_request(&artifact),
                actor("observer")
            )
            .is_err()
        );
    }
    let mut missing = binding_request(&artifact);
    missing.id = "other".into();
    missing.artifact_id = "absent".into();
    assert!(
        db.bind_experience_artifact("tenant", "scope", "attempt", missing, actor("observer"))
            .is_err()
    );
}

#[test]
fn experience_lineage_preserves_historical_branch_and_reports_truncation() {
    let (dir, db) = setup();
    let root = create(&db);
    let old = db
        .observe_experience(
            "tenant",
            "scope",
            "attempt",
            command("old", 1, &root.context_digest, "unknown"),
            actor("observer"),
        )
        .unwrap();
    let mut middle = input("middle");
    middle.parent = Some(ExperienceParent {
        attempt_id: "attempt".into(),
        event_id: "old".into(),
    });
    let middle = db
        .create_experience("tenant", "scope", middle, actor("writer"))
        .unwrap();
    let step = db
        .observe_experience(
            "tenant",
            "scope",
            "middle",
            command("step", 1, &middle.context_digest, "failed"),
            actor("observer"),
        )
        .unwrap();
    let mut leaf = input("leaf");
    leaf.parent = Some(ExperienceParent {
        attempt_id: "middle".into(),
        event_id: "step".into(),
    });
    db.create_experience("tenant", "scope", leaf, actor("writer"))
        .unwrap();
    db.observe_experience(
        "tenant",
        "scope",
        "attempt",
        command("new", 2, &root.context_digest, "succeeded"),
        actor("observer"),
    )
    .unwrap();
    let bounded = db.experience_lineage("tenant", "scope", "leaf", 1).unwrap();
    assert!(!bounded.complete);
    assert_eq!(bounded.ancestors, vec![step.clone()]);
    assert_eq!(bounded.next_parent_digest, Some(old.event_digest.clone()));
    let tail = db
        .experience_lineage("tenant", "scope", "middle", 1)
        .unwrap();
    assert!(tail.complete);
    assert_eq!(tail.ancestors, vec![old.clone()]);
    let full = db
        .experience_lineage("tenant", "scope", "leaf", 64)
        .unwrap();
    assert!(full.complete);
    assert_eq!(full.ancestors, vec![step, old]);
    drop(db);
    let db = LearningMemory::open(dir.path()).unwrap();
    assert_eq!(
        full,
        db.experience_lineage("tenant", "scope", "leaf", 64)
            .unwrap()
    );
    let root = db
        .experience_lineage("tenant", "scope", "attempt", 1)
        .unwrap();
    assert!(root.complete && root.ancestors.is_empty() && root.next_parent_digest.is_none());
}
#[test]
fn experience_lineage_enforces_namespace_and_work_limits() {
    let (_dir, db) = setup();
    create(&db);
    for limit in [0, 65, usize::MAX] {
        assert!(
            db.experience_lineage("tenant", "scope", "attempt", limit)
                .is_err()
        );
    }
    for (tenant, namespace, id) in [
        ("foreign", "scope", "attempt"),
        ("tenant", "other", "attempt"),
        ("tenant", "scope", "absent"),
    ] {
        assert!(matches!(
            db.experience_lineage(tenant, namespace, id, 64),
            Err(Error::KeyNotFound(_))
        ));
    }
}
