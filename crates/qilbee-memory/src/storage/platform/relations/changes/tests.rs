use super::super::super::semantic_tests::{actor, create, open};
use super::*;
use tempfile::TempDir;
#[path = "tests_checkpoints.rs"]
mod checkpoints;

fn query() -> RelationChangesQuery {
    RelationChangesQuery {
        after: None,
        through: None,
        limit: 256,
    }
}
fn tip(db: &RocksDbMemoryStorage) -> RelationChangeCursor {
    db.relation_changes("scope", &query())
        .unwrap()
        .high_watermark
        .unwrap()
}
fn assertion(db: &RocksDbMemoryStorage) -> MemoryRelationCommand {
    let a = create(db, "scope", "a");
    let b = create(db, "scope", "b");
    serde_json::from_value(serde_json::json!({"contract_version":1,"idempotency_key":"assert", "operation":{"type":"assert","relation":{
        "source":{"record_id":a.record_id,"revision":1},"target":{"record_id":b.record_id,"revision":1},"kind":"supports",
        "provenance":{"origin":"tool_observation","method":"fixture","method_revision":"v1","evidence_ref":"trace://relation-feed"}
    }}})).unwrap()
}
fn mutate(
    db: &RocksDbMemoryStorage,
    id: Uuid,
    revision: u64,
    action: &str,
) -> Result<MemoryRelationReceipt> {
    let mut operation = serde_json::json!({"type":action,"relation_id":id,"expected_revision":revision,"evidence_ref":"trace://decision"});
    if action == "review" {
        operation["disposition"] = "rejected".into();
    }
    db.apply_memory_relation_command("scope", &actor(), &serde_json::from_value(serde_json::json!({"contract_version":1,"idempotency_key":format!("{action}-{revision}"),"operation":operation})).unwrap())
}
#[test]
fn relation_feed_is_atomic_bounded_idempotent_and_preserves_memory_feeds() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let command = assertion(&db);
    let before = db
        .verified_memory_changes(
            "scope",
            &VerifiedMemoryChangesQuery {
                after: None,
                through: None,
                limit: 256,
            },
        )
        .unwrap();
    assert!(!db.relation_changes("scope", &query()).unwrap().active);
    let baseline = db.activate_relation_changes("scope").unwrap();
    let first = db
        .apply_memory_relation_command("scope", &actor(), &command)
        .unwrap();
    mutate(&db, first.relation_id, 1, "retire").unwrap();
    let page = db
        .relation_changes(
            "scope",
            &RelationChangesQuery {
                limit: 1,
                ..query()
            },
        )
        .unwrap();
    assert!(!page.complete);
    assert_eq!(page.high_watermark.as_ref().unwrap().sequence, 2);
    assert_eq!(page.changes[0].change.receipt_digest, first.receipt_digest);
    assert_eq!(
        page.changes[0].change.relation_digest,
        first.relation_digest
    );
    mutate(&db, first.relation_id, 2, "restore").unwrap();
    let fenced = RelationChangesQuery {
        after: page.next_cursor,
        through: page.high_watermark,
        limit: 1,
    };
    let remaining = db.relation_changes("scope", &fenced).unwrap();
    assert!(remaining.complete);
    assert_eq!(remaining.changes.len(), 1);
    assert_eq!(remaining.changes[0].change.kind, RelationAction::Retired);
    mutate(&db, first.relation_id, 3, "review").unwrap();
    assert_eq!(tip(&db).sequence, 4);
    assert_eq!(db.activate_relation_changes("scope").unwrap(), baseline);
    assert_eq!(
        db.apply_memory_relation_command("scope", &actor(), &command)
            .unwrap(),
        first
    );
    assert!(mutate(&db, first.relation_id, 1, "restore").is_err());
    assert_eq!(tip(&db).sequence, 4);
    assert_eq!(
        db.verified_memory_changes(
            "scope",
            &VerifiedMemoryChangesQuery {
                after: None,
                through: None,
                limit: 256
            }
        )
        .unwrap(),
        before
    );
    let all = db.relation_changes("scope", &query()).unwrap();
    let encoded = serde_json::to_string(&all).unwrap();
    assert!(!encoded.contains("trace://"));
    assert!(!encoded.contains("primary"));
    assert_eq!(
        all.changes
            .iter()
            .map(|e| e.change.relation_revision)
            .collect::<Vec<_>>(),
        vec![1, 2, 3, 4]
    );
    drop(db);
    let db = open(dir.path());
    assert_eq!(db.relation_changes("scope", &query()).unwrap(), all);
    assert_eq!(db.relation_changes("scope", &fenced).unwrap(), remaining);
    assert!(matches!(
        db.relation_changes("foreign", &fenced),
        Err(Error::JournalHistoryConflict(_))
    ));
}
#[test]
fn relation_cursors_detect_divergent_restore_even_for_identical_commands() {
    let dir = TempDir::new().unwrap();
    let db = open(&dir.path().join("live"));
    let command = assertion(&db);
    let receipt = db
        .apply_memory_relation_command("scope", &actor(), &command)
        .unwrap();
    let prefix = tip(&db);
    let backup = dir.path().join("backup");
    rocksdb::checkpoint::Checkpoint::new(&db.db)
        .unwrap()
        .create_checkpoint(&backup)
        .unwrap();
    mutate(&db, receipt.relation_id, 1, "retire").unwrap();
    let lost = tip(&db);
    let restored = open(&backup);
    mutate(&restored, receipt.relation_id, 1, "retire").unwrap();
    assert_eq!(tip(&restored).sequence, lost.sequence);
    assert_ne!(tip(&restored).prefix_digest, lost.prefix_digest);
    mutate(&restored, receipt.relation_id, 2, "restore").unwrap();
    assert!(
        restored
            .relation_changes(
                "scope",
                &RelationChangesQuery {
                    after: Some(prefix),
                    ..query()
                }
            )
            .is_ok()
    );
    for through in [true, false] {
        let mut q = query();
        if through {
            q.through = Some(lost.clone());
        } else {
            q.after = Some(lost.clone());
        }
        assert!(matches!(
            restored.relation_changes("scope", &q),
            Err(Error::JournalHistoryConflict(_))
        ));
    }
}
#[test]
fn relation_feed_validates_bounds_stream_identity_and_inactive_state() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let inactive = db.relation_changes("scope", &query()).unwrap();
    assert!(inactive.complete);
    assert_eq!(inactive.high_watermark, None);
    for limit in [0, 257, usize::MAX] {
        assert!(matches!(
            db.relation_changes("scope", &RelationChangesQuery { limit, ..query() }),
            Err(Error::ValidationError(_))
        ));
    }
    let baseline = db.activate_relation_changes("scope").unwrap();
    for field in ["version", "prefix_digest", "stream"] {
        let mut cursor = serde_json::to_value(&baseline).unwrap();
        cursor[field] = if field == "version" {
            serde_json::json!(2)
        } else {
            serde_json::json!("memory")
        };
        let parsed = serde_json::from_value::<RelationChangeCursor>(cursor);
        if let Ok(cursor) = parsed {
            assert!(matches!(
                db.relation_changes(
                    "scope",
                    &RelationChangesQuery {
                        after: Some(cursor),
                        ..query()
                    }
                ),
                Err(Error::ValidationError(_))
            ));
        }
    }
    let mut ahead = baseline.clone();
    ahead.sequence = 1;
    assert!(matches!(
        db.relation_changes(
            "scope",
            &RelationChangesQuery {
                after: Some(ahead),
                ..query()
            }
        ),
        Err(Error::JournalHistoryConflict(_))
    ));
    assert!(matches!(
        db.relation_changes(
            "foreign",
            &RelationChangesQuery {
                after: Some(baseline),
                ..query()
            }
        ),
        Err(Error::JournalHistoryConflict(_))
    ));
    let command = assertion(&db);
    db.apply_memory_relation_command("scope", &actor(), &command)
        .unwrap();
    let page = db.relation_changes("scope", &query()).unwrap();
    assert!(matches!(
        db.relation_changes(
            "scope",
            &RelationChangesQuery {
                after: page.high_watermark,
                through: page.baseline,
                limit: 1
            }
        ),
        Err(Error::ValidationError(_))
    ));
}
#[test]
fn relation_feed_corruption_blocks_new_relation_commits_and_returns_no_partial_page() {
    for mode in [
        "missing_header",
        "missing_entry",
        "digest",
        "historical_pair",
        "metadata",
        "oversized",
    ] {
        let dir = TempDir::new().unwrap();
        let db = open(dir.path());
        let cmd = assertion(&db);
        let receipt = db
            .apply_memory_relation_command("scope", &actor(), &cmd)
            .unwrap();
        let cf = db.cf(crate::storage::cf::AGENT_META).unwrap();
        let event = event_key("scope", 1);
        match mode {
            "missing_header" => db
                .db
                .delete_cf(cf, record_prefix(JOURNAL, "scope"))
                .unwrap(),
            "missing_entry" => db.db.delete_cf(cf, &event).unwrap(),
            "historical_pair" => db
                .db
                .delete_cf(
                    cf,
                    super::super::history_key("scope", receipt.relation_id, 1),
                )
                .unwrap(),
            "oversized" => db
                .db
                .put_cf(cf, &event, vec![b'x'; MAX_CHANGE_BYTES + 1])
                .unwrap(),
            _ => {
                let mut node: StoredChange =
                    decode(&db.db.get_cf(cf, &event).unwrap().unwrap()).unwrap();
                if mode == "digest" {
                    node.previous_digest = "f".repeat(64);
                } else {
                    node.change.relation_revision = 2;
                    node.cursor.prefix_digest = node.digest("scope").unwrap();
                }
                db.db.put_cf(cf, &event, encode(&node).unwrap()).unwrap();
            }
        }
        assert!(
            matches!(
                db.relation_changes("scope", &query()),
                Err(Error::DataCorruption(_))
            ),
            "{mode}"
        );
        assert!(
            matches!(
                mutate(&db, receipt.relation_id, 1, "retire"),
                Err(Error::DataCorruption(_))
            ),
            "{mode}"
        );
        let bytes = db
            .db
            .get_cf(cf, record_key(RELATION, "scope", receipt.relation_id))
            .unwrap()
            .unwrap();
        assert_eq!(decode::<MemoryRelation>(&bytes).unwrap().revision, 1);
        assert!(
            db.db
                .get_cf(
                    cf,
                    super::super::history_key("scope", receipt.relation_id, 2)
                )
                .unwrap()
                .is_none()
        );
    }
}
#[test]
fn relation_feed_upgrade_does_not_reinvent_earlier_assertions_or_replays() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let cmd = assertion(&db);
    let receipt = db
        .apply_memory_relation_command("scope", &actor(), &cmd)
        .unwrap();
    // Model a volume written before the journal feature, retaining relation history and receipts.
    let cf = db.cf(crate::storage::cf::AGENT_META).unwrap();
    db.db
        .delete_cf(cf, record_prefix(JOURNAL, "scope"))
        .unwrap();
    db.db.delete_cf(cf, event_key("scope", 1)).unwrap();
    assert_eq!(
        db.apply_memory_relation_command("scope", &actor(), &cmd)
            .unwrap(),
        receipt
    );
    assert!(!db.relation_changes("scope", &query()).unwrap().active);
    let baseline = db.activate_relation_changes("scope").unwrap();
    assert_eq!(baseline.sequence, 0);
    mutate(&db, receipt.relation_id, 1, "retire").unwrap();
    let page = db.relation_changes("scope", &query()).unwrap();
    assert_eq!(page.changes.len(), 1);
    assert_eq!(page.changes[0].change.relation_revision, 2);
}

#[test]
fn relation_feed_staged_event_is_not_published_when_adjacency_preparation_fails() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let command = assertion(&db);
    let receipt = db
        .apply_memory_relation_command("scope", &actor(), &command)
        .unwrap();
    let before = db.relation_changes("scope", &query()).unwrap();
    let relation = db
        .read_memory_relation("scope", receipt.relation_id)
        .unwrap()
        .unwrap();
    let prefix = super::super::adjacency::prefix(OUTGOING, "scope", &relation.input.source);
    db.db
        .delete_cf(
            db.cf(crate::storage::cf::AGENT_META).unwrap(),
            super::super::adjacency::head_key(&prefix),
        )
        .unwrap();
    assert!(matches!(
        mutate(&db, receipt.relation_id, 1, "retire"),
        Err(Error::DataCorruption(_))
    ));
    assert_eq!(db.relation_changes("scope", &query()).unwrap(), before);
    assert!(
        db.memory_relation_revision("scope", receipt.relation_id, 2)
            .unwrap()
            .is_none()
    );
}
#[test]
fn relation_feed_concurrent_readers_observe_complete_atomic_prefixes() {
    use std::sync::{Arc, Barrier};
    let dir = TempDir::new().unwrap();
    let db = Arc::new(open(dir.path()));
    let command = assertion(&db);
    let receipt = db
        .apply_memory_relation_command("scope", &actor(), &command)
        .unwrap();
    let barrier = Arc::new(Barrier::new(3));
    let mut readers = vec![];
    for _ in 0..2 {
        let db = db.clone();
        let barrier = barrier.clone();
        readers.push(std::thread::spawn(move || {
            barrier.wait();
            for _ in 0..60 {
                let page = db.relation_changes("scope", &query()).unwrap();
                assert!(page.complete);
                assert_eq!(page.next_cursor, page.high_watermark);
                assert_eq!(
                    page.changes.len() as u64,
                    page.high_watermark.unwrap().sequence
                );
                for (i, event) in page.changes.iter().enumerate() {
                    assert_eq!(event.cursor.sequence, i as u64 + 1);
                    assert_eq!(event.change.relation_revision, i as u64 + 1);
                }
            }
        }));
    }
    barrier.wait();
    for revision in 1..=40 {
        mutate(&db, receipt.relation_id, revision, "review").unwrap();
    }
    for reader in readers {
        reader.join().unwrap();
    }
    assert_eq!(tip(&db).sequence, 41);
}
#[test]
fn relation_feed_maximum_page_and_fence_do_not_skip_positions() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let command = assertion(&db);
    let receipt = db
        .apply_memory_relation_command("scope", &actor(), &command)
        .unwrap();
    for revision in 1..=259 {
        mutate(&db, receipt.relation_id, revision, "review").unwrap();
    }
    let first = db.relation_changes("scope", &query()).unwrap();
    assert_eq!(first.changes.len(), 256);
    assert!(!first.complete);
    assert_eq!(first.next_cursor.as_ref().unwrap().sequence, 256);
    let second = db
        .relation_changes(
            "scope",
            &RelationChangesQuery {
                after: first.next_cursor,
                through: first.high_watermark,
                limit: 256,
            },
        )
        .unwrap();
    assert!(second.complete);
    assert_eq!(second.changes.len(), 4);
    assert_eq!(second.changes[0].cursor.sequence, 257);
    assert_eq!(second.next_cursor.unwrap().sequence, 260);
}
