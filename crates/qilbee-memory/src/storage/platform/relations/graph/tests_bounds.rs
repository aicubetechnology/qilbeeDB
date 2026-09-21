use super::*;

#[test]
fn typed_graph_relation_metadata_bytes_are_bounded_across_parallel_assertions() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let a = create(&db, "scope", "a");
    let b = create(&db, "scope", "b");
    let mut command = MemoryRelationCommand {
        contract_version: 1,
        idempotency_key: "large-0".into(),
        operation: MemoryRelationOperation::Assert {
            relation: MemoryRelationInput {
                source: MemorySourceRef {
                    record_id: a.record_id,
                    revision: 1,
                },
                target: MemorySourceRef {
                    record_id: b.record_id,
                    revision: 1,
                },
                kind: MemoryRelationKind::Supports,
                provenance: RelationProvenance {
                    origin: RelationOrigin::ModelInference,
                    method: "\"".repeat(256),
                    method_revision: "\"".repeat(256),
                    evidence_ref: "\"".repeat(2048),
                    model: Some(RelationModelIdentity {
                        provider: "\"".repeat(256),
                        model: "\"".repeat(256),
                        revision: "\"".repeat(256),
                    }),
                },
                valid_from_millis: None,
                valid_until_millis: None,
            },
        },
    };
    let first = db
        .apply_memory_relation_command("scope", &actor(), &command)
        .unwrap();
    let bytes = encode(
        &db.read_memory_relation("scope", first.relation_id)
            .unwrap()
            .unwrap(),
    )
    .unwrap()
    .len();
    let count = MAX_TYPED_GRAPH_RELATION_BYTES / bytes + 2;
    assert!(count < MAX_TYPED_GRAPH_EDGES);
    for i in 1..count {
        command.idempotency_key = format!("large-{i}");
        db.apply_memory_relation_command("scope", &actor(), &command)
            .unwrap();
    }
    let mut q = query(vec![a.record_id]);
    q.direction = TypedGraphDirection::Outgoing;
    q.edge_limit = MAX_TYPED_GRAPH_EDGES;
    q.scan_limit = MAX_TYPED_GRAPH_SCAN;
    assert!(matches!(
        db.read_memory_typed_graph("scope", &q),
        Err(Error::ValidationError(_))
    ));
}

#[test]
fn typed_graph_cuts_separate_depth_nodes_edges_scan_and_unexamined_roots() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let a = create(&db, "scope", "a");
    let b = create(&db, "scope", "b");
    let c = create(&db, "scope", "c");
    link(&db, "scope", &a, &b, MemoryRelationKind::Supports, "ab");
    link(&db, "scope", &b, &c, MemoryRelationKind::Supports, "bc");
    let mut q = query(vec![a.record_id]);
    q.direction = TypedGraphDirection::Outgoing;
    for (field, reason, nodes) in [
        ("depth", TypedGraphStopReason::DepthLimit, 2),
        ("nodes", TypedGraphStopReason::NodeLimit, 1),
        ("edges", TypedGraphStopReason::EdgeLimit, 2),
        ("scan", TypedGraphStopReason::ScanLimit, 2),
    ] {
        let mut limited = q.clone();
        match field {
            "depth" => limited.max_depth = 1,
            "nodes" => limited.node_limit = 1,
            "edges" => limited.edge_limit = 1,
            _ => limited.scan_limit = 1,
        }
        let g = db.read_memory_typed_graph("scope", &limited).unwrap();
        assert!(!g.coverage.complete, "{field}");
        assert!(
            g.coverage.stop_reasons.contains(&reason),
            "{field}: {:?}",
            g.coverage.stop_reasons
        );
        assert_eq!(g.nodes.len(), nodes);
        assert!(g.coverage.records_examined <= limited.node_limit);
        assert!(g.edges.len() <= limited.edge_limit);
        assert!(g.coverage.adjacency_entries_examined <= limited.scan_limit);
        let included: BTreeMap<_, _> = g
            .nodes
            .iter()
            .map(|n| (n.record.record_id, n.record.revision))
            .collect();
        for e in &g.edges {
            assert_eq!(included[&e.input.source.record_id], e.input.source.revision);
            assert_eq!(included[&e.input.target.record_id], e.input.target.revision);
        }
    }
    q.root_record_ids = vec![Uuid::new_v4(), a.record_id, b.record_id];
    q.node_limit = 2;
    let g = db.read_memory_typed_graph("scope", &q).unwrap();
    assert_eq!(
        g.roots.iter().map(|r| r.status).collect::<Vec<_>>(),
        vec![
            MemoryGraphRootStatus::Unavailable,
            MemoryGraphRootStatus::Included,
            MemoryGraphRootStatus::NotExamined
        ]
    );
    let mut zero = query(vec![a.record_id, b.record_id, c.record_id]);
    zero.max_depth = 0;
    let g = db.read_memory_typed_graph("scope", &zero).unwrap();
    assert!(g.coverage.complete);
    assert_eq!(g.edges.len(), 2);
    assert!(g.nodes.iter().all(|n| n.depth == 0));
    // An exact scan budget can still finish when no further scoped entry exists.
    q = query(vec![a.record_id]);
    q.direction = TypedGraphDirection::Outgoing;
    q.scan_limit = 2;
    let g = db.read_memory_typed_graph("scope", &q).unwrap();
    assert!(g.coverage.complete);
    assert_eq!(g.coverage.adjacency_entries_examined, 2);
}

#[test]
fn typed_graph_filters_stale_endpoints_without_spending_valid_edge_budget() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let a = create(&db, "scope", "a");
    for i in 0..4 {
        let b = create(&db, "scope", &format!("deleted-{i}"));
        link(
            &db,
            "scope",
            &a,
            &b,
            MemoryRelationKind::Supports,
            &format!("ab-{i}"),
        );
        db.apply_memory_command(
            "scope",
            &actor(),
            &MemoryCommand {
                contract_version: 1,
                idempotency_key: format!("delete-{i}"),
                operation: MemoryOperation::Delete {
                    record_id: b.record_id,
                    expected_revision: 1,
                },
            },
        )
        .unwrap();
    }
    let live = create(&db, "scope", "live");
    let valid = link(
        &db,
        "scope",
        &a,
        &live,
        MemoryRelationKind::Supports,
        "valid",
    );
    let mut q = query(vec![a.record_id]);
    q.direction = TypedGraphDirection::Outgoing;
    q.edge_limit = 1;
    q.scan_limit = 5;
    let g = db.read_memory_typed_graph("scope", &q).unwrap();
    assert!(g.coverage.complete);
    assert_eq!(g.edges[0].relation_id, valid.relation_id);
    assert_eq!(g.nodes.len(), 2);
    assert_eq!(g.coverage.records_examined, 6);
    assert_eq!(g.coverage.adjacency_entries_examined, 5);
    q.scan_limit = 4;
    let limited = db.read_memory_typed_graph("scope", &q).unwrap();
    assert!(!limited.coverage.complete);
    assert!(
        limited
            .coverage
            .stop_reasons
            .contains(&TypedGraphStopReason::ScanLimit)
    );
}

#[test]
fn typed_graph_invalid_queries_fail_before_any_read() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let id = Uuid::new_v4();
    for case in 0..10 {
        let mut q = query(vec![id]);
        match case {
            0 => q.root_record_ids.clear(),
            1 => q.root_record_ids.push(id),
            2 => q.root_record_ids = (0..17).map(|_| Uuid::new_v4()).collect(),
            3 => q.relation_kinds.clear(),
            4 => q.relation_kinds.push(q.relation_kinds[0]),
            5 => q.max_depth = 9,
            6 => q.node_limit = 0,
            7 => q.node_limit = 257,
            8 => q.edge_limit = 1025,
            _ => q.scan_limit = 4097,
        }
        assert!(
            matches!(
                db.read_memory_typed_graph("scope", &q),
                Err(Error::ValidationError(_))
            ),
            "case {case}"
        );
    }
}

#[test]
fn typed_graph_root_and_dependency_bytes_fail_without_partial_success() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let large = db
        .apply_memory_command(
            "scope",
            &actor(),
            &MemoryCommand {
                contract_version: 1,
                idempotency_key: "large".into(),
                operation: MemoryOperation::Create {
                    record: input(&"x".repeat(MAX_MEMORY_READ_BYTES)),
                },
            },
        )
        .unwrap();
    assert!(matches!(
        db.read_memory_typed_graph("scope", &query(vec![large.record_id])),
        Err(Error::ValidationError(_))
    ));
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
                        evidence_ref: "trace://fixture".into(),
                    },
                },
            },
        )
        .unwrap();
    let root = create(&db, "scope", "root");
    link(
        &db,
        "scope",
        &root,
        &derived,
        MemoryRelationKind::Supports,
        "derived-link",
    );
    db.apply_memory_command(
        "scope",
        &actor(),
        &MemoryCommand {
            contract_version: 1,
            idempotency_key: "large-source".into(),
            operation: MemoryOperation::Update {
                record_id: source.record_id,
                expected_revision: 1,
                record: input(&"x".repeat(MAX_DEPENDENCY_BYTES)),
            },
        },
    )
    .unwrap();
    assert!(matches!(
        db.read_memory_typed_graph("scope", &query(vec![root.record_id])),
        Err(Error::ValidationError(_))
    ));
}
