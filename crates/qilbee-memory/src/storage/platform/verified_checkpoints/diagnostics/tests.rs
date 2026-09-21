use super::*;
use crate::storage::platform::semantic_tests::{actor, create, open};
use tempfile::TempDir;

fn inspect(
    db: &RocksDbMemoryStorage,
    witness: Option<&VerifiedMemoryCursor>,
) -> MemoryConsumerDiagnostics {
    db.diagnose_memory_consumer("scope", &actor().subject_id, "cache", witness)
        .unwrap()
}
fn advance(db: &RocksDbMemoryStorage, key: &str) -> VerifiedMemoryCheckpoint {
    let state = inspect(db, None);
    db.commit_verified_memory_checkpoint(
        "scope",
        &actor(),
        &VerifiedMemoryCheckpointCommand {
            contract_version: 2,
            idempotency_key: key.into(),
            consumer_id: "cache".into(),
            expected_revision: state.checkpoint.as_ref().map_or(0, |c| c.revision),
            expected_checkpoint_digest: state.checkpoint.map(|c| c.checkpoint_digest),
            cursor: state.high_watermark.unwrap(),
        },
    )
    .unwrap()
    .checkpoint
}
fn overwrite(db: &RocksDbMemoryStorage, checkpoint: &VerifiedMemoryCheckpoint) {
    db.db
        .put_cf(
            db.cf(crate::storage::cf::AGENT_META).unwrap(),
            checkpoint_key(0x28, "scope", &actor().subject_id, "cache").unwrap(),
            encode(checkpoint).unwrap(),
        )
        .unwrap();
}

#[test]
fn consumer_diagnostics_distinguish_unknown_from_zero_and_preserve_restart() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let missing = inspect(&db, None);
    assert!(!missing.active);
    assert_eq!(missing.checkpoint_status, ConsumerCheckpointStatus::Missing);
    assert_eq!(missing.pending_positions, None);
    assert_eq!(missing.witness_status, ConsumerWitnessStatus::NotProvided);
    let baseline = db.activate_verified_memory_journal("scope").unwrap();
    let active = inspect(&db, Some(&baseline));
    assert!(active.active);
    assert_eq!(active.pending_positions, None);
    assert_eq!(active.witness_status, ConsumerWitnessStatus::Compatible);
    let first = advance(&db, "initial");
    let zero = inspect(&db, Some(&baseline));
    assert_eq!(zero.checkpoint.as_ref(), Some(&first));
    assert_eq!(zero.pending_positions, Some(0));
    assert_eq!(
        zero.checkpoint_relative_to_witness,
        Some(ConsumerCursorOrder::Equal)
    );
    for n in 0..3 {
        create(&db, "scope", &format!("source-{n}"));
    }
    let pending = inspect(&db, None);
    let tip = pending.high_watermark.clone().unwrap();
    assert_eq!(pending.pending_positions, Some(3));
    assert_eq!(
        inspect(&db, Some(&tip)).checkpoint_relative_to_witness,
        Some(ConsumerCursorOrder::Before)
    );
    advance(&db, "caught-up");
    let after = inspect(&db, Some(&baseline));
    assert_eq!(after.pending_positions, Some(0));
    assert_eq!(
        after.checkpoint_relative_to_witness,
        Some(ConsumerCursorOrder::After)
    );
    assert_eq!(after.high_watermark, Some(tip));
    assert_eq!(inspect(&db, Some(&baseline)), after);
    assert_eq!(
        db.diagnose_memory_consumer("scope", "other", "cache", None)
            .unwrap()
            .checkpoint_status,
        ConsumerCheckpointStatus::Missing
    );
    assert!(
        !db.diagnose_memory_consumer("other", &actor().subject_id, "cache", None)
            .unwrap()
            .active
    );
    drop(db);
    assert_eq!(inspect(&open(dir.path()), Some(&baseline)), after);
}

#[test]
fn consumer_diagnostics_use_one_snapshot_for_tip_and_progress() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    db.activate_verified_memory_journal("scope").unwrap();
    let first = advance(&db, "initial");
    create(&db, "scope", "before-snapshot");
    let snapshot = db.memory_snapshot();
    create(&db, "scope", "after-snapshot");
    let latest = advance(&db, "latest");
    let old = snapshot
        .diagnose_consumer("scope", &actor().subject_id, "cache", Some(&first.cursor))
        .unwrap();
    assert_eq!(old.checkpoint, Some(first));
    assert_eq!(old.high_watermark.unwrap().sequence, 1);
    assert_eq!(old.pending_positions, Some(1));
    let now = inspect(&db, None);
    assert_eq!(now.checkpoint, Some(latest));
    assert_eq!(now.high_watermark.unwrap().sequence, 2);
    assert_eq!(now.pending_positions, Some(0));
}

#[test]
fn consumer_diagnostics_detect_lost_witness_after_restored_sequences_catch_up() {
    let dir = TempDir::new().unwrap();
    let db = open(&dir.path().join("live"));
    create(&db, "scope", "common");
    let common = advance(&db, "common");
    let backup = dir.path().join("backup");
    rocksdb::checkpoint::Checkpoint::new(&db.db)
        .unwrap()
        .create_checkpoint(&backup)
        .unwrap();
    create(&db, "scope", "lost");
    let lost = advance(&db, "lost");
    let restored = open(&backup);
    create(&restored, "scope", "branch");
    let branch = advance(&restored, "branch");
    assert_eq!(lost.revision, branch.revision);
    assert_eq!(lost.cursor.sequence, branch.cursor.sequence);
    assert_ne!(lost.checkpoint_digest, branch.checkpoint_digest);
    let diagnosis = inspect(&restored, Some(&lost.cursor));
    assert_eq!(
        diagnosis.checkpoint_status,
        ConsumerCheckpointStatus::Compatible
    );
    assert_eq!(diagnosis.pending_positions, Some(0));
    assert_eq!(
        diagnosis.witness_status,
        ConsumerWitnessStatus::HistoryIncompatible
    );
    assert_eq!(diagnosis.checkpoint_relative_to_witness, None);
    assert_eq!(
        inspect(&restored, Some(&common.cursor)).witness_status,
        ConsumerWitnessStatus::Compatible
    );
    assert_eq!(inspect(&restored, None).checkpoint, Some(branch));
}

#[test]
fn incompatible_progress_requires_explicit_evidenced_cas_recovery() {
    let dir = TempDir::new().unwrap();
    let db = open(&dir.path().join("live"));
    create(&db, "scope", "common");
    advance(&db, "common");
    let backup = dir.path().join("backup");
    rocksdb::checkpoint::Checkpoint::new(&db.db)
        .unwrap()
        .create_checkpoint(&backup)
        .unwrap();
    create(&db, "scope", "lost");
    let lost = advance(&db, "lost");
    let restored = open(&backup);
    create(&restored, "scope", "branch");
    // Simulate a self-consistent checkpoint retained from a different restored suffix.
    overwrite(&restored, &lost);
    let diagnosis = inspect(&restored, Some(&lost.cursor));
    assert_eq!(
        diagnosis.checkpoint_status,
        ConsumerCheckpointStatus::HistoryIncompatible
    );
    assert_eq!(diagnosis.checkpoint, Some(lost.clone()));
    assert_eq!(diagnosis.pending_positions, None);
    assert_eq!(
        diagnosis.witness_status,
        ConsumerWitnessStatus::HistoryIncompatible
    );
    assert!(matches!(
        restored.read_verified_memory_checkpoint("scope", &actor().subject_id, "cache"),
        Err(Error::DataCorruption(_))
    ));
    let mut recovery = VerifiedCheckpointRecoveryCommand {
        contract_version: 2,
        idempotency_key: "reconcile".into(),
        consumer_id: "cache".into(),
        expected_revision: lost.revision,
        expected_checkpoint_digest: lost.checkpoint_digest.clone(),
        cursor: diagnosis.high_watermark.unwrap(),
        evidence_ref: "fixture://reconciled-branch".into(),
    };
    recovery.expected_checkpoint_digest = "0".repeat(64);
    assert!(matches!(
        restored.recover_verified_memory_checkpoint("scope", &actor(), &recovery),
        Err(Error::TransactionConflict(_))
    ));
    assert_eq!(inspect(&restored, None).checkpoint, Some(lost.clone()));
    recovery.expected_checkpoint_digest = lost.checkpoint_digest.clone();
    let valid_target = recovery.cursor.clone();
    recovery.cursor = lost.cursor.clone();
    assert!(matches!(
        restored.recover_verified_memory_checkpoint("scope", &actor(), &recovery),
        Err(Error::JournalHistoryConflict(_))
    ));
    assert_eq!(inspect(&restored, None).checkpoint, Some(lost.clone()));
    recovery.cursor = valid_target;
    let accepted = restored
        .recover_verified_memory_checkpoint("scope", &actor(), &recovery)
        .unwrap();
    assert_eq!(accepted.previous, lost);
    assert_eq!(accepted.checkpoint.revision, accepted.previous.revision + 1);
    assert_eq!(inspect(&restored, None).pending_positions, Some(0));
    create(&restored, "scope", "after-recovery");
    let newest = advance(&restored, "advanced");
    assert_eq!(
        restored
            .recover_verified_memory_checkpoint("scope", &actor(), &recovery)
            .unwrap(),
        accepted
    );
    assert_eq!(inspect(&restored, None).checkpoint, Some(newest));
    assert_eq!(
        restored
            .read_verified_checkpoint_recovery(
                "scope",
                &actor().subject_id,
                "cache",
                accepted.checkpoint.revision
            )
            .unwrap(),
        Some(accepted.clone())
    );
    drop(restored);
    let reopened = open(&backup);
    assert_eq!(
        reopened
            .recover_verified_memory_checkpoint("scope", &actor(), &recovery)
            .unwrap(),
        accepted
    );
    assert_eq!(
        inspect(&reopened, None).checkpoint_status,
        ConsumerCheckpointStatus::Compatible
    );
}

#[test]
fn diagnostics_reject_malformed_requests_and_never_hide_encountered_corruption() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let baseline = db.activate_verified_memory_journal("scope").unwrap();
    let first = advance(&db, "initial");
    for consumer in ["", "\n", &"x".repeat(129)] {
        assert!(matches!(
            db.diagnose_memory_consumer("scope", &actor().subject_id, consumer, None),
            Err(Error::ValidationError(_))
        ));
    }
    let mut bad = baseline.clone();
    bad.prefix_digest = "G".repeat(64);
    assert!(matches!(
        db.diagnose_memory_consumer("inactive", &actor().subject_id, "cache", Some(&bad)),
        Err(Error::ValidationError(_))
    ));
    let mut bad = first.clone();
    bad.revision += 1;
    overwrite(&db, &bad);
    assert!(matches!(
        db.diagnose_memory_consumer("scope", &actor().subject_id, "cache", None),
        Err(Error::DataCorruption(_))
    ));
    let command = VerifiedCheckpointRecoveryCommand {
        contract_version: 2,
        idempotency_key: "must-not-repair-digest".into(),
        consumer_id: "cache".into(),
        expected_revision: bad.revision,
        expected_checkpoint_digest: bad.checkpoint_digest.clone(),
        cursor: baseline,
        evidence_ref: "fixture://invalid".into(),
    };
    assert!(matches!(
        db.recover_verified_memory_checkpoint("scope", &actor(), &command),
        Err(Error::DataCorruption(_))
    ));
    overwrite(&db, &first);
    create(&db, "scope", "event");
    let mut key = record_prefix(0x27, "scope");
    key.extend_from_slice(&1u64.to_be_bytes());
    db.db
        .delete_cf(db.cf(crate::storage::cf::AGENT_META).unwrap(), key)
        .unwrap();
    assert!(matches!(
        db.diagnose_memory_consumer("scope", &actor().subject_id, "cache", None),
        Err(Error::DataCorruption(_))
    ));
}

#[test]
fn pending_positions_are_bounded_observations_not_a_hidden_history_audit() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    db.activate_verified_memory_journal("scope").unwrap();
    advance(&db, "initial");
    for n in 0..32 {
        create(&db, "scope", &format!("event-{n}"));
    }
    // The endpoint observes its boundaries. A separate bounded audit detects middle damage.
    let mut key = record_prefix(0x27, "scope");
    key.extend_from_slice(&16u64.to_be_bytes());
    db.db
        .delete_cf(db.cf(crate::storage::cf::AGENT_META).unwrap(), key)
        .unwrap();
    assert_eq!(inspect(&db, None).pending_positions, Some(32));
    assert!(
        db.verified_memory_changes(
            "scope",
            &VerifiedMemoryChangesQuery {
                after: None,
                through: None,
                limit: 32
            }
        )
        .is_err()
    );
    let middle = VerifiedMemoryCursor {
        sequence: 16,
        ..inspect(&db, None).high_watermark.unwrap()
    };
    assert!(matches!(
        db.diagnose_memory_consumer("scope", &actor().subject_id, "cache", Some(&middle)),
        Err(Error::DataCorruption(_))
    ));
}
