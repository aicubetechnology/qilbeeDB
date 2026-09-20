use super::semantic_tests::{create, open};
use super::*;
use tempfile::TempDir;

fn query() -> VerifiedMemoryChangesQuery {
    VerifiedMemoryChangesQuery {
        after: None,
        through: None,
        limit: 256,
    }
}

fn key(prefix: u8, sequence: u64) -> Vec<u8> {
    let mut key = record_prefix(prefix, "scope");
    key.extend_from_slice(&sequence.to_be_bytes());
    key
}

#[test]
fn journal_audit_covers_an_explicit_fence_and_does_not_advance_it() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    for n in 0..4 {
        create(&db, "scope", &format!("event-{n}"));
    }
    let first = db
        .audit_memory_journal(
            "scope",
            &VerifiedMemoryChangesQuery {
                limit: 2,
                ..query()
            },
        )
        .unwrap();
    assert_eq!(first.links_checked, 2);
    assert_eq!(first.checked_after.as_ref().unwrap().sequence, 0);
    assert_eq!(first.checked_through.as_ref().unwrap().sequence, 2);
    assert!(!first.complete);
    create(&db, "scope", "later");
    let next = VerifiedMemoryChangesQuery {
        after: first.checked_through,
        through: first.high_watermark,
        limit: 2,
    };
    let last = db.audit_memory_journal("scope", &next).unwrap();
    assert_eq!(last.links_checked, 2);
    assert!(last.complete);
    assert_eq!(last.checked_through.as_ref().unwrap().sequence, 4);
    let encoded = serde_json::to_string(&last).unwrap();
    for field in ["record_id", "author", "changes"] {
        assert!(!encoded.contains(field));
    }
    drop(db);
    let db = open(dir.path());
    assert_eq!(db.audit_memory_journal("scope", &next).unwrap(), last);
    assert_eq!(
        db.verified_memory_changes("scope", &query())
            .unwrap()
            .high_watermark
            .unwrap()
            .sequence,
        5
    );
}

#[test]
fn journal_audit_distinguishes_inactive_empty_and_legacy_only_history() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let inactive = db.audit_memory_journal("scope", &query()).unwrap();
    assert!(!inactive.active);
    assert!(
        inactive.complete
            && inactive.baseline.is_none()
            && inactive.checked_after.is_none()
            && inactive.checked_through.is_none()
            && inactive.high_watermark.is_none()
    );
    assert_eq!(inactive.links_checked, 0);
    let baseline = db.activate_verified_memory_journal("scope").unwrap();
    let empty = db.audit_memory_journal("scope", &query()).unwrap();
    assert!(empty.active && empty.complete);
    assert_eq!(empty.links_checked, 0);
    assert_eq!(empty.checked_after, Some(baseline.clone()));
    assert_eq!(empty.checked_through, Some(baseline));
    for n in 1..=3 {
        create(&db, "scope", &format!("legacy-{n}"));
    }
    // Reproduce an old-format volume by removing only the version-two sidecar.
    let cf = db.cf(super::super::cf::AGENT_META).unwrap();
    db.db.delete_cf(cf, record_prefix(0x26, "scope")).unwrap();
    for n in 1..=3 {
        db.db.delete_cf(cf, key(0x27, n)).unwrap();
    }
    assert_eq!(
        db.audit_memory_journal("scope", &query()).unwrap(),
        inactive
    );
    let baseline = db.activate_verified_memory_journal("scope").unwrap();
    assert_eq!(baseline.sequence, 3);
    // Pre-baseline damage is outside this audit's coverage; do not imply legacy verification.
    db.db.delete_cf(cf, key(0x21, 1)).unwrap();
    let legacy = db.audit_memory_journal("scope", &query()).unwrap();
    assert!(legacy.active && legacy.complete);
    assert_eq!(legacy.links_checked, 0);
    assert_eq!(legacy.baseline, Some(baseline));
    create(&db, "scope", "anchored");
    assert_eq!(
        db.audit_memory_journal("scope", &query())
            .unwrap()
            .links_checked,
        1
    );
}

#[test]
fn journal_audit_detects_missing_or_damaged_middle_entries_beyond_a_valid_tip() {
    for (prefix, remove) in [(0x21, true), (0x27, true), (0x21, false), (0x27, false)] {
        let dir = TempDir::new().unwrap();
        let db = open(dir.path());
        for n in 0..4 {
            create(&db, "scope", &format!("event-{n}"));
        }
        let tip = db
            .audit_memory_journal("scope", &query())
            .unwrap()
            .high_watermark;
        let cf = db.cf(super::super::cf::AGENT_META).unwrap();
        let middle = key(prefix, 2);
        if remove {
            db.db.delete_cf(cf, middle).unwrap();
        } else {
            let mut entry: serde_json::Value =
                decode(&db.db.get_cf(cf, &middle).unwrap().unwrap()).unwrap();
            if prefix == 0x21 {
                entry["record_revision"] = 999.into();
            } else {
                entry["previous_digest"] = "f".repeat(64).into();
            }
            db.db.put_cf(cf, middle, encode(&entry).unwrap()).unwrap();
        }
        assert!(
            db.audit_memory_journal(
                "scope",
                &VerifiedMemoryChangesQuery {
                    after: tip,
                    ..query()
                }
            )
            .unwrap()
            .complete
        );
        let first = db
            .audit_memory_journal(
                "scope",
                &VerifiedMemoryChangesQuery {
                    limit: 1,
                    ..query()
                },
            )
            .unwrap();
        assert_eq!(first.links_checked, 1);
        let next = VerifiedMemoryChangesQuery {
            after: first.checked_through,
            through: first.high_watermark,
            ..query()
        };
        assert!(matches!(
            db.audit_memory_journal("scope", &next),
            Err(Error::DataCorruption(_))
        ));
        assert!(matches!(
            db.audit_memory_journal("scope", &query()),
            Err(Error::DataCorruption(_))
        ));
    }
}

#[test]
fn journal_audit_rejects_divergent_restore_cursors_after_sequence_catch_up() {
    let dir = TempDir::new().unwrap();
    let db = open(&dir.path().join("live"));
    create(&db, "scope", "shared-prefix");
    let common = db
        .audit_memory_journal("scope", &query())
        .unwrap()
        .high_watermark;
    let backup = dir.path().join("backup");
    rocksdb::checkpoint::Checkpoint::new(&db.db)
        .unwrap()
        .create_checkpoint(&backup)
        .unwrap();
    create(&db, "scope", "original-branch");
    let lost = db
        .audit_memory_journal("scope", &query())
        .unwrap()
        .high_watermark;
    let restored = open(&backup);
    create(&restored, "scope", "new-branch");
    create(&restored, "scope", "past-old-tip");
    let resumed = restored
        .audit_memory_journal(
            "scope",
            &VerifiedMemoryChangesQuery {
                after: common,
                ..query()
            },
        )
        .unwrap();
    assert_eq!(resumed.links_checked, 2);
    for fence in [false, true] {
        let mut q = query();
        if fence {
            q.through = lost.clone();
        } else {
            q.after = lost.clone();
        }
        assert!(matches!(
            restored.audit_memory_journal("scope", &q),
            Err(Error::ConstraintViolation(_))
        ));
    }
}

#[test]
fn journal_audit_checks_link_continuity_even_when_an_anchor_digest_is_valid() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    for n in 0..4 {
        create(&db, "scope", &format!("event-{n}"));
    }
    let cf = db.cf(super::super::cf::AGENT_META).unwrap();
    let middle = key(0x27, 2);
    let mut anchor: serde_json::Value =
        decode(&db.db.get_cf(cf, &middle).unwrap().unwrap()).unwrap();
    anchor["previous_digest"] = "f".repeat(64).into();
    // Create an individually consistent fixture whose predecessor link is wrong.
    let encoded = encode(&serde_json::json!([
        "qilbee.memory.journal.anchor.v2",
        "scope",
        anchor["cursor"]["version"],
        anchor["cursor"]["journal_id"],
        anchor["cursor"]["generation"],
        anchor["cursor"]["sequence"],
        anchor["previous_digest"],
        anchor["nonce"],
        anchor["event_digest"]
    ]))
    .unwrap();
    anchor["cursor"]["prefix_digest"] = digest(&encoded)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
        .into();
    let cursor: VerifiedMemoryCursor = serde_json::from_value(anchor["cursor"].clone()).unwrap();
    db.db.put_cf(cf, middle, encode(&anchor).unwrap()).unwrap();
    let point = db
        .audit_memory_journal(
            "scope",
            &VerifiedMemoryChangesQuery {
                after: Some(cursor.clone()),
                through: Some(cursor),
                limit: 1,
            },
        )
        .unwrap();
    assert!(point.complete);
    assert_eq!(point.links_checked, 0);
    assert!(matches!(
        db.audit_memory_journal("scope", &query()),
        Err(Error::DataCorruption(_))
    ));
}
