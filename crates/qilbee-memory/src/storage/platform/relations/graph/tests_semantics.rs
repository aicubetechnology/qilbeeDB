use super::*;

#[test]
fn typed_graph_concurrent_readers_never_observe_partial_adjacency_publication() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let a = create(&db, "scope", "a");
    let b = create(&db, "scope", "b");
    let edge = link(&db, "scope", &a, &b, MemoryRelationKind::Supports, "edge");
    let start = std::sync::Barrier::new(4);
    std::thread::scope(|threads| {
        threads.spawn(|| {
            start.wait();
            for i in 0..40 {
                db.apply_memory_relation_command(
                    "scope",
                    &actor(),
                    &MemoryRelationCommand {
                        contract_version: 1,
                        idempotency_key: format!("review-{i}"),
                        operation: MemoryRelationOperation::Review {
                            relation_id: edge.relation_id,
                            expected_revision: i + 1,
                            disposition: if i % 2 == 0 {
                                MemoryReviewDisposition::Rejected
                            } else {
                                MemoryReviewDisposition::Approved
                            },
                            evidence_ref: "trace://concurrent-review".into(),
                        },
                    },
                )
                .unwrap();
            }
        });
        for _ in 0..2 {
            threads.spawn(|| {
                start.wait();
                for _ in 0..80 {
                    let g = db
                        .read_memory_typed_graph("scope", &query(vec![a.record_id]))
                        .unwrap();
                    assert!(g.coverage.complete);
                    assert!(g.edges.len() <= 1);
                    assert_eq!(g.coverage.adjacency_entries_examined, g.edges.len() * 2);
                    assert_eq!(g.nodes.len(), 1 + g.edges.len());
                }
            });
        }
        start.wait();
    });
    let g = db
        .read_memory_typed_graph("scope", &query(vec![a.record_id]))
        .unwrap();
    assert_eq!(g.edges[0].revision, 41);
    assert!(g.coverage.complete);
}

#[test]
fn typed_graph_revalidates_transitive_sources_at_depth_zero_and_preserves_old_snapshots() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let source = create(&db, "scope", "source");
    let derived = db
        .apply_memory_command(
            "scope",
            &actor(),
            &MemoryCommand {
                contract_version: 1,
                idempotency_key: "derived".into(),
                operation: MemoryOperation::Derive {
                    record: input("derived"),
                    derivation: MemoryDerivation {
                        sources: vec![MemorySourceRef {
                            record_id: source.record_id,
                            revision: 1,
                        }],
                        method: "fixture".into(),
                        method_revision: "v1".into(),
                        evidence_ref: "trace://derived".into(),
                    },
                },
            },
        )
        .unwrap();
    let a = create(&db, "scope", "root");
    link(
        &db,
        "scope",
        &a,
        &derived,
        MemoryRelationKind::Supports,
        "edge",
    );
    let old = db.memory_snapshot();
    let before = old.typed_graph("scope", &query(vec![a.record_id])).unwrap();
    assert_eq!(before.edges.len(), 1);
    assert_eq!(before.coverage.dependency_work.records_examined, 1);
    let mut zero = query(vec![derived.record_id]);
    zero.max_depth = 0;
    assert_eq!(
        db.read_memory_typed_graph("scope", &zero).unwrap().roots[0].status,
        MemoryGraphRootStatus::Included
    );
    db.apply_memory_command(
        "scope",
        &actor(),
        &MemoryCommand {
            contract_version: 1,
            idempotency_key: "correct-source".into(),
            operation: MemoryOperation::Update {
                record_id: source.record_id,
                expected_revision: 1,
                record: input("corrected"),
            },
        },
    )
    .unwrap();
    let current = db
        .read_memory_typed_graph("scope", &query(vec![a.record_id]))
        .unwrap();
    assert!(current.edges.is_empty());
    assert_eq!(current.nodes.len(), 1);
    assert!(current.coverage.complete);
    assert_eq!(
        db.read_memory_typed_graph("scope", &zero).unwrap().roots[0].status,
        MemoryGraphRootStatus::Unavailable
    );
    assert_eq!(
        old.typed_graph("scope", &query(vec![a.record_id]))
            .unwrap()
            .edges,
        before.edges
    );
}

#[test]
fn typed_graph_retirement_review_restore_and_duplicate_replays_preserve_index_counts() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let a = create(&db, "scope", "a");
    let b = create(&db, "scope", "b");
    let edge = link(
        &db,
        "scope",
        &a,
        &b,
        MemoryRelationKind::CausalClaim,
        "edge",
    );
    for (revision, action, expected) in [
        (1, "retire", 0),
        (2, "restore", 1),
        (3, "reject", 0),
        (4, "approve", 1),
    ] {
        let evidence_ref = "trace://decision".into();
        let operation = match action {
            "retire" => MemoryRelationOperation::Retire {
                relation_id: edge.relation_id,
                expected_revision: revision,
                evidence_ref,
            },
            "restore" => MemoryRelationOperation::Restore {
                relation_id: edge.relation_id,
                expected_revision: revision,
                evidence_ref,
            },
            _ => MemoryRelationOperation::Review {
                relation_id: edge.relation_id,
                expected_revision: revision,
                evidence_ref,
                disposition: if action == "reject" {
                    MemoryReviewDisposition::Rejected
                } else {
                    MemoryReviewDisposition::Approved
                },
            },
        };
        let command = MemoryRelationCommand {
            contract_version: 1,
            idempotency_key: action.into(),
            operation,
        };
        let receipt = db
            .apply_memory_relation_command("scope", &actor(), &command)
            .unwrap();
        assert_eq!(
            db.apply_memory_relation_command("scope", &actor(), &command)
                .unwrap(),
            receipt
        );
        let g = db
            .read_memory_typed_graph("scope", &query(vec![a.record_id]))
            .unwrap();
        assert!(g.coverage.complete);
        assert_eq!(g.edges.len(), expected);
        assert_eq!(g.coverage.adjacency_entries_examined, expected * 2);
    }
    drop(db);
    let db = open(dir.path());
    let g = db
        .read_memory_typed_graph("scope", &query(vec![a.record_id]))
        .unwrap();
    assert!(g.coverage.complete);
    assert_eq!(g.edges[0].input.kind, MemoryRelationKind::CausalClaim);
    assert_eq!(g.edges[0].revision, 5);
}

#[test]
fn typed_graph_checks_relation_and_endpoint_expiry_without_feed_events() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let a = create(&db, "scope", "a");
    let until = chrono::Utc::now().timestamp_millis() + 60_000;
    let mut payload = input("expiring");
    payload.valid_until_millis = Some(until);
    let b = db
        .apply_memory_command(
            "scope",
            &actor(),
            &MemoryCommand {
                contract_version: 1,
                idempotency_key: "b".into(),
                operation: MemoryOperation::Create { record: payload },
            },
        )
        .unwrap();
    let command = MemoryRelationCommand {
        contract_version: 1,
        idempotency_key: "interval".into(),
        operation: MemoryRelationOperation::Assert {
            relation: MemoryRelationInput {
                evidence_sources: Vec::new(),
                source: MemorySourceRef {
                    record_id: a.record_id,
                    revision: 1,
                },
                target: MemorySourceRef {
                    record_id: b.record_id,
                    revision: 1,
                },
                kind: MemoryRelationKind::SemanticRelated,
                provenance: RelationProvenance {
                    origin: RelationOrigin::ImportedAssertion,
                    method: "fixture".into(),
                    method_revision: "v1".into(),
                    evidence_ref: "trace://interval".into(),
                    model: None,
                },
                valid_from_millis: Some(until - 1000),
                valid_until_millis: Some(until - 100),
            },
        },
    };
    db.apply_memory_relation_command("scope", &actor(), &command)
        .unwrap();
    link(
        &db,
        "scope",
        &a,
        &b,
        MemoryRelationKind::Supports,
        "endpoint-expiry",
    );
    let feed = db
        .memory_changes(
            "scope",
            &MemoryChangesQuery {
                after: None,
                through: None,
                limit: 256,
            },
        )
        .unwrap();
    for (now, edges) in [
        (until - 1001, 0),
        (until - 1000, 1),
        (until - 101, 1),
        (until - 100, 0),
        (until, 0),
        (until + 1000, 0),
    ] {
        let mut view = db.memory_snapshot();
        view.now = now;
        let mut q = query(vec![a.record_id]);
        q.relation_kinds = vec![MemoryRelationKind::SemanticRelated];
        let g = view.typed_graph("scope", &q).unwrap();
        assert_eq!(g.edges.len(), edges);
        assert!(g.coverage.complete);
        assert_eq!(g.evaluated_at_millis, now);
    }
    for (now, expected) in [(until - 1, 1), (until, 0)] {
        let mut view = db.memory_snapshot();
        view.now = now;
        let mut q = query(vec![a.record_id]);
        q.relation_kinds = vec![MemoryRelationKind::Supports];
        assert_eq!(view.typed_graph("scope", &q).unwrap().edges.len(), expected);
    }
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
        feed
    );
}

#[test]
fn typed_graph_foreign_namespace_corruption_does_not_affect_rows_or_statistics() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let a = create(&db, "scope", "same text");
    let b = create(&db, "scope", "same target");
    link(&db, "scope", &a, &b, MemoryRelationKind::SameEntity, "edge");
    let foreign = create(&db, "scope-longer", "same text");
    let other = create(&db, "scope-longer", "same target");
    let edge = link(
        &db,
        "scope-longer",
        &foreign,
        &other,
        MemoryRelationKind::SameEntity,
        "edge",
    );
    let before = db
        .read_memory_typed_graph("scope", &query(vec![a.record_id]))
        .unwrap();
    db.db
        .put_cf(
            db.cf(crate::storage::cf::AGENT_META).unwrap(),
            record_key(RELATION, "scope-longer", edge.relation_id),
            b"corrupt foreign metadata",
        )
        .unwrap();
    let after = db
        .read_memory_typed_graph("scope", &query(vec![a.record_id]))
        .unwrap();
    assert_eq!(after.edges, before.edges);
    assert_eq!(
        serde_json::to_value(&after.coverage).unwrap(),
        serde_json::to_value(&before.coverage).unwrap()
    );
    let absent = db
        .read_memory_typed_graph("scope", &query(vec![foreign.record_id]))
        .unwrap();
    assert_eq!(absent.roots[0].status, MemoryGraphRootStatus::Unavailable);
    assert_eq!(absent.coverage.relations_examined, 0);
    assert_eq!(absent.coverage.record_bytes, 0);
}
