use super::semantic_tests::{actor, create, open};
use super::*;
use tempfile::TempDir;
fn cursor(db: &RocksDbMemoryStorage) -> MemoryChangeCursor {
    db.memory_changes(
        "scope",
        &MemoryChangesQuery {
            after: None,
            through: None,
            limit: 256,
        },
    )
    .unwrap()
    .high_watermark
    .unwrap()
}
fn command(cursor: MemoryChangeCursor, key: &str, revision: u64) -> MemoryCheckpointCommand {
    MemoryCheckpointCommand {
        contract_version: 1,
        idempotency_key: key.into(),
        consumer_id: "cache".into(),
        expected_revision: revision,
        cursor,
    }
}
#[test]
fn checkpoint_recovery_rotation_and_historical_retries_do_not_regress_progress() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let who = actor();
    create(&db, "scope", "first");
    let first = command(cursor(&db), "first", 0);
    let receipt = db.commit_memory_checkpoint("scope", &who, &first).unwrap();
    assert_eq!(receipt.checkpoint.revision, 1);
    create(&db, "scope", "second");
    let second = command(cursor(&db), "second", 1);
    let latest = db.commit_memory_checkpoint("scope", &who, &second).unwrap();
    assert_eq!(latest.checkpoint.cursor.sequence, 2);
    let rotated = RecordAuthor {
        credential_id: Uuid::new_v4(),
        ..who.clone()
    };
    assert_eq!(
        db.commit_memory_checkpoint("scope", &rotated, &first)
            .unwrap(),
        receipt
    );
    assert_eq!(
        db.read_memory_checkpoint("scope", &who.subject_id, "cache")
            .unwrap()
            .unwrap(),
        latest.checkpoint
    );
    assert!(matches!(
        db.commit_memory_checkpoint("scope", &who, &command(first.cursor.clone(), "regress", 2)),
        Err(Error::ConstraintViolation(_))
    ));
    let mut altered = first.clone();
    altered.consumer_id = "other".into();
    assert!(matches!(
        db.commit_memory_checkpoint("scope", &who, &altered),
        Err(Error::ConstraintViolation(_))
    ));
    drop(db);
    let db = open(dir.path());
    assert_eq!(
        db.commit_memory_checkpoint("scope", &rotated, &first)
            .unwrap(),
        receipt
    );
    assert_eq!(
        db.read_memory_checkpoint("scope", &who.subject_id, "cache")
            .unwrap()
            .unwrap(),
        latest.checkpoint
    );
    assert_eq!(cursor(&db).sequence, 2);
}
#[test]
fn checkpoints_isolate_subjects_consumers_and_namespace_and_reject_invalid_cursors() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let who = actor();
    create(&db, "scope", "first");
    let original = cursor(&db);
    let mut zero = original.clone();
    zero.sequence = 0;
    db.commit_memory_checkpoint("scope", &who, &command(zero, "initial", 0))
        .unwrap();
    assert!(
        db.read_memory_checkpoint("scope", "another-subject", "cache")
            .unwrap()
            .is_none()
    );
    assert!(
        db.read_memory_checkpoint("foreign", &who.subject_id, "cache")
            .unwrap()
            .is_none()
    );
    assert!(
        db.read_memory_checkpoint("scope", &who.subject_id, "another-consumer")
            .unwrap()
            .is_none()
    );
    let other = RecordAuthor {
        subject_id: "another-subject".into(),
        ..actor()
    };
    assert!(
        db.commit_memory_checkpoint("scope", &other, &command(original.clone(), "initial", 0))
            .is_ok()
    );
    let mut ahead = original.clone();
    ahead.sequence += 1;
    assert!(matches!(
        db.commit_memory_checkpoint("scope", &who, &command(ahead, "ahead", 1)),
        Err(Error::ConstraintViolation(_))
    ));
    let mut foreign = original.clone();
    foreign.journal_id = Uuid::new_v4();
    assert!(matches!(
        db.commit_memory_checkpoint("scope", &who, &command(foreign, "foreign", 1)),
        Err(Error::ConstraintViolation(_))
    ));
    assert!(matches!(
        db.commit_memory_checkpoint("empty", &who, &command(original.clone(), "empty", 0)),
        Err(Error::ConstraintViolation(_))
    ));
    assert!(matches!(
        db.commit_memory_checkpoint("scope", &who, &command(original, "stale-cas", 0)),
        Err(Error::TransactionConflict(_))
    ));
}
#[test]
fn concurrent_checkpoint_compare_and_set_has_one_winner_without_feed_feedback() {
    use std::sync::{Arc, Barrier};
    let dir = TempDir::new().unwrap();
    let db = Arc::new(open(dir.path()));
    let who = actor();
    create(&db, "scope", "first");
    let current = cursor(&db);
    let gate = Arc::new(Barrier::new(10));
    let threads: Vec<_> = (0..10)
        .map(|n| {
            let db = db.clone();
            let who = who.clone();
            let gate = gate.clone();
            let current = current.clone();
            std::thread::spawn(move || {
                gate.wait();
                db.commit_memory_checkpoint(
                    "scope",
                    &who,
                    &command(current, &format!("cas-{n}"), 0),
                )
            })
        })
        .collect();
    let results: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert!(
        results
            .iter()
            .filter(|r| r.is_err())
            .all(|r| matches!(r, Err(Error::TransactionConflict(_))))
    );
    assert_eq!(cursor(&db), current);
}
#[test]
fn checkpoint_integrity_and_identity_validation_fail_closed() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let who = actor();
    create(&db, "scope", "first");
    let c = command(cursor(&db), "commit", 0);
    let mut invalid = c.clone();
    invalid.consumer_id = "\n".into();
    assert!(matches!(
        db.commit_memory_checkpoint("scope", &who, &invalid),
        Err(Error::ValidationError(_))
    ));
    db.commit_memory_checkpoint("scope", &who, &c).unwrap();
    let mut key = vec![0x24];
    key.extend(encode(&("scope", &who.subject_id, "cache")).unwrap());
    let cf = db.cf(super::super::cf::AGENT_META).unwrap();
    let mut stored: serde_json::Value =
        serde_json::from_slice(&db.db.get_cf(cf, &key).unwrap().unwrap()).unwrap();
    stored["cursor"]["sequence"] = 0.into();
    db.db
        .put_cf(cf, key, serde_json::to_vec(&stored).unwrap())
        .unwrap();
    assert!(matches!(
        db.read_memory_checkpoint("scope", &who.subject_id, "cache"),
        Err(Error::DataCorruption(_))
    ));
    assert!(matches!(
        db.commit_memory_checkpoint("scope", &who, &command(cursor(&db), "new", 1)),
        Err(Error::DataCorruption(_))
    ));
}

#[test]
fn consumer_reconciliation_advances_through_empty_bounded_candidate_pages() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let who = actor();
    for n in 0..5 {
        let receipt = create(&db, "scope", &format!("hidden-{n}"));
        db.review_memory_record(
            "scope",
            &who,
            &MemoryReviewCommand {
                contract_version: 1,
                idempotency_key: format!("reject-{n}"),
                record_id: receipt.record_id,
                expected_revision: 1,
                disposition: MemoryReviewDisposition::Rejected,
                evidence_ref: "test://reconciliation".into(),
            },
        )
        .unwrap();
    }
    let mut query: MemoryQuery = serde_json::from_value(serde_json::json!({"limit":10})).unwrap();
    assert_eq!(query.scan_limit, 10_000);
    query.scan_limit = 1;
    let mut cursors = std::collections::BTreeSet::new();
    let mut examined = 0;
    loop {
        let page = db.query_memory_records("scope", &query).unwrap();
        assert!(page.records.is_empty());
        assert!(page.scanned_records <= 1);
        examined += page.scanned_records;
        if let Some(cursor) = page.next_after {
            assert!(cursors.insert(cursor));
            query.after = Some(cursor);
        } else {
            break;
        }
    }
    assert_eq!(examined, 5);
    assert_eq!(cursors.len(), 5);
    query.scan_limit = 0;
    assert!(db.query_memory_records("scope", &query).is_err());
}
