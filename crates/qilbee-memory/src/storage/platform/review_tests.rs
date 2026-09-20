use super::semantic_tests::{actor, attach, create, input, open, search, space};
use super::*;
use tempfile::TempDir;
fn decision(id: Uuid, rev: u64, disposition: MemoryReviewDisposition) -> MemoryReviewCommand {
    MemoryReviewCommand {
        contract_version: 1,
        idempotency_key: format!("review-{rev}"),
        record_id: id,
        expected_revision: rev,
        disposition,
        evidence_ref: "evidence://fixture/report".into(),
    }
}
fn lexical() -> LexicalQuery {
    LexicalQuery {
        text: "ZX17".into(),
        limit: 10,
        scan_limit: 100,
        scan_bytes_limit: 8_388_608,
        after: None,
        episode_type: None,
        tag: None,
    }
}
fn hybrid() -> HybridQuery {
    HybridQuery {
        text: "ZX17".into(),
        space: space(),
        vector: vec![1.0, 0.0, 0.0],
        limit: 10,
        ranking_version: HybridRankingVersion::WeightedRrfV1,
        min_score: -1.0,
        scan_limit: 100,
        scan_bytes_limit: 8_388_608,
        after: None,
        episode_type: None,
        tag: None,
    }
}
#[test]
fn review_rejection_precedes_all_retrieval_and_corpus_statistics() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let reviewer = actor();
    let target = create(&db, "scope", "ZX17");
    let baseline = create(&db, "baseline", "ZX17");
    let rejected = create(&db, "scope", "ZX17 ZX17 distracting words");
    for (n, r) in [target.clone(), rejected.clone()].iter().enumerate() {
        db.apply_memory_embedding(
            "scope",
            &reviewer,
            &attach(r.record_id, &format!("vec{n}"), vec![1.0, 0.0, 0.0]),
        )
        .unwrap();
    }
    let old = db.memory_snapshot();
    db.review_memory_record(
        "scope",
        &reviewer,
        &decision(rejected.record_id, 1, MemoryReviewDisposition::Rejected),
    )
    .unwrap();
    assert!(
        db.read_memory_record("scope", rejected.record_id)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        db.query_memory_records(
            "scope",
            &MemoryQuery {
                scan_limit: 10_000,
                limit: 10,
                after: None,
                text_contains: None,
                episode_type: None,
                tag: None
            }
        )
        .unwrap()
        .records
        .len(),
        1
    );
    let page = db.search_memory_lexical("scope", &lexical()).unwrap();
    let clean = db.search_memory_lexical("baseline", &lexical()).unwrap();
    assert_eq!(page.corpus_records, 1);
    assert_eq!(page.hits[0].score, clean.hits[0].score);
    assert_eq!(clean.hits[0].record.record_id, baseline.record_id);
    let sem = db.search_memory_semantic("scope", &search()).unwrap();
    assert_eq!(sem.hits.len(), 1);
    assert_eq!(sem.hits[0].record.record_id, target.record_id);
    let mixed = db.search_memory_hybrid("scope", &hybrid()).unwrap();
    assert_eq!(mixed.corpus_records, 1);
    assert_eq!(mixed.embedded_records, 1);
    assert_eq!(
        old.search_hybrid("scope", &hybrid())
            .unwrap()
            .corpus_records,
        2
    );
    let mut vector = attach(rejected.record_id, "rejected-vector", vec![1.0, 0.0, 0.0]);
    vector.record_revision = 2;
    assert!(
        db.apply_memory_embedding("scope", &reviewer, &vector)
            .is_err()
    );
    db.review_memory_record(
        "scope",
        &reviewer,
        &decision(rejected.record_id, 2, MemoryReviewDisposition::Approved),
    )
    .unwrap();
    assert_eq!(
        db.search_memory_hybrid("scope", &hybrid())
            .unwrap()
            .corpus_records,
        2
    );
    assert_eq!(
        db.search_memory_hybrid("scope", &hybrid())
            .unwrap()
            .embedded_records,
        1
    );
    vector.idempotency_key = "approved-vector".into();
    vector.record_revision = 3;
    db.apply_memory_embedding("scope", &reviewer, &vector)
        .unwrap();
    assert_eq!(
        db.search_memory_semantic("scope", &search())
            .unwrap()
            .hits
            .len(),
        2
    );
}
#[test]
fn review_history_and_idempotency_survive_updates_deletion_and_restart() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let created = create(&db, "scope", "original");
    let original = db
        .read_memory_record("scope", created.record_id)
        .unwrap()
        .unwrap();
    let reviewer = RecordAuthor {
        subject_id: "reviewer".into(),
        ..actor()
    };
    let command = decision(created.record_id, 1, MemoryReviewDisposition::Rejected);
    let receipt = db
        .review_memory_record("scope", &reviewer, &command)
        .unwrap();
    let raw = db
        .platform_record_locked("scope", created.record_id)
        .unwrap()
        .unwrap();
    assert_eq!(raw.author, original.author);
    let mut rotated = reviewer.clone();
    rotated.credential_id = Uuid::new_v4();
    assert_eq!(
        db.review_memory_record("scope", &rotated, &command)
            .unwrap(),
        receipt
    );
    let mut changed = command.clone();
    changed.disposition = MemoryReviewDisposition::Approved;
    assert!(matches!(
        db.review_memory_record("scope", &reviewer, &changed),
        Err(Error::ConstraintViolation(_))
    ));
    db.apply_memory_command(
        "scope",
        &actor(),
        &MemoryCommand {
            contract_version: 1,
            idempotency_key: "correct".into(),
            operation: MemoryOperation::Update {
                record_id: created.record_id,
                expected_revision: 2,
                record: input("corrected"),
            },
        },
    )
    .unwrap();
    let corrected = db
        .read_memory_record("scope", created.record_id)
        .unwrap()
        .unwrap();
    assert!(corrected.review.is_none());
    assert_eq!(corrected.revision, 3);
    db.apply_memory_command(
        "scope",
        &actor(),
        &MemoryCommand {
            contract_version: 1,
            idempotency_key: "delete".into(),
            operation: MemoryOperation::Delete {
                record_id: created.record_id,
                expected_revision: 3,
            },
        },
    )
    .unwrap();
    assert!(
        db.review_memory_record(
            "scope",
            &reviewer,
            &decision(created.record_id, 4, MemoryReviewDisposition::Approved)
        )
        .is_err()
    );
    drop(db);
    let db = open(dir.path());
    assert_eq!(
        db.read_memory_review("scope", created.record_id, 2)
            .unwrap(),
        Some(receipt.clone())
    );
    assert_eq!(
        db.review_memory_record("scope", &rotated, &command)
            .unwrap(),
        receipt
    );
    assert!(
        db.read_memory_review("other", created.record_id, 2)
            .unwrap()
            .is_none()
    );
    assert!(
        db.memory_review_state("scope", created.record_id)
            .unwrap()
            .unwrap()
            .deleted
    );
    let events = db
        .memory_changes(
            "scope",
            &MemoryChangesQuery {
                after: None,
                through: None,
                limit: 256,
            },
        )
        .unwrap();
    assert_eq!(events.changes.len(), 4);
    assert_eq!(events.changes[1].kind, MemoryChangeKind::Reviewed);
    assert_eq!(events.changes[1].author, reviewer);
    let mut expired = input("expired");
    expired.valid_until_millis = Some(1);
    let r = db
        .apply_memory_command(
            "scope",
            &actor(),
            &MemoryCommand {
                contract_version: 1,
                idempotency_key: "expired".into(),
                operation: MemoryOperation::Create { record: expired },
            },
        )
        .unwrap();
    db.review_memory_record(
        "scope",
        &reviewer,
        &MemoryReviewCommand {
            idempotency_key: "expired-review".into(),
            ..decision(r.record_id, 1, MemoryReviewDisposition::Approved)
        },
    )
    .unwrap();
    assert!(
        db.read_memory_record("scope", r.record_id)
            .unwrap()
            .is_none()
    );
    assert!(
        db.memory_review_state("scope", r.record_id)
            .unwrap()
            .unwrap()
            .expired
    );
}
#[test]
fn concurrent_review_decisions_have_one_revision_winner_and_one_event() {
    use std::sync::{Arc, Barrier};
    let dir = TempDir::new().unwrap();
    let db = Arc::new(open(dir.path()));
    let r = create(&db, "scope", "race");
    let gate = Arc::new(Barrier::new(8));
    let joins: Vec<_> = (0..8)
        .map(|i| {
            let db = db.clone();
            let gate = gate.clone();
            std::thread::spawn(move || {
                gate.wait();
                db.review_memory_record(
                    "scope",
                    &actor(),
                    &MemoryReviewCommand {
                        idempotency_key: format!("r{i}"),
                        ..decision(r.record_id, 1, MemoryReviewDisposition::Approved)
                    },
                )
            })
        })
        .collect();
    let results: Vec<_> = joins.into_iter().map(|j| j.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert!(
        results
            .iter()
            .filter(|r| r.is_err())
            .all(|r| matches!(r, Err(Error::TransactionConflict(_))))
    );
    assert_eq!(
        db.memory_review_state("scope", r.record_id)
            .unwrap()
            .unwrap()
            .revision,
        2
    );
    assert_eq!(
        db.memory_changes(
            "scope",
            &MemoryChangesQuery {
                after: None,
                through: None,
                limit: 256
            }
        )
        .unwrap()
        .changes
        .len(),
        2
    );
}
#[test]
fn review_integrity_and_invalid_input_fail_without_mutation() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let r = create(&db, "scope", "record");
    let who = actor();
    let mut c = decision(r.record_id, 1, MemoryReviewDisposition::Approved);
    c.evidence_ref = "\n".into();
    assert!(db.review_memory_record("scope", &who, &c).is_err());
    assert_eq!(
        db.memory_review_state("scope", r.record_id)
            .unwrap()
            .unwrap()
            .revision,
        1
    );
    c.evidence_ref = "evidence".into();
    let receipt = db.review_memory_record("scope", &who, &c).unwrap();
    let cf = db.cf(super::super::cf::AGENT_META).unwrap();
    let mut key = record_key(0x23, "scope", r.record_id);
    key.extend_from_slice(&2u64.to_be_bytes());
    let mut stored: serde_json::Value =
        serde_json::from_slice(&db.db.get_cf(cf, &key).unwrap().unwrap()).unwrap();
    stored["receipt"]["review"]["evidence_ref"] = "tampered".into();
    db.db
        .put_cf(cf, key, serde_json::to_vec(&stored).unwrap())
        .unwrap();
    assert!(matches!(
        db.read_memory_review("scope", r.record_id, 2),
        Err(Error::DataCorruption(_))
    ));
    assert_eq!(db.review_memory_record("scope", &who, &c).unwrap(), receipt);
}
