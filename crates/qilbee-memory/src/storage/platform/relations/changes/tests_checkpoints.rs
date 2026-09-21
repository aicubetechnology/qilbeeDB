use super::*;
fn command(
    cursor: RelationChangeCursor,
    previous: Option<&RelationCheckpoint>,
    id: &str,
) -> RelationCheckpointCommand {
    RelationCheckpointCommand {
        contract_version: 1,
        idempotency_key: id.into(),
        consumer_id: "cache".into(),
        expected_revision: previous.map_or(0, |p| p.revision),
        expected_checkpoint_digest: previous.map(|p| p.checkpoint_digest.clone()),
        operation: RelationCheckpointOperation::Advance { cursor },
    }
}
#[test]
fn relation_checkpoints_preserve_history_subjects_monotonicity_and_historical_replays() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let cmd = assertion(&db);
    let rel = db
        .apply_memory_relation_command("scope", &actor(), &cmd)
        .unwrap();
    let initial = command(tip(&db), None, "initial");
    let a = db
        .commit_relation_checkpoint("scope", &actor(), &initial)
        .unwrap();
    let mut rotated = actor();
    rotated.credential_id = Uuid::new_v4();
    assert_eq!(
        db.commit_relation_checkpoint("scope", &rotated, &initial)
            .unwrap(),
        a
    );
    let mut foreign = actor();
    foreign.subject_id = "other".into();
    assert!(
        db.read_relation_checkpoint("scope", &foreign.subject_id, "cache")
            .unwrap()
            .is_none()
    );
    let independent = db
        .commit_relation_checkpoint("scope", &foreign, &initial)
        .unwrap();
    assert_ne!(
        independent.checkpoint.checkpoint_digest,
        a.checkpoint.checkpoint_digest
    );
    mutate(&db, rel.relation_id, 1, "retire").unwrap();
    let diagnostic = db
        .diagnose_relation_consumer("scope", &actor().subject_id, "cache", Some(&tip(&db)))
        .unwrap();
    assert_eq!(diagnostic.pending_positions, Some(1));
    assert_eq!(
        diagnostic.checkpoint_relative_to_witness,
        Some(ConsumerCursorOrder::Before)
    );
    let b = db
        .commit_relation_checkpoint(
            "scope",
            &actor(),
            &command(tip(&db), Some(&a.checkpoint), "advance"),
        )
        .unwrap();
    assert_eq!(b.previous, Some(a.checkpoint.clone()));
    assert_eq!(b.checkpoint.revision, 2);
    assert_eq!(
        db.commit_relation_checkpoint("scope", &actor(), &initial)
            .unwrap(),
        a
    );
    assert_eq!(
        db.read_relation_checkpoint("scope", &actor().subject_id, "cache")
            .unwrap(),
        Some(b.checkpoint.clone())
    );
    assert!(matches!(
        db.commit_relation_checkpoint(
            "scope",
            &actor(),
            &command(a.checkpoint.cursor.clone(), Some(&b.checkpoint), "regress")
        ),
        Err(Error::CheckpointRegression(_))
    ));
    assert!(matches!(
        db.commit_relation_checkpoint("scope", &actor(), &command(tip(&db), None, "stale")),
        Err(Error::TransactionConflict(_))
    ));
    assert!(matches!(
        db.commit_relation_checkpoint(
            "scope",
            &actor(),
            &command(tip(&db), Some(&b.checkpoint), "initial")
        ),
        Err(Error::ConstraintViolation(_))
    ));
    assert_eq!(tip(&db).sequence, 2);
    drop(db);
    let db = open(dir.path());
    assert_eq!(
        db.relation_checkpoint_revision("scope", &actor().subject_id, "cache", 1)
            .unwrap(),
        Some(a)
    );
    assert_eq!(
        db.relation_checkpoint_revision("scope", &actor().subject_id, "cache", 2)
            .unwrap(),
        Some(b)
    );
}
#[test]
fn relation_checkpoint_race_has_one_winner_and_activation_is_idempotent() {
    use std::sync::{Arc, Barrier};
    let dir = TempDir::new().unwrap();
    let db = Arc::new(open(dir.path()));
    let barrier = Arc::new(Barrier::new(6));
    let mut workers = vec![];
    for n in 0..6 {
        let db = db.clone();
        let barrier = barrier.clone();
        workers.push(std::thread::spawn(move || {
            barrier.wait();
            let baseline = db.activate_relation_changes("scope").unwrap();
            let outcome = db.commit_relation_checkpoint(
                "scope",
                &actor(),
                &command(baseline.clone(), None, &format!("race-{n}")),
            );
            (baseline, outcome)
        }));
    }
    let results: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
    assert!(results.iter().all(|r| r.0 == results[0].0));
    assert_eq!(results.iter().filter(|r| r.1.is_ok()).count(), 1);
    assert!(
        results
            .iter()
            .filter_map(|r| r.1.as_ref().err())
            .all(|e| matches!(e, Error::TransactionConflict(_)))
    );
    assert_eq!(tip(&db).sequence, 0);
}

#[test]
fn relation_checkpoint_reconciliation_records_exact_previous_state_and_rejects_stale_repair() {
    use super::super::checkpoints::{CHECKPOINT, historical_key, key};
    let dir = TempDir::new().unwrap();
    let db = open(&dir.path().join("live"));
    let cmd = assertion(&db);
    let relation = db
        .apply_memory_relation_command("scope", &actor(), &cmd)
        .unwrap();
    let initial = db
        .commit_relation_checkpoint("scope", &actor(), &command(tip(&db), None, "initial"))
        .unwrap();
    let backup = dir.path().join("backup");
    rocksdb::checkpoint::Checkpoint::new(&db.db)
        .unwrap()
        .create_checkpoint(&backup)
        .unwrap();
    mutate(&db, relation.relation_id, 1, "retire").unwrap();
    let lost = db
        .commit_relation_checkpoint(
            "scope",
            &actor(),
            &command(tip(&db), Some(&initial.checkpoint), "lost"),
        )
        .unwrap();
    let restored = open(&backup);
    mutate(&restored, relation.relation_id, 1, "retire").unwrap();
    let witness = db
        .diagnose_relation_consumer("scope", &actor().subject_id, "cache", Some(&tip(&restored)))
        .unwrap();
    assert_eq!(
        witness.witness_status,
        ConsumerWitnessStatus::HistoryIncompatible
    );
    // A retained consumer checkpoint can outlive a separately restored stream. Preserve it as evidence.
    let cf = restored.cf(crate::storage::cf::AGENT_META).unwrap();
    restored
        .db
        .put_cf(
            cf,
            key(CHECKPOINT, "scope", &actor().subject_id, "cache").unwrap(),
            encode(&lost.checkpoint).unwrap(),
        )
        .unwrap();
    restored
        .db
        .put_cf(
            cf,
            historical_key("scope", &actor().subject_id, "cache", 2).unwrap(),
            encode(&lost).unwrap(),
        )
        .unwrap();
    assert!(matches!(
        restored.read_relation_checkpoint("scope", &actor().subject_id, "cache"),
        Err(Error::JournalHistoryConflict(_))
    ));
    let diagnostic = restored
        .diagnose_relation_consumer("scope", &actor().subject_id, "cache", Some(&tip(&restored)))
        .unwrap();
    assert_eq!(
        diagnostic.checkpoint_status,
        ConsumerCheckpointStatus::HistoryIncompatible
    );
    assert_eq!(diagnostic.pending_positions, None);
    assert_eq!(diagnostic.checkpoint, Some(lost.checkpoint.clone()));
    let mut repair = command(tip(&restored), Some(&lost.checkpoint), "repair");
    assert!(matches!(
        restored.commit_relation_checkpoint("scope", &actor(), &repair),
        Err(Error::JournalHistoryConflict(_))
    ));
    repair.operation = RelationCheckpointOperation::Reconcile {
        cursor: tip(&restored),
        evidence_ref: "trace://sink-reconciled".into(),
    };
    let done = restored
        .commit_relation_checkpoint("scope", &actor(), &repair)
        .unwrap();
    assert_eq!(done.previous, Some(lost.checkpoint));
    assert_eq!(done.checkpoint.revision, 3);
    assert_eq!(
        done.reconciliation_evidence,
        Some("trace://sink-reconciled".into())
    );
    assert_eq!(
        restored
            .commit_relation_checkpoint("scope", &actor(), &repair)
            .unwrap(),
        done
    );
    repair.idempotency_key = "stale-repair".into();
    assert!(matches!(
        restored.commit_relation_checkpoint("scope", &actor(), &repair),
        Err(Error::TransactionConflict(_))
    ));
    let baseline = restored.activate_relation_changes("scope").unwrap();
    let mut rewind = command(baseline.clone(), Some(&done.checkpoint), "rewind");
    assert!(matches!(
        restored.commit_relation_checkpoint("scope", &actor(), &rewind),
        Err(Error::CheckpointRegression(_))
    ));
    rewind.operation = RelationCheckpointOperation::Reconcile {
        cursor: baseline,
        evidence_ref: "trace://explicit-rebuild".into(),
    };
    let rewound = restored
        .commit_relation_checkpoint("scope", &actor(), &rewind)
        .unwrap();
    assert_eq!(rewound.checkpoint.cursor.sequence, 0);
    assert_eq!(rewound.checkpoint.revision, 4);
    assert_eq!(
        restored
            .relation_checkpoint_revision("scope", &actor().subject_id, "cache", 3)
            .unwrap(),
        Some(done)
    );
    assert_eq!(tip(&restored).sequence, 2);
}
#[test]
fn relation_checkpoint_missing_heads_history_and_tampered_receipts_fail_closed() {
    use super::super::checkpoints::{CHECKPOINT, CHECKPOINT_RECEIPT, historical_key, key};
    for mode in ["current", "history", "receipt", "digest"] {
        let dir = TempDir::new().unwrap();
        let db = open(dir.path());
        let baseline = db.activate_relation_changes("scope").unwrap();
        let cmd = command(baseline, None, "initialize");
        let receipt = db
            .commit_relation_checkpoint("scope", &actor(), &cmd)
            .unwrap();
        let cf = db.cf(crate::storage::cf::AGENT_META).unwrap();
        match mode {
            "current" => db
                .db
                .delete_cf(
                    cf,
                    key(CHECKPOINT, "scope", &actor().subject_id, "cache").unwrap(),
                )
                .unwrap(),
            "history" => db
                .db
                .delete_cf(
                    cf,
                    historical_key("scope", &actor().subject_id, "cache", 1).unwrap(),
                )
                .unwrap(),
            "receipt" => {
                let k = key(
                    CHECKPOINT_RECEIPT,
                    "scope",
                    &actor().subject_id,
                    "initialize",
                )
                .unwrap();
                let mut value: serde_json::Value =
                    decode(&db.db.get_cf(cf, &k).unwrap().unwrap()).unwrap();
                value["receipt"]["checkpoint"]["revision"] = 999.into();
                db.db.put_cf(cf, k, encode(&value).unwrap()).unwrap();
            }
            _ => {
                let mut changed = receipt.checkpoint.clone();
                changed.cursor.sequence = 1;
                db.db
                    .put_cf(
                        cf,
                        key(CHECKPOINT, "scope", &actor().subject_id, "cache").unwrap(),
                        encode(&changed).unwrap(),
                    )
                    .unwrap();
            }
        }
        if mode == "receipt" {
            assert!(matches!(
                db.commit_relation_checkpoint("scope", &actor(), &cmd),
                Err(Error::DataCorruption(_))
            ));
        } else {
            assert!(
                matches!(
                    db.diagnose_relation_consumer("scope", &actor().subject_id, "cache", None),
                    Err(Error::DataCorruption(_))
                ),
                "{mode}"
            );
            assert!(
                matches!(
                    db.commit_relation_checkpoint(
                        "scope",
                        &actor(),
                        &command(
                            receipt.checkpoint.cursor.clone(),
                            Some(&receipt.checkpoint),
                            "blocked"
                        )
                    ),
                    Err(Error::DataCorruption(_))
                ),
                "{mode}"
            );
        }
    }
}
#[test]
fn relation_checkpoint_invalid_expectations_evidence_and_foreign_history_publish_nothing() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let baseline = db.activate_relation_changes("scope").unwrap();
    let foreign = db.activate_relation_changes("foreign").unwrap();
    assert!(matches!(
        db.commit_relation_checkpoint("scope", &actor(), &command(foreign, None, "wrong-journal")),
        Err(Error::JournalHistoryConflict(_))
    ));
    for mode in [
        "version",
        "consumer",
        "key",
        "expectation",
        "evidence",
        "reconcile_initial",
    ] {
        let mut cmd = command(baseline.clone(), None, "invalid");
        match mode {
            "version" => cmd.contract_version = 2,
            "consumer" => cmd.consumer_id = " ".into(),
            "key" => cmd.idempotency_key = "x".repeat(257),
            "expectation" => cmd.expected_revision = 1,
            "evidence" => {
                cmd.expected_revision = 1;
                cmd.expected_checkpoint_digest = Some("a".repeat(64));
                cmd.operation = RelationCheckpointOperation::Reconcile {
                    cursor: baseline.clone(),
                    evidence_ref: "\n".into(),
                };
            }
            _ => {
                cmd.operation = RelationCheckpointOperation::Reconcile {
                    cursor: baseline.clone(),
                    evidence_ref: "trace://first".into(),
                }
            }
        }
        assert!(
            matches!(
                db.commit_relation_checkpoint("scope", &actor(), &cmd),
                Err(Error::ValidationError(_))
            ),
            "{mode}"
        );
    }
    assert!(
        db.read_relation_checkpoint("scope", &actor().subject_id, "cache")
            .unwrap()
            .is_none()
    );
    assert_eq!(tip(&db).sequence, 0);
}
