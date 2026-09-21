use super::*;
use crate::storage::platform::semantic_tests::{actor, create, input, open};
use tempfile::TempDir;

fn derive(db: &RocksDbMemoryStorage, key: &str, sources: &[CommandReceipt]) -> CommandReceipt {
    db.apply_memory_command(
        "scope",
        &actor(),
        &MemoryCommand {
            contract_version: 1,
            idempotency_key: key.into(),
            operation: MemoryOperation::Derive {
                record: input(key),
                derivation: MemoryDerivation {
                    sources: sources
                        .iter()
                        .map(|s| MemorySourceRef {
                            record_id: s.record_id,
                            revision: s.revision,
                        })
                        .collect(),
                    method: "fixture".into(),
                    method_revision: "v1".into(),
                    evidence_ref: "trace://batch".into(),
                },
            },
        },
    )
    .unwrap()
}
fn update(db: &RocksDbMemoryStorage, receipt: &CommandReceipt) {
    db.apply_memory_command(
        "scope",
        &actor(),
        &MemoryCommand {
            contract_version: 1,
            idempotency_key: format!("update-{}", receipt.record_id),
            operation: MemoryOperation::Update {
                record_id: receipt.record_id,
                expected_revision: receipt.revision,
                record: input("corrected"),
            },
        },
    )
    .unwrap();
}
fn tip(db: &RocksDbMemoryStorage) -> VerifiedMemoryCursor {
    db.verified_memory_changes(
        "scope",
        &VerifiedMemoryChangesQuery {
            after: None,
            through: None,
            limit: 256,
        },
    )
    .unwrap()
    .high_watermark
    .unwrap()
}

#[test]
fn batch_read_keeps_order_and_matches_live_reads_after_rejection_deletion_and_reopen() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let origin = create(&db, "scope", "origin");
    let first = derive(&db, "first", std::slice::from_ref(&origin));
    let second = derive(&db, "second", std::slice::from_ref(&first));
    let live = create(&db, "scope", "live");
    let deleted = create(&db, "scope", "deleted");
    db.apply_memory_command(
        "scope",
        &actor(),
        &MemoryCommand {
            contract_version: 1,
            idempotency_key: "delete".into(),
            operation: MemoryOperation::Delete {
                record_id: deleted.record_id,
                expected_revision: 1,
            },
        },
    )
    .unwrap();
    let ids = [
        second.record_id,
        Uuid::new_v4(),
        live.record_id,
        deleted.record_id,
        first.record_id,
        origin.record_id,
    ];
    let before = db.read_memory_records("scope", &ids).unwrap();
    assert_eq!(
        before
            .entries
            .iter()
            .map(|e| e.record_id)
            .collect::<Vec<_>>(),
        ids
    );
    assert_eq!(
        before
            .entries
            .iter()
            .map(|e| e.record.is_some())
            .collect::<Vec<_>>(),
        [true, false, true, false, true, true]
    );
    // Shared source records are fetched once per request, across all roots.
    assert_eq!(before.dependency_work.records_examined, 2);
    db.review_memory_record(
        "scope",
        &actor(),
        &MemoryReviewCommand {
            contract_version: 1,
            idempotency_key: "reject".into(),
            record_id: origin.record_id,
            expected_revision: 1,
            disposition: MemoryReviewDisposition::Rejected,
            evidence_ref: "trace://rejection".into(),
        },
    )
    .unwrap();
    let fence = tip(&db);
    drop(db);
    let db = open(dir.path());
    let after = db.read_memory_records("scope", &ids).unwrap();
    assert_eq!(
        after
            .entries
            .iter()
            .map(|e| e.record.is_some())
            .collect::<Vec<_>>(),
        [false, false, true, false, false, false]
    );
    for entry in after.entries {
        assert_eq!(
            serde_json::to_value(&entry.record).unwrap(),
            serde_json::to_value(db.read_memory_record("scope", entry.record_id).unwrap()).unwrap()
        );
    }
    assert_eq!(tip(&db), fence, "Reads must not append events");
    let outside = db.read_memory_records("other", &ids).unwrap();
    assert!(outside.entries.iter().all(|entry| entry.record.is_none()));
    assert_eq!(outside.record_bytes, 0);
    assert_eq!(outside.dependency_work, DependencyWork::default());
}

#[test]
fn batch_read_uses_one_snapshot_for_roots_and_transitive_dependencies() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let source = create(&db, "scope", "source");
    let first = derive(&db, "first", std::slice::from_ref(&source));
    let second = derive(&db, "second", std::slice::from_ref(&first));
    let ids = [second.record_id, source.record_id, first.record_id];
    let snapshot = db.memory_snapshot();
    update(&db, &source);
    let old = snapshot.read_records("scope", &ids).unwrap();
    assert!(
        old.entries
            .iter()
            .all(|entry| entry.record.as_ref().unwrap().revision == 1)
    );
    assert_eq!(old.evaluated_at_millis, snapshot.now);
    let current = db.read_memory_records("scope", &ids).unwrap();
    assert!(current.entries[0].record.is_none());
    assert_eq!(current.entries[1].record.as_ref().unwrap().revision, 2);
    assert!(current.entries[2].record.is_none());
}

#[test]
fn batch_read_clock_expiration_needs_no_event_and_applies_to_transitive_sources() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let deadline = chrono::Utc::now().timestamp_millis() + 60_000;
    let mut record = input("expires");
    record.valid_until_millis = Some(deadline);
    let source = db
        .apply_memory_command(
            "scope",
            &actor(),
            &MemoryCommand {
                contract_version: 1,
                idempotency_key: "expiring".into(),
                operation: MemoryOperation::Create { record },
            },
        )
        .unwrap();
    let derived = derive(&db, "derived", std::slice::from_ref(&source));
    let fence = tip(&db);
    let ids = [derived.record_id, source.record_id];
    let mut before = db.memory_snapshot();
    before.now = deadline - 1;
    assert!(
        before
            .read_records("scope", &ids)
            .unwrap()
            .entries
            .iter()
            .all(|e| e.record.is_some())
    );
    let mut expired = db.memory_snapshot();
    expired.now = deadline;
    let batch = expired.read_records("scope", &ids).unwrap();
    assert_eq!(batch.evaluated_at_millis, deadline);
    assert!(batch.entries.iter().all(|e| e.record.is_none()));
    assert_eq!(tip(&db), fence);
}

#[test]
fn batch_read_rejects_invalid_counts_and_any_encountered_corruption() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let source = create(&db, "scope", "source");
    let derived = derive(&db, "derived", std::slice::from_ref(&source));
    let live = create(&db, "scope", "live");
    for ids in [
        vec![],
        vec![source.record_id; 2],
        (0..101).map(|_| Uuid::new_v4()).collect(),
    ] {
        assert!(matches!(
            db.read_memory_records("scope", &ids),
            Err(Error::ValidationError(_))
        ));
    }
    let ids: Vec<_> = (0..100).map(|_| Uuid::new_v4()).collect();
    assert_eq!(
        db.read_memory_records("scope", &ids).unwrap().entries.len(),
        100
    );
    db.db
        .delete_cf(
            db.cf(crate::storage::cf::EPISODE_INDEX).unwrap(),
            record_key(0x11, "scope", source.record_id),
        )
        .unwrap();
    for id in [source.record_id, derived.record_id] {
        assert!(
            matches!(
                db.read_memory_records("scope", &[live.record_id, id]),
                Err(Error::DataCorruption(_))
            ),
            "No partial result or unavailable fallback on corrupt data"
        );
    }
}

#[test]
fn batch_read_bounds_root_bytes_and_aggregate_dependency_work() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let mut sources = vec![];
    for index in 0..9 {
        let mut record = input("large");
        record.metadata.insert(
            "payload".into(),
            serde_json::Value::String("x".repeat(2 * 1024 * 1024)),
        );
        sources.push(
            db.apply_memory_command(
                "scope",
                &actor(),
                &MemoryCommand {
                    contract_version: 1,
                    idempotency_key: format!("large-{index}"),
                    operation: MemoryOperation::Create { record },
                },
            )
            .unwrap(),
        );
    }
    let ids: Vec<_> = sources[..4].iter().map(|s| s.record_id).collect();
    assert!(matches!(
        db.read_memory_records("scope", &ids),
        Err(Error::ValidationError(_))
    ));
    assert!(db.read_memory_records("scope", &ids[..3]).is_ok());
    let roots: Vec<_> = sources
        .chunks(3)
        .enumerate()
        .map(|(n, group)| derive(&db, &format!("group-{n}"), group).record_id)
        .collect();
    assert!(db.read_memory_records("scope", &roots[..2]).is_ok());
    assert!(
        matches!(
            db.read_memory_records("scope", &roots),
            Err(Error::ValidationError(_))
        ),
        "Dependency work must not reset per root"
    );
}
