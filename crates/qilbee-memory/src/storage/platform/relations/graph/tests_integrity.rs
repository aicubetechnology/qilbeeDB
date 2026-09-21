use super::*;

#[test]
fn typed_graph_detects_a_legacy_memory_writer_and_rebuilds_current_revision_heads() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let a = create(&db, "scope", "a");
    let b = create(&db, "scope", "b");
    link(&db, "scope", &a, &b, MemoryRelationKind::Supports, "edge");
    let cf = db.cf(crate::storage::cf::AGENT_META).unwrap();
    let tip_key = record_prefix(0x57, "scope");
    let old_tip = db.db.get_cf(cf, &tip_key).unwrap().unwrap();
    db.apply_memory_command(
        "scope",
        &actor(),
        &MemoryCommand {
            contract_version: 1,
            idempotency_key: "old-writer-update".into(),
            operation: MemoryOperation::Update {
                record_id: b.record_id,
                expected_revision: 1,
                record: input("changed target"),
            },
        },
    )
    .unwrap();
    // Reproduce an older memory binary: journal advances, but new revision heads and tip do not.
    db.db.put_cf(cf, tip_key, old_tip).unwrap();
    for kind in [OUTGOING, INCOMING] {
        let prefix = adjacency::prefix(
            kind,
            "scope",
            &MemorySourceRef {
                record_id: b.record_id,
                revision: 2,
            },
        );
        db.db.delete_cf(cf, adjacency::head_key(&prefix)).unwrap();
    }
    drop(db);
    let db = open(dir.path());
    let g = db
        .read_memory_typed_graph("scope", &query(vec![a.record_id, b.record_id]))
        .unwrap();
    assert!(g.coverage.complete);
    assert!(g.edges.is_empty());
    assert_eq!(g.nodes.len(), 2);
    assert_eq!(g.nodes[1].record.revision, 2);
}
fn clear_projection(db: &RocksDbMemoryStorage, namespace: &str) {
    let cf = db.cf(crate::storage::cf::AGENT_META).unwrap();
    let prefix = record_prefix(0x56, namespace);
    let keys: Vec<_> = db
        .db
        .iterator_cf(
            cf,
            rocksdb::IteratorMode::From(&prefix, rocksdb::Direction::Forward),
        )
        .map(|r| r.unwrap().0)
        .take_while(|k| k.starts_with(&prefix))
        .collect();
    for key in keys {
        db.db.delete_cf(cf, key).unwrap();
    }
    db.db.delete_cf(cf, record_prefix(0x57, namespace)).unwrap();
}

#[test]
fn typed_graph_detects_missing_outgoing_and_incoming_entries_even_when_both_disappear() {
    for direction in [
        TypedGraphDirection::Outgoing,
        TypedGraphDirection::Incoming,
        TypedGraphDirection::Both,
    ] {
        let dir = TempDir::new().unwrap();
        let db = open(dir.path());
        let a = create(&db, "scope", "a");
        let b = create(&db, "scope", "b");
        let edge = link(&db, "scope", &a, &b, MemoryRelationKind::Supports, "edge");
        let cf = db.cf(crate::storage::cf::AGENT_META).unwrap();
        for (kind, record) in [(OUTGOING, &a), (INCOMING, &b)] {
            db.db
                .delete_cf(
                    cf,
                    adjacency_key(
                        kind,
                        "scope",
                        &MemorySourceRef {
                            record_id: record.record_id,
                            revision: 1,
                        },
                        edge.relation_id,
                    ),
                )
                .unwrap();
        }
        let mut q = query(vec![if direction == TypedGraphDirection::Incoming {
            b.record_id
        } else {
            a.record_id
        }]);
        q.direction = direction;
        assert!(matches!(
            db.read_memory_typed_graph("scope", &q),
            Err(Error::DataCorruption(_))
        ));
        drop(db);
        let db = open(dir.path());
        assert!(
            matches!(
                db.read_memory_typed_graph("scope", &q),
                Err(Error::DataCorruption(_))
            ),
            "A matching ready tip must not silently repair corrupt serving indexes"
        );
    }
}

#[test]
fn typed_graph_migrates_in_bounded_batches_and_recovers_an_incomplete_build() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let a = create(&db, "scope", "root");
    for i in 0..130 {
        let b = create(&db, "scope", &format!("child-{i}"));
        link(
            &db,
            "scope",
            &a,
            &b,
            MemoryRelationKind::Supports,
            &format!("edge-{i}"),
        );
    }
    let mut q = query(vec![a.record_id]);
    q.node_limit = 256;
    let before = db.read_memory_typed_graph("scope", &q).unwrap();
    assert!(before.coverage.complete);
    let journal = db
        .memory_changes(
            "scope",
            &MemoryChangesQuery {
                after: None,
                through: None,
                limit: 256,
            },
        )
        .unwrap();
    clear_projection(&db, "scope");
    // Simulate a committed partial initialization without its final ready marker.
    let prefix = adjacency::prefix(
        OUTGOING,
        "scope",
        &MemorySourceRef {
            record_id: a.record_id,
            revision: 1,
        },
    );
    db.db
        .put_cf(
            db.cf(crate::storage::cf::AGENT_META).unwrap(),
            adjacency::head_key(&prefix),
            b"partial",
        )
        .unwrap();
    drop(db);
    let db = open(dir.path());
    let after = db.read_memory_typed_graph("scope", &q).unwrap();
    assert!(after.coverage.complete);
    assert_eq!(after.edges, before.edges);
    assert_eq!(
        serde_json::to_value(after.nodes).unwrap(),
        serde_json::to_value(before.nodes).unwrap()
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
        .unwrap(),
        journal
    );
}

#[test]
fn typed_graph_missing_head_fails_closed_and_failed_upgrade_never_marks_ready() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let a = create(&db, "scope", "a");
    let b = create(&db, "scope", "b");
    let edge = link(&db, "scope", &a, &b, MemoryRelationKind::Supports, "edge");
    let cf = db.cf(crate::storage::cf::AGENT_META).unwrap();
    let prefix = adjacency::prefix(
        OUTGOING,
        "scope",
        &MemorySourceRef {
            record_id: a.record_id,
            revision: 1,
        },
    );
    db.db.delete_cf(cf, adjacency::head_key(&prefix)).unwrap();
    assert!(matches!(
        db.read_memory_typed_graph("scope", &query(vec![a.record_id])),
        Err(Error::DataCorruption(_))
    ));
    clear_projection(&db, "scope");
    let key = adjacency_key(
        INCOMING,
        "scope",
        &MemorySourceRef {
            record_id: b.record_id,
            revision: 1,
        },
        edge.relation_id,
    );
    let saved = db.db.get_cf(cf, &key).unwrap().unwrap();
    db.db.delete_cf(cf, &key).unwrap();
    assert!(matches!(
        db.initialize_relation_adjacency(),
        Err(Error::DataCorruption(_))
    ));
    assert!(
        db.db
            .get_cf(cf, record_prefix(0x57, "scope"))
            .unwrap()
            .is_none()
    );
    db.db.put_cf(cf, key, saved).unwrap();
    db.initialize_relation_adjacency().unwrap();
    assert!(
        db.read_memory_typed_graph("scope", &query(vec![a.record_id]))
            .unwrap()
            .coverage
            .complete
    );
}
