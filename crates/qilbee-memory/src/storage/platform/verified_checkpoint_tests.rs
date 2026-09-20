use super::semantic_tests::{actor, create, open};
use super::*;
use tempfile::TempDir;
fn tip(db: &RocksDbMemoryStorage) -> VerifiedMemoryCursor {
    db.verified_memory_changes(
        "scope",
        &VerifiedMemoryChangesQuery {
            after: None,
            through: None,
            limit: 1,
        },
    )
    .unwrap()
    .high_watermark
    .unwrap()
}
fn command(
    db: &RocksDbMemoryStorage,
    who: &RecordAuthor,
    key: &str,
) -> VerifiedMemoryCheckpointCommand {
    let current = db
        .read_verified_memory_checkpoint("scope", &who.subject_id, "cache")
        .unwrap();
    VerifiedMemoryCheckpointCommand {
        contract_version: 2,
        idempotency_key: key.into(),
        consumer_id: "cache".into(),
        expected_revision: current.as_ref().map_or(0, |c| c.revision),
        expected_checkpoint_digest: current.map(|c| c.checkpoint_digest),
        cursor: tip(db),
    }
}
#[test]
fn verified_checkpoint_retries_preserve_original_receipts_and_current_progress() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let who = actor();
    create(&db, "scope", "first");
    let first = command(&db, &who, "first");
    let original = db
        .commit_verified_memory_checkpoint("scope", &who, &first)
        .unwrap();
    create(&db, "scope", "second");
    let next = command(&db, &who, "next");
    let latest = db
        .commit_verified_memory_checkpoint("scope", &who, &next)
        .unwrap();
    let rotated = RecordAuthor {
        credential_id: Uuid::new_v4(),
        ..who.clone()
    };
    assert_eq!(
        db.commit_verified_memory_checkpoint("scope", &rotated, &first)
            .unwrap(),
        original
    );
    assert_eq!(
        db.read_verified_memory_checkpoint("scope", &who.subject_id, "cache")
            .unwrap(),
        Some(latest.checkpoint.clone())
    );
    let mut backwards = command(&db, &who, "backwards");
    backwards.cursor = first.cursor.clone();
    assert!(matches!(
        db.commit_verified_memory_checkpoint("scope", &who, &backwards),
        Err(Error::ConstraintViolation(_))
    ));
    let mut altered = first.clone();
    altered.cursor = tip(&db);
    assert!(matches!(
        db.commit_verified_memory_checkpoint("scope", &who, &altered),
        Err(Error::ConstraintViolation(_))
    ));
    assert!(
        db.read_verified_memory_checkpoint("scope", "other", "cache")
            .unwrap()
            .is_none()
    );
    assert!(
        db.read_verified_memory_checkpoint("other", &who.subject_id, "cache")
            .unwrap()
            .is_none()
    );
    assert!(
        db.read_memory_checkpoint("scope", &who.subject_id, "cache")
            .unwrap()
            .is_none()
    );
    drop(db);
    let db = open(dir.path());
    assert_eq!(
        db.commit_verified_memory_checkpoint("scope", &who, &first)
            .unwrap(),
        original
    );
    assert_eq!(
        db.read_verified_memory_checkpoint("scope", &who.subject_id, "cache")
            .unwrap(),
        Some(latest.checkpoint)
    );
}
#[test]
fn verified_checkpoint_cas_detects_same_revision_after_divergent_restore() {
    let dir = TempDir::new().unwrap();
    let db = open(&dir.path().join("live"));
    let who = actor();
    create(&db, "scope", "common");
    db.commit_verified_memory_checkpoint("scope", &who, &command(&db, &who, "common"))
        .unwrap();
    let backup = dir.path().join("backup");
    rocksdb::checkpoint::Checkpoint::new(&db.db)
        .unwrap()
        .create_checkpoint(&backup)
        .unwrap();
    create(&db, "scope", "original");
    let original = db
        .commit_verified_memory_checkpoint("scope", &who, &command(&db, &who, "original"))
        .unwrap();
    let restored = open(&backup);
    create(&restored, "scope", "divergent");
    let mut lost = command(&restored, &who, "lost");
    lost.cursor = original.checkpoint.cursor.clone();
    assert!(matches!(
        restored.commit_verified_memory_checkpoint("scope", &who, &lost),
        Err(Error::ConstraintViolation(_))
    ));
    let branch = restored
        .commit_verified_memory_checkpoint("scope", &who, &command(&restored, &who, "divergent"))
        .unwrap();
    assert_eq!(branch.checkpoint.revision, original.checkpoint.revision);
    let mut stale = command(&restored, &who, "stale-cas");
    stale.expected_checkpoint_digest = Some(original.checkpoint.checkpoint_digest);
    assert!(matches!(
        restored.commit_verified_memory_checkpoint("scope", &who, &stale),
        Err(Error::TransactionConflict(_))
    ));
    assert_eq!(
        restored
            .read_verified_memory_checkpoint("scope", &who.subject_id, "cache")
            .unwrap(),
        Some(branch.checkpoint)
    );
}
#[test]
fn verified_checkpoint_concurrent_cas_has_one_winner_and_no_journal_feedback() {
    use std::sync::{Arc, Barrier};
    let dir = TempDir::new().unwrap();
    let db = Arc::new(open(dir.path()));
    let who = actor();
    let baseline = db.activate_verified_memory_journal("scope").unwrap();
    let first = command(&db, &who, "first");
    let gate = Arc::new(Barrier::new(10));
    let handles: Vec<_> = (0..10)
        .map(|n| {
            let db = db.clone();
            let who = who.clone();
            let gate = gate.clone();
            let mut c = first.clone();
            c.idempotency_key = format!("cas-{n}");
            std::thread::spawn(move || {
                gate.wait();
                db.commit_verified_memory_checkpoint("scope", &who, &c)
            })
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert!(
        results
            .iter()
            .filter_map(|r| r.as_ref().err())
            .all(|e| matches!(e, Error::TransactionConflict(_)))
    );
    assert_eq!(tip(&db), baseline);
}
#[test]
fn verified_checkpoint_rejects_malformed_expectations_and_stored_corruption() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let who = actor();
    create(&db, "scope", "first");
    let mut c = command(&db, &who, "first");
    c.expected_checkpoint_digest = Some("f".repeat(64));
    assert!(matches!(
        db.commit_verified_memory_checkpoint("scope", &who, &c),
        Err(Error::ValidationError(_))
    ));
    c.expected_checkpoint_digest = None;
    db.commit_verified_memory_checkpoint("scope", &who, &c)
        .unwrap();
    let mut bad = command(&db, &who, "bad");
    bad.expected_checkpoint_digest = None;
    assert!(matches!(
        db.commit_verified_memory_checkpoint("scope", &who, &bad),
        Err(Error::ValidationError(_))
    ));
    let cf = db.cf(super::super::cf::AGENT_META).unwrap();
    let mut key = vec![0x28];
    key.extend(encode(&("scope", &who.subject_id, "cache")).unwrap());
    let mut stored: serde_json::Value = decode(&db.db.get_cf(cf, &key).unwrap().unwrap()).unwrap();
    stored["cursor"]["sequence"] = 0.into();
    db.db.put_cf(cf, key, encode(&stored).unwrap()).unwrap();
    assert!(matches!(
        db.read_verified_memory_checkpoint("scope", &who.subject_id, "cache"),
        Err(Error::DataCorruption(_))
    ));
}
