use super::*;
use crate::storage::platform::semantic_tests::{actor, create, open};
use tempfile::TempDir;

fn selection(kind: CompanyConsumerKind, private: Option<&str>, owner: &str) -> CompanyConsumerRef {
    CompanyConsumerRef {
        kind,
        scope: MemoryResourceScope {
            project_id: "project".into(),
            mission_id: Some("mission".into()),
            agent_id: "agent".into(),
            visibility: if private.is_some() {
                MemoryVisibility::Private
            } else {
                MemoryVisibility::Shared
            },
        },
        private_subject_id: private.map(str::to_owned),
        subject_id: owner.into(),
        consumer_id: "cache".into(),
    }
}
fn query(kind: CompanyConsumerKind) -> CompanyConsumerQuery {
    CompanyConsumerQuery {
        kind,
        text: None,
        limit: 25,
        max_scanned_records: 100,
        cursor: None,
    }
}
fn seed(db: &RocksDbMemoryStorage, company: &str, selected: &CompanyConsumerRef) {
    let ns = selected.namespace(company).unwrap();
    let mut author = actor();
    author.subject_id = selected.subject_id.clone();
    match selected.kind {
        CompanyConsumerKind::MemoryV1 => {
            create(db, &ns, "first");
            let cursor = db
                .memory_changes(
                    &ns,
                    &MemoryChangesQuery {
                        after: None,
                        through: None,
                        limit: 1,
                    },
                )
                .unwrap()
                .high_watermark
                .unwrap();
            db.commit_memory_checkpoint(
                &ns,
                &author,
                &MemoryCheckpointCommand {
                    contract_version: 1,
                    idempotency_key: format!(
                        "initial-{}-{}",
                        author.subject_id, selected.consumer_id
                    ),
                    consumer_id: selected.consumer_id.clone(),
                    expected_revision: 0,
                    cursor,
                },
            )
            .unwrap();
        }
        CompanyConsumerKind::MemoryV2 => {
            let cursor = db.activate_verified_memory_journal(&ns).unwrap();
            db.commit_verified_memory_checkpoint(
                &ns,
                &author,
                &VerifiedMemoryCheckpointCommand {
                    contract_version: 2,
                    idempotency_key: format!(
                        "initial-{}-{}",
                        author.subject_id, selected.consumer_id
                    ),
                    consumer_id: selected.consumer_id.clone(),
                    expected_revision: 0,
                    expected_checkpoint_digest: None,
                    cursor,
                },
            )
            .unwrap();
        }
        CompanyConsumerKind::RelationsV1 => {
            let cursor = db.activate_relation_changes(&ns).unwrap();
            db.commit_relation_checkpoint(
                &ns,
                &author,
                &RelationCheckpointCommand {
                    contract_version: 1,
                    idempotency_key: format!(
                        "initial-{}-{}",
                        author.subject_id, selected.consumer_id
                    ),
                    consumer_id: selected.consumer_id.clone(),
                    expected_revision: 0,
                    expected_checkpoint_digest: None,
                    operation: RelationCheckpointOperation::Advance { cursor },
                },
            )
            .unwrap();
        }
    }
}

#[test]
fn company_consumer_directory_preserves_all_feed_kinds_and_restart() {
    let dir = TempDir::new().unwrap();
    let kinds = [
        CompanyConsumerKind::MemoryV1,
        CompanyConsumerKind::MemoryV2,
        CompanyConsumerKind::RelationsV1,
    ];
    {
        let db = open(dir.path());
        for kind in kinds {
            seed(&db, "company", &selection(kind, None, "owner"));
        }
    }
    let db = open(dir.path());
    for kind in kinds {
        let selected = selection(kind, None, "owner");
        let page = db.company_consumers("company", &query(kind)).unwrap();
        assert_eq!(page.entries.len(), 1);
        assert_eq!(page.entries[0].consumer, selected);
        assert_eq!(page.entries[0].revision, 1);
        assert_eq!(page.scanned_records, 1);
        assert_eq!(page.stop_reason, CompanyConsumerStop::Exhausted);
        assert!(page.next_cursor.is_none());
        assert!(
            db.inspect_company_consumer("company", &selected, None)
                .unwrap()
                .is_some()
        );
        assert!(
            db.inspect_company_consumer("other", &selected, None)
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn company_consumer_directory_bounds_company_before_scanning_and_keeps_owners_distinct() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let kind = CompanyConsumerKind::MemoryV2;
    for company in ["a", "aa", "a\"\\α", "a\"\\α-other"] {
        for (private, owner) in [
            (None, "alice"),
            (None, "bob"),
            (Some("alice"), "alice"),
            (Some("bob"), "bob"),
        ] {
            seed(&db, company, &selection(kind, private, owner));
        }
    }
    for company in ["a", "aa", "a\"\\α", "a\"\\α-other"] {
        let page = db.company_consumers(company, &query(kind)).unwrap();
        assert_eq!(page.entries.len(), 4);
        assert_eq!(page.scanned_records, 4);
        assert_eq!(
            page.entries
                .iter()
                .filter(|e| e.consumer.private_subject_id.is_none())
                .count(),
            2
        );
    }
    assert_eq!(
        db.company_consumers("absent", &query(kind))
            .unwrap()
            .scanned_records,
        0
    );
}

#[test]
fn company_consumer_directory_advances_empty_filtered_pages_and_binds_cursor() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let kind = CompanyConsumerKind::MemoryV2;
    for owner in ["alice", "bob", "carol"] {
        seed(&db, "company", &selection(kind, None, owner));
    }
    let mut q = query(kind);
    q.text = Some("CAROL".into());
    q.max_scanned_records = 1;
    let mut seen = vec![];
    let mut pages = 0;
    loop {
        let page = db.company_consumers("company", &q).unwrap();
        pages += 1;
        seen.extend(page.entries);
        if let Some(cursor) = page.next_cursor {
            let mut changed = q.clone();
            changed.cursor = Some(cursor.clone());
            changed.text = Some("alice".into());
            assert!(db.company_consumers("company", &changed).is_err());
            changed = q.clone();
            changed.cursor = Some(cursor.clone());
            assert!(db.company_consumers("other", &changed).is_err());
            changed.kind = CompanyConsumerKind::RelationsV1;
            assert!(db.company_consumers("company", &changed).is_err());
            q.cursor = Some(cursor);
        } else {
            break;
        }
        assert!(pages < 4);
    }
    assert_eq!(pages, 3);
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].consumer.subject_id, "carol");
}

#[test]
fn company_consumer_directory_does_not_suppress_corrupt_checkpoints_with_filter() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let kind = CompanyConsumerKind::MemoryV2;
    let selected = selection(kind, None, "owner");
    seed(&db, "company", &selected);
    db.db
        .put_cf(
            db.cf(crate::storage::cf::AGENT_META).unwrap(),
            key(
                kind,
                &selected.namespace("company").unwrap(),
                "owner",
                "cache",
            )
            .unwrap(),
            b"{}",
        )
        .unwrap();
    let mut q = query(kind);
    q.text = Some("unmatched".into());
    assert!(db.company_consumers("company", &q).is_err());
    assert!(
        db.inspect_company_consumer("company", &selected, None)
            .is_err()
    );
}

#[test]
fn company_consumer_directory_legacy_progress_is_only_a_range_estimate() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let selected = selection(CompanyConsumerKind::MemoryV1, None, "owner");
    seed(&db, "company", &selected);
    create(&db, &selected.namespace("company").unwrap(), "second");
    let Some(CompanyConsumerDetails::MemoryV1(details)) = db
        .inspect_company_consumer("company", &selected, None)
        .unwrap()
    else {
        panic!("legacy observation required")
    };
    assert!(details.position_in_range);
    assert_eq!(details.sequence_distance_estimate, Some(1));
    let witness = CompanyConsumerWitness::MemoryV2(
        db.activate_verified_memory_journal(&selected.namespace("company").unwrap())
            .unwrap(),
    );
    assert!(
        db.inspect_company_consumer("company", &selected, Some(&witness))
            .is_err()
    );
    let relation = selection(CompanyConsumerKind::RelationsV1, None, "owner");
    seed(&db, "company", &relation);
    assert!(
        db.inspect_company_consumer("company", &relation, Some(&witness))
            .is_err()
    );
}

#[test]
fn company_consumer_directory_retains_incompatible_progress_for_diagnosis() {
    let dir = TempDir::new().unwrap();
    let db = open(&dir.path().join("live"));
    let selected = selection(CompanyConsumerKind::MemoryV2, None, "owner");
    seed(&db, "company", &selected);
    let ns = selected.namespace("company").unwrap();
    let backup = dir.path().join("backup");
    rocksdb::checkpoint::Checkpoint::new(&db.db)
        .unwrap()
        .create_checkpoint(&backup)
        .unwrap();
    create(&db, &ns, "lost");
    let Some(CompanyConsumerDetails::MemoryV2(before)) = db
        .inspect_company_consumer("company", &selected, None)
        .unwrap()
    else {
        panic!()
    };
    let previous = before.checkpoint.unwrap();
    let mut author = actor();
    author.subject_id = selected.subject_id.clone();
    let lost = db
        .commit_verified_memory_checkpoint(
            &ns,
            &author,
            &VerifiedMemoryCheckpointCommand {
                contract_version: 2,
                idempotency_key: "advance".into(),
                consumer_id: "cache".into(),
                expected_revision: previous.revision,
                expected_checkpoint_digest: Some(previous.checkpoint_digest),
                cursor: before.high_watermark.unwrap(),
            },
        )
        .unwrap()
        .checkpoint;
    let restored = open(&backup);
    create(&restored, &ns, "different-branch");
    restored
        .db
        .put_cf(
            restored.cf(crate::storage::cf::AGENT_META).unwrap(),
            key(selected.kind, &ns, "owner", "cache").unwrap(),
            encode(&lost).unwrap(),
        )
        .unwrap();
    assert!(
        restored
            .read_verified_memory_checkpoint(&ns, "owner", "cache")
            .is_err()
    );
    let page = restored
        .company_consumers("company", &query(selected.kind))
        .unwrap();
    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.entries[0].revision, lost.revision);
    let witness = CompanyConsumerWitness::MemoryV2(lost.cursor.clone());
    let Some(CompanyConsumerDetails::MemoryV2(details)) = restored
        .inspect_company_consumer("company", &selected, Some(&witness))
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(
        details.checkpoint_status,
        ConsumerCheckpointStatus::HistoryIncompatible
    );
    assert_eq!(
        details.witness_status,
        ConsumerWitnessStatus::HistoryIncompatible
    );
    assert_eq!(details.pending_positions, None);
    assert_eq!(details.checkpoint, Some(lost));
}

#[test]
fn company_consumer_directory_rejects_invalid_limits_and_untrusted_continuations() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let kind = CompanyConsumerKind::MemoryV2;
    seed(&db, "company", &selection(kind, None, "alice"));
    seed(&db, "company", &selection(kind, None, "bob"));
    for (limit, scan) in [(0, 1), (51, 1), (1, 0), (1, 1001)] {
        let mut q = query(kind);
        q.limit = limit;
        q.max_scanned_records = scan;
        assert!(db.company_consumers("company", &q).is_err());
    }
    let mut q = query(kind);
    q.limit = 1;
    let first = db.company_consumers("company", &q).unwrap();
    assert_eq!(first.stop_reason, CompanyConsumerStop::EntryLimit);
    let cursor = first.next_cursor.unwrap();
    q.cursor = Some(cursor.clone());
    let second = db.company_consumers("company", &q).unwrap();
    assert_eq!(second.stop_reason, CompanyConsumerStop::Exhausted);
    assert_ne!(first.entries[0].consumer, second.entries[0].consumer);
    for position in ["ff".into(), "NOTHEX".into(), "0".into(), "00".repeat(16385)] {
        let mut bad = cursor.clone();
        bad.position = position;
        q.cursor = Some(bad);
        assert!(db.company_consumers("company", &q).is_err());
    }
}

#[test]
fn company_consumer_directory_byte_budget_preserves_progress_without_matches() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let kind = CompanyConsumerKind::MemoryV2;
    for n in 0..950 {
        let mut selected = selection(kind, Some(&"\\".repeat(256)), "owner");
        selected.scope.project_id = "\\".repeat(256);
        selected.scope.agent_id = "\\".repeat(256);
        selected.scope.mission_id = Some("\\".repeat(256));
        selected.consumer_id = format!("consumer-{n:04}");
        seed(&db, "company", &selected);
    }
    let mut q = query(kind);
    q.text = Some("no-match".into());
    q.max_scanned_records = 1000;
    let first = db.company_consumers("company", &q).unwrap();
    assert!(first.entries.is_empty());
    assert_eq!(first.stop_reason, CompanyConsumerStop::ByteLimit);
    assert!(first.scanned_records > 0 && first.scanned_records < 950);
    assert!(first.scanned_record_bytes <= MAX_BYTES);
    q.cursor = first.next_cursor;
    let second = db.company_consumers("company", &q).unwrap();
    assert_eq!(second.stop_reason, CompanyConsumerStop::Exhausted);
    assert_eq!(first.scanned_records + second.scanned_records, 950);
    assert!(second.entries.is_empty());
}
