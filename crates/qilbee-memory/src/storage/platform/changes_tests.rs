use super::*;
use crate::MemoryStorageConfig;
use std::sync::{Arc, Barrier};
fn open(path: &std::path::Path) -> RocksDbMemoryStorage {
    RocksDbMemoryStorage::open(MemoryStorageConfig {
        path: path.to_str().unwrap().into(),
        enable_wal: true,
        sync_writes: true,
        ..Default::default()
    })
    .unwrap()
}
fn author() -> RecordAuthor {
    RecordAuthor {
        credential_id: Uuid::new_v4(),
        subject_id: "agent".into(),
    }
}
fn create(key: &str) -> MemoryCommand {
    serde_json::from_value(serde_json::json!({"contract_version":1,"idempotency_key":key,"operation":{"type":"create","record":{"episode_type":"Observation","event_time_millis":1,"content":{"primary":"journal fixture"}}}})).unwrap()
}
fn query() -> MemoryChangesQuery {
    MemoryChangesQuery {
        after: None,
        through: None,
        limit: 256,
    }
}
#[test]
fn memory_changes_are_atomic_ordered_idempotent_and_recoverable() {
    let dir = tempfile::TempDir::new().unwrap();
    let db = open(dir.path());
    let author = author();
    let command = create("create");
    assert_eq!(
        db.memory_changes("scope", &query()).unwrap().high_watermark,
        None
    );
    let created = db.apply_memory_command("scope", &author, &command).unwrap();
    assert_eq!(
        created,
        db.apply_memory_command("scope", &author, &command).unwrap()
    );
    let embedding = EmbeddingCommand {
        contract_version: 1,
        idempotency_key: "embed".into(),
        record_id: created.record_id,
        record_revision: 1,
        space: EmbeddingSpace {
            provider: "fixture".into(),
            model: "fixture".into(),
            revision: "v1".into(),
            dimensions: 2,
        },
        vector: vec![1.0, 0.0],
    };
    db.apply_memory_embedding("scope", &author, &embedding)
        .unwrap();
    db.apply_memory_embedding("scope", &author, &embedding)
        .unwrap();
    let mut identical = embedding.clone();
    identical.idempotency_key = "same-value".into();
    db.apply_memory_embedding("scope", &author, &identical)
        .unwrap();
    let delete = MemoryCommand {
        contract_version: 1,
        idempotency_key: "delete".into(),
        operation: MemoryOperation::Delete {
            record_id: created.record_id,
            expected_revision: 1,
        },
    };
    db.apply_memory_command("scope", &author, &delete).unwrap();
    let page = db.memory_changes("scope", &query()).unwrap();
    assert_eq!(
        page.changes.iter().map(|c| c.kind).collect::<Vec<_>>(),
        vec![
            MemoryChangeKind::Created,
            MemoryChangeKind::EmbeddingAttached,
            MemoryChangeKind::Deleted
        ]
    );
    assert_eq!(
        page.changes
            .iter()
            .map(|c| c.cursor.sequence)
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    assert_eq!(page.changes[2].record_revision, 2);
    assert!(page.complete);
    assert!(
        page.changes
            .iter()
            .all(|c| c.author == author && c.record_id == created.record_id)
    );
    assert!(
        db.read_memory_record("scope", created.record_id)
            .unwrap()
            .is_none()
    );
    let mut stale = delete;
    stale.idempotency_key = "conflict".into();
    assert!(db.apply_memory_command("scope", &author, &stale).is_err());
    assert_eq!(db.memory_changes("scope", &query()).unwrap(), page);
    drop(db);
    let db = open(dir.path());
    assert_eq!(db.memory_changes("scope", &query()).unwrap(), page);
    assert_eq!(
        db.apply_memory_command("scope", &author, &command).unwrap(),
        created
    );
    assert_eq!(db.memory_changes("scope", &query()).unwrap(), page);
}
#[test]
fn memory_changes_preserve_fences_progress_and_scope() {
    let dir = tempfile::TempDir::new().unwrap();
    let db = open(dir.path());
    let author = author();
    for n in 0..3 {
        db.apply_memory_command("scope", &author, &create(&n.to_string()))
            .unwrap();
    }
    let mut q = query();
    q.limit = 1;
    let first = db.memory_changes("scope", &q).unwrap();
    assert!(!first.complete);
    db.apply_memory_command("scope", &author, &create("later"))
        .unwrap();
    q.after = first.next_cursor;
    q.through = first.high_watermark;
    let second = db.memory_changes("scope", &q).unwrap();
    assert_eq!(second.changes[0].cursor.sequence, 2);
    assert!(!second.complete);
    q.after = second.next_cursor;
    let third = db.memory_changes("scope", &q).unwrap();
    assert_eq!(third.changes[0].cursor.sequence, 3);
    assert!(third.complete);
    q.after = third.next_cursor;
    q.through = None;
    let fourth = db.memory_changes("scope", &q).unwrap();
    assert_eq!(fourth.changes[0].cursor.sequence, 4);
    db.apply_memory_command("foreign", &author, &create("one"))
        .unwrap();
    assert!(db.memory_changes("foreign", &q).is_err());
    assert!(db.memory_changes("empty", &q).is_err());
    q.after.as_mut().unwrap().sequence = 99;
    assert!(db.memory_changes("scope", &q).is_err());
    for limit in [0, 257] {
        let mut q = query();
        q.limit = limit;
        assert!(db.memory_changes("scope", &q).is_err());
    }
}
#[test]
fn concurrent_memory_changes_have_one_contiguous_commit_order() {
    let dir = tempfile::TempDir::new().unwrap();
    let db = Arc::new(open(dir.path()));
    let barrier = Arc::new(Barrier::new(8));
    let actor = author();
    let workers: Vec<_> = (0..8)
        .map(|i| {
            let db = db.clone();
            let b = barrier.clone();
            let a = actor.clone();
            std::thread::spawn(move || {
                b.wait();
                db.apply_memory_command("scope", &a, &create(&i.to_string()))
                    .unwrap()
            })
        })
        .collect();
    let receipts: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
    let page = db.memory_changes("scope", &query()).unwrap();
    assert_eq!(page.changes.len(), 8);
    for (i, change) in page.changes.iter().enumerate() {
        assert_eq!(change.cursor.sequence, i as u64 + 1);
        assert!(receipts.iter().any(|r| r.record_id == change.record_id));
    }
}
