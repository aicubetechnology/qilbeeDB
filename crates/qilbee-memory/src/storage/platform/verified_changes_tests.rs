use super::semantic_tests::{actor, create, open};
use super::*;
use tempfile::TempDir;
fn query() -> VerifiedMemoryChangesQuery {
    VerifiedMemoryChangesQuery {
        after: None,
        through: None,
        limit: 256,
    }
}
fn tip(db: &RocksDbMemoryStorage) -> VerifiedMemoryCursor {
    db.verified_memory_changes("scope", &query())
        .unwrap()
        .high_watermark
        .unwrap()
}
#[test]
fn verified_feed_detects_divergent_restore_after_sequences_catch_up() {
    let dir = TempDir::new().unwrap();
    let db = open(&dir.path().join("live"));
    create(&db, "scope", "shared-prefix");
    let prefix = tip(&db);
    let backup = dir.path().join("backup");
    rocksdb::checkpoint::Checkpoint::new(&db.db)
        .unwrap()
        .create_checkpoint(&backup)
        .unwrap();
    create(&db, "scope", "original-branch");
    let original = tip(&db);
    let restored = open(&backup);
    create(&restored, "scope", "restored-branch");
    create(&restored, "scope", "catch-up");
    assert!(tip(&restored).sequence > original.sequence);
    // A valid common-prefix cursor survives. A cursor from the lost branch does not.
    assert!(
        restored
            .verified_memory_changes(
                "scope",
                &VerifiedMemoryChangesQuery {
                    after: Some(prefix),
                    ..query()
                }
            )
            .is_ok()
    );
    for fence in [false, true] {
        let mut q = query();
        if fence {
            q.through = Some(original.clone());
        } else {
            q.after = Some(original.clone());
        }
        assert!(matches!(
            restored.verified_memory_changes("scope", &q),
            Err(Error::JournalHistoryConflict(_))
        ));
    }
    // The compatibility endpoint intentionally retains its older sequence-only semantics.
    assert!(
        restored
            .memory_changes(
                "scope",
                &MemoryChangesQuery {
                    after: Some(MemoryChangeCursor {
                        journal_id: original.journal_id,
                        sequence: original.sequence
                    }),
                    through: None,
                    limit: 10
                }
            )
            .is_ok()
    );
}
#[test]
fn verified_feed_is_bounded_durable_idempotent_and_scope_local() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    assert!(
        !db.verified_memory_changes("scope", &query())
            .unwrap()
            .active
    );
    create(&db, "scope", "one");
    create(&db, "scope", "two");
    let first = db
        .verified_memory_changes(
            "scope",
            &VerifiedMemoryChangesQuery {
                limit: 1,
                ..query()
            },
        )
        .unwrap();
    assert_eq!(first.baseline.as_ref().unwrap().sequence, 0);
    assert!(!first.complete);
    create(&db, "scope", "three");
    let q = VerifiedMemoryChangesQuery {
        after: first.next_cursor,
        through: first.high_watermark,
        limit: 1,
    };
    let second = db.verified_memory_changes("scope", &q).unwrap();
    assert!(second.complete);
    assert_eq!(second.changes.len(), 1);
    assert!(matches!(
        db.verified_memory_changes("foreign", &q),
        Err(Error::JournalHistoryConflict(_))
    ));
    drop(db);
    let db = open(dir.path());
    assert_eq!(db.verified_memory_changes("scope", &q).unwrap(), second);
    let mut bad = q;
    bad.after.as_mut().unwrap().prefix_digest = "00".into();
    assert!(matches!(
        db.verified_memory_changes("scope", &bad),
        Err(Error::ValidationError(_))
    ));
}
#[test]
fn legacy_upgrade_exposes_a_boundary_without_inventing_verified_history() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    create(&db, "scope", "legacy");
    let cf = db.cf(super::super::cf::AGENT_META).unwrap();
    db.db.delete_cf(cf, record_prefix(0x26, "scope")).unwrap();
    let mut key = record_prefix(0x27, "scope");
    key.extend_from_slice(&1_u64.to_be_bytes());
    db.db.delete_cf(cf, key).unwrap();
    assert!(
        !db.verified_memory_changes("scope", &query())
            .unwrap()
            .active
    );
    create(&db, "scope", "new");
    let page = db.verified_memory_changes("scope", &query()).unwrap();
    assert_eq!(page.baseline.unwrap().sequence, 1);
    assert_eq!(page.changes.len(), 1);
    assert_eq!(page.changes[0].cursor.sequence, 2);
}
#[test]
fn corrupt_anchor_prevents_new_mutations_and_returns_no_partial_feed() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    create(&db, "scope", "first");
    let cf = db.cf(super::super::cf::AGENT_META).unwrap();
    let mut key = record_prefix(0x27, "scope");
    key.extend_from_slice(&1_u64.to_be_bytes());
    let mut stored: serde_json::Value = decode(&db.db.get_cf(cf, &key).unwrap().unwrap()).unwrap();
    stored["previous_digest"] = "f".repeat(64).into();
    db.db.put_cf(cf, key, encode(&stored).unwrap()).unwrap();
    assert!(matches!(
        db.verified_memory_changes("scope", &query()),
        Err(Error::DataCorruption(_))
    ));
    let command: MemoryCommand = serde_json::from_value(serde_json::json!({"contract_version":1,"idempotency_key":"must-not-commit","operation":{"type":"create","record":{"episode_type":"Observation","event_time_millis":1,"content":{"primary":"blocked"}}}})).unwrap();
    assert!(matches!(
        db.apply_memory_command("scope", &actor(), &command),
        Err(Error::DataCorruption(_))
    ));
    assert_eq!(
        db.memory_changes(
            "scope",
            &MemoryChangesQuery {
                after: None,
                through: None,
                limit: 10
            }
        )
        .unwrap()
        .changes
        .len(),
        1
    );
}

#[test]
fn explicit_activation_is_concurrently_idempotent_and_survives_empty_journal_restart() {
    use std::sync::{Arc, Barrier};
    let dir = TempDir::new().unwrap();
    let db = Arc::new(open(dir.path()));
    let gate = Arc::new(Barrier::new(10));
    let handles: Vec<_> = (0..10)
        .map(|_| {
            let db = db.clone();
            let gate = gate.clone();
            std::thread::spawn(move || {
                gate.wait();
                db.activate_verified_memory_journal("scope").unwrap()
            })
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert!(results.iter().all(|r| r == &results[0]));
    assert_eq!(results[0].sequence, 0);
    let baseline = results[0].clone();
    drop(db);
    let db = open(dir.path());
    assert_eq!(
        db.activate_verified_memory_journal("scope").unwrap(),
        baseline
    );
    let page = db.verified_memory_changes("scope", &query()).unwrap();
    assert!(page.active && page.complete);
    assert!(page.changes.is_empty());
    assert_eq!(page.high_watermark, Some(baseline.clone()));
    assert!(
        db.memory_changes(
            "scope",
            &MemoryChangesQuery {
                after: None,
                through: None,
                limit: 1
            }
        )
        .unwrap()
        .high_watermark
        .is_none()
    );
    create(&db, "scope", "first-after-activation");
    let page = db
        .verified_memory_changes(
            "scope",
            &VerifiedMemoryChangesQuery {
                after: Some(baseline.clone()),
                ..query()
            },
        )
        .unwrap();
    assert_eq!(page.changes.len(), 1);
    assert_eq!(page.high_watermark.unwrap().journal_id, baseline.journal_id);
    assert_eq!(
        db.activate_verified_memory_journal("scope").unwrap(),
        baseline
    );
}
#[test]
fn explicit_activation_preserves_legacy_events_and_starts_after_their_tip() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    for i in 1..=3 {
        create(&db, "scope", &format!("legacy-{i}"));
    }
    let q = MemoryChangesQuery {
        after: None,
        through: None,
        limit: 10,
    };
    let before = db.memory_changes("scope", &q).unwrap();
    let cf = db.cf(super::super::cf::AGENT_META).unwrap();
    db.db.delete_cf(cf, record_prefix(0x26, "scope")).unwrap();
    for n in 1_u64..=3 {
        let mut key = record_prefix(0x27, "scope");
        key.extend_from_slice(&n.to_be_bytes());
        db.db.delete_cf(cf, key).unwrap();
    }
    let baseline = db.activate_verified_memory_journal("scope").unwrap();
    assert_eq!(baseline.sequence, 3);
    assert_eq!(db.memory_changes("scope", &q).unwrap(), before);
    assert!(
        db.verified_memory_changes("scope", &query())
            .unwrap()
            .changes
            .is_empty()
    );
    create(&db, "scope", "first-new");
    let page = db.verified_memory_changes("scope", &query()).unwrap();
    assert_eq!(page.changes.len(), 1);
    assert_eq!(page.changes[0].cursor.sequence, 4);
    assert_eq!(page.baseline, Some(baseline));
}
