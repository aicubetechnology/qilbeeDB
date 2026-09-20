use super::semantic_tests::{actor, attach, create, input, open, search, space};
use super::*;
use tempfile::TempDir;
fn command(key: &str, sources: Vec<MemorySourceRef>) -> MemoryCommand {
    MemoryCommand {
        contract_version: 1,
        idempotency_key: key.into(),
        operation: MemoryOperation::Derive {
            record: input("ZX17 derived conclusion"),
            derivation: MemoryDerivation {
                sources,
                method: "agent-synthesis".into(),
                method_revision: "prompt-v1".into(),
                evidence_ref: "trace://run/1".into(),
            },
        },
    }
}
fn source(r: &CommandReceipt) -> MemorySourceRef {
    MemorySourceRef {
        record_id: r.record_id,
        revision: r.revision,
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
fn derived_revisions_are_checked_before_every_serving_path_and_statistics() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let s = create(&db, "scope", "ZX17 source");
    let cmd = command("derive", vec![source(&s)]);
    let d = db.apply_memory_command("scope", &actor(), &cmd).unwrap();
    db.apply_memory_embedding(
        "scope",
        &actor(),
        &attach(d.record_id, "vector", vec![1.0, 0.0, 0.0]),
    )
    .unwrap();
    assert_eq!(
        db.search_memory_hybrid("scope", &hybrid())
            .unwrap()
            .corpus_records,
        2
    );
    let old = db.memory_snapshot();
    db.apply_memory_command(
        "scope",
        &actor(),
        &MemoryCommand {
            contract_version: 1,
            idempotency_key: "correct".into(),
            operation: MemoryOperation::Update {
                record_id: s.record_id,
                expected_revision: 1,
                record: input("ZX17 corrected"),
            },
        },
    )
    .unwrap();
    assert!(
        db.read_memory_record("scope", d.record_id)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        db.query_memory_records(
            "scope",
            &MemoryQuery {
                scan_limit: 10_000,
                limit: 100,
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
    assert!(
        db.search_memory_semantic("scope", &search())
            .unwrap()
            .hits
            .is_empty()
    );
    let l = db.search_memory_lexical("scope", &lexical()).unwrap();
    assert_eq!(l.corpus_records, 1);
    assert_eq!(l.dependency_work.records_examined, 1);
    assert!(l.dependency_work.bytes_examined > 0);
    assert_eq!(
        db.search_memory_hybrid("scope", &hybrid())
            .unwrap()
            .corpus_records,
        1
    );
    assert_eq!(
        old.search_hybrid("scope", &hybrid())
            .unwrap()
            .corpus_records,
        2
    );
    let mut fresh = attach(d.record_id, "new-vector", vec![1.0, 0.0, 0.0]);
    fresh.space.revision = "new-space".into();
    assert!(
        db.apply_memory_embedding("scope", &actor(), &fresh)
            .is_err()
    );
    assert!(
        db.apply_memory_command("scope", &actor(), &command("stale", vec![source(&s)]))
            .is_err()
    );
    db.review_memory_record(
        "scope",
        &actor(),
        &MemoryReviewCommand {
            contract_version: 1,
            idempotency_key: "approve-derived".into(),
            record_id: d.record_id,
            expected_revision: 1,
            disposition: MemoryReviewDisposition::Approved,
            evidence_ref: "test://approval".into(),
        },
    )
    .unwrap();
    assert!(
        db.read_memory_record("scope", d.record_id)
            .unwrap()
            .is_none()
    );
    let baseline = create(&db, "baseline", "ZX17 corrected");
    assert_ne!(baseline.record_id, s.record_id);
    assert_eq!(
        db.search_memory_lexical("baseline", &lexical())
            .unwrap()
            .hits[0]
            .score,
        l.hits[0].score
    );
}
#[test]
fn derivation_admission_is_scoped_immutable_idempotent_and_durable() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let who = actor();
    let s = create(&db, "scope", "source");
    let cmd = command("derive", vec![source(&s)]);
    let d = db.apply_memory_command("scope", &who, &cmd).unwrap();
    assert_eq!(d.action, "derived");
    assert!(db.apply_memory_command("foreign", &who, &cmd).is_err());
    assert!(
        db.apply_memory_command(
            "scope",
            &who,
            &command("duplicate", vec![source(&s), source(&s)])
        )
        .is_err()
    );
    assert!(
        db.apply_memory_command("scope", &who, &command("empty", vec![]))
            .is_err()
    );
    let update = MemoryCommand {
        contract_version: 1,
        idempotency_key: "wash-provenance".into(),
        operation: MemoryOperation::Update {
            record_id: d.record_id,
            expected_revision: 1,
            record: input("replacement"),
        },
    };
    assert!(matches!(
        db.apply_memory_command("scope", &who, &update),
        Err(Error::ConstraintViolation(_))
    ));
    let record = db
        .read_memory_record("scope", d.record_id)
        .unwrap()
        .unwrap();
    assert_eq!(record.derivation.unwrap().sources, vec![source(&s)]);
    db.apply_memory_command(
        "scope",
        &who,
        &MemoryCommand {
            contract_version: 1,
            idempotency_key: "delete-source".into(),
            operation: MemoryOperation::Delete {
                record_id: s.record_id,
                expected_revision: 1,
            },
        },
    )
    .unwrap();
    drop(db);
    let db = open(dir.path());
    assert_eq!(db.apply_memory_command("scope", &who, &cmd).unwrap(), d);
    assert!(
        db.read_memory_record("scope", d.record_id)
            .unwrap()
            .is_none()
    );
    let events = db
        .memory_changes(
            "scope",
            &MemoryChangesQuery {
                limit: 256,
                after: None,
                through: None,
            },
        )
        .unwrap();
    assert_eq!(events.changes.len(), 3);
    assert_eq!(events.changes[1].kind, MemoryChangeKind::Derived);
}
#[test]
fn derivation_dependency_reads_are_cached_bounded_and_integrity_checked() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let s = create(&db, "scope", "source");
    for n in 0..20 {
        db.apply_memory_command(
            "scope",
            &actor(),
            &command(&format!("derived{n}"), vec![source(&s)]),
        )
        .unwrap();
    }
    let page = db.search_memory_lexical("scope", &lexical()).unwrap();
    assert_eq!(page.corpus_records, 21);
    assert_eq!(page.dependency_work.records_examined, 1);
    let mut r = input("oversized dependency");
    r.content.primary = "x".repeat(MAX_DEPENDENCY_BYTES + 1);
    let huge = db
        .apply_memory_command(
            "scope",
            &actor(),
            &MemoryCommand {
                contract_version: 1,
                idempotency_key: "huge".into(),
                operation: MemoryOperation::Create { record: r },
            },
        )
        .unwrap();
    assert!(matches!(
        db.apply_memory_command(
            "scope",
            &actor(),
            &command("too-large", vec![source(&huge)])
        ),
        Err(Error::ValidationError(_))
    ));
    let snapshot = db.memory_snapshot();
    for _ in 0..MAX_DEPENDENCY_RECORDS {
        assert!(
            snapshot
                .dependency("scope", Uuid::new_v4())
                .unwrap()
                .is_none()
        );
    }
    assert!(matches!(
        snapshot.dependency("scope", Uuid::new_v4()),
        Err(Error::ValidationError(_))
    ));
    let key = record_key(0x10, "scope", s.record_id);
    db.db
        .put_cf(db.cf(super::super::cf::EPISODES).unwrap(), key, b"tampered")
        .unwrap();
    // The oversized fixture may precede the corrupt record in UUID order.
    // Keep this integrity assertion independent of the separate scan-budget test.
    let mut integrity_query = lexical();
    integrity_query.scan_bytes_limit = 64 * 1024 * 1024;
    assert!(matches!(
        db.search_memory_lexical("scope", &integrity_query),
        Err(Error::DataCorruption(_))
    ));
}
#[test]
fn expired_and_rejected_sources_cannot_support_derived_memory() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let s = create(&db, "scope", "source");
    let d = db
        .apply_memory_command("scope", &actor(), &command("derived", vec![source(&s)]))
        .unwrap();
    let before = db.memory_snapshot();
    db.review_memory_record(
        "scope",
        &actor(),
        &MemoryReviewCommand {
            contract_version: 1,
            idempotency_key: "reject".into(),
            record_id: s.record_id,
            expected_revision: 1,
            disposition: MemoryReviewDisposition::Rejected,
            evidence_ref: "test://rejection".into(),
        },
    )
    .unwrap();
    assert!(
        db.read_memory_record("scope", d.record_id)
            .unwrap()
            .is_none()
    );
    assert!(
        before
            .eligible(
                "scope",
                &before.record("scope", d.record_id).unwrap().unwrap()
            )
            .unwrap()
    );
    assert!(
        db.apply_memory_command(
            "scope",
            &actor(),
            &command(
                "rejected-source",
                vec![MemorySourceRef {
                    revision: 2,
                    ..source(&s)
                }]
            )
        )
        .is_err()
    );
    let mut r = input("expiring");
    r.valid_until_millis = Some(chrono::Utc::now().timestamp_millis() + 60_000);
    let exp = db
        .apply_memory_command(
            "scope",
            &actor(),
            &MemoryCommand {
                contract_version: 1,
                idempotency_key: "expires".into(),
                operation: MemoryOperation::Create { record: r.clone() },
            },
        )
        .unwrap();
    let derived = db
        .apply_memory_command(
            "scope",
            &actor(),
            &command("expiring-derived", vec![source(&exp)]),
        )
        .unwrap();
    let mut future = db.memory_snapshot();
    future.now = r.valid_until_millis.unwrap();
    assert!(
        !future
            .eligible(
                "scope",
                &future.record("scope", derived.record_id).unwrap().unwrap()
            )
            .unwrap()
    );
}
