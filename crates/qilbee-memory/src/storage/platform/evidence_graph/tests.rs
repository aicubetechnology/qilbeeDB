use super::*;
use crate::storage::platform::semantic_tests::{actor, create, input, open};
use tempfile::TempDir;

fn query(ids: &[Uuid]) -> MemoryGraphQuery {
    MemoryGraphQuery {
        root_record_ids: ids.to_vec(),
        max_depth: 8,
        node_limit: 256,
    }
}
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
                    method: "fixture-extraction".into(),
                    method_revision: "v1".into(),
                    evidence_ref: "trace://fixture".into(),
                },
            },
        },
    )
    .unwrap()
}
fn diamond(db: &RocksDbMemoryStorage) -> (CommandReceipt, Vec<CommandReceipt>) {
    let leaf = create(db, "scope", "leaf");
    let a = derive(db, "a", std::slice::from_ref(&leaf));
    let b = derive(db, "b", std::slice::from_ref(&leaf));
    let root = derive(db, "root", &[b.clone(), a.clone()]);
    let mut middle = vec![a, b];
    middle.sort_by_key(|s| s.record_id);
    middle.push(leaf);
    (root, middle)
}
fn assert_graph_links(graph: &MemoryEvidenceGraph) {
    let nodes: BTreeMap<_, _> = graph
        .nodes
        .iter()
        .map(|n| (n.record.record_id, n.record.revision))
        .collect();
    assert_eq!(nodes.len(), graph.nodes.len());
    for edge in &graph.edges {
        assert_eq!(nodes[&edge.source.record_id], edge.source.revision);
        assert_eq!(nodes[&edge.target.record_id], edge.target.revision);
        assert_eq!(edge.relation, MemoryGraphRelation::DerivedFrom);
    }
}

#[test]
fn evidence_graph_deduplicates_diamonds_and_preserves_order_provenance_and_reopen() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let (root, rest) = diamond(&db);
    let q = query(&[root.record_id]);
    let first = db.read_memory_graph("scope", &q).unwrap();
    assert_eq!(
        first
            .nodes
            .iter()
            .map(|n| n.record.record_id)
            .collect::<Vec<_>>(),
        [
            vec![root.record_id],
            rest.iter().map(|r| r.record_id).collect()
        ]
        .concat()
    );
    assert_eq!(
        first.nodes.iter().map(|n| n.depth).collect::<Vec<_>>(),
        [0, 1, 1, 2]
    );
    assert_eq!(first.edges.len(), 4);
    assert_eq!(first.coverage.records_examined, 4);
    assert_eq!(first.coverage.dependency_work.records_examined, 3);
    assert!(first.coverage.complete);
    assert_graph_links(&first);
    assert_eq!(
        first.nodes[0]
            .record
            .derivation
            .as_ref()
            .unwrap()
            .method_revision,
        "v1"
    );
    drop(db);
    let db = open(dir.path());
    let next = db.read_memory_graph("scope", &q).unwrap();
    assert_eq!(
        serde_json::to_value(first.nodes).unwrap(),
        serde_json::to_value(next.nodes).unwrap()
    );
    assert_eq!(first.edges, next.edges);
    let foreign = db.read_memory_graph("other-scope", &q).unwrap();
    assert!(foreign.nodes.is_empty());
    assert_eq!(foreign.roots[0].status, MemoryGraphRootStatus::Unavailable);
    assert!(foreign.coverage.complete);
}

#[test]
fn evidence_graph_reports_depth_and_node_cuts_without_dangling_edges() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let (root, rest) = diamond(&db);
    let mut q = query(&[root.record_id]);
    q.max_depth = 0;
    let shallow = db.read_memory_graph("scope", &q).unwrap();
    assert_eq!(shallow.nodes.len(), 1);
    assert_eq!(shallow.coverage.dependency_work.records_examined, 3);
    assert_eq!(
        shallow.coverage.stop_reasons,
        [MemoryGraphStopReason::DepthLimit]
    );
    assert!(!shallow.coverage.complete);
    assert!(shallow.edges.is_empty());
    q.max_depth = 8;
    q.node_limit = 2;
    let cut = db.read_memory_graph("scope", &q).unwrap();
    assert_eq!(cut.nodes.len(), 2);
    assert_eq!(cut.edges.len(), 1);
    assert_eq!(
        cut.coverage.stop_reasons,
        [MemoryGraphStopReason::NodeLimit]
    );
    assert_graph_links(&cut);
    let missing = Uuid::new_v4();
    q.root_record_ids = vec![missing, root.record_id];
    q.node_limit = 1;
    let cut = db.read_memory_graph("scope", &q).unwrap();
    assert_eq!(cut.roots[0].status, MemoryGraphRootStatus::Unavailable);
    assert_eq!(cut.roots[1].status, MemoryGraphRootStatus::NotExamined);
    assert_eq!(cut.coverage.records_examined, 1);
    assert!(!cut.coverage.complete);
    // Independently requested roots can close a depth-zero graph completely.
    q.root_record_ids = [
        vec![root.record_id],
        rest.iter().map(|r| r.record_id).collect(),
    ]
    .concat();
    q.max_depth = 0;
    q.node_limit = 4;
    let closed = db.read_memory_graph("scope", &q).unwrap();
    assert!(closed.coverage.complete);
    assert_eq!(closed.edges.len(), 4);
    assert!(closed.nodes.iter().all(|n| n.depth == 0));
}

#[test]
fn evidence_graph_snapshot_prevents_mixed_revisions_and_checks_hidden_ancestors() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let (root, rest) = diamond(&db);
    let leaf = rest.last().unwrap();
    let before = db.memory_snapshot();
    db.apply_memory_command(
        "scope",
        &actor(),
        &MemoryCommand {
            contract_version: 1,
            idempotency_key: "edit".into(),
            operation: MemoryOperation::Update {
                record_id: leaf.record_id,
                expected_revision: 1,
                record: input("corrected"),
            },
        },
    )
    .unwrap();
    let old = before
        .evidence_graph("scope", &query(&[root.record_id]))
        .unwrap();
    assert_eq!(old.nodes.len(), 4);
    assert!(old.nodes.iter().all(|n| n.record.revision == 1));
    let mut q = query(&[root.record_id, leaf.record_id]);
    q.max_depth = 0;
    let current = db.read_memory_graph("scope", &q).unwrap();
    assert_eq!(current.roots[0].status, MemoryGraphRootStatus::Unavailable);
    assert_eq!(current.nodes.len(), 1);
    assert_eq!(current.nodes[0].record.revision, 2);
    assert!(current.coverage.complete);
}

#[test]
fn evidence_graph_rejection_deletion_and_expiry_never_reuse_derived_context() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    for action in ["reject", "delete", "expire"] {
        let mut record = input(action);
        let expiry = chrono::Utc::now().timestamp_millis() + 60_000;
        if action == "expire" {
            record.valid_until_millis = Some(expiry);
        }
        let source = db
            .apply_memory_command(
                "scope",
                &actor(),
                &MemoryCommand {
                    contract_version: 1,
                    idempotency_key: format!("source-{action}"),
                    operation: MemoryOperation::Create { record },
                },
            )
            .unwrap();
        let root = derive(
            &db,
            &format!("derived-{action}"),
            std::slice::from_ref(&source),
        );
        if action == "reject" {
            db.review_memory_record(
                "scope",
                &actor(),
                &MemoryReviewCommand {
                    contract_version: 1,
                    idempotency_key: "reject".into(),
                    record_id: source.record_id,
                    expected_revision: 1,
                    disposition: MemoryReviewDisposition::Rejected,
                    evidence_ref: "trace://review".into(),
                },
            )
            .unwrap();
        } else if action == "delete" {
            db.apply_memory_command(
                "scope",
                &actor(),
                &MemoryCommand {
                    contract_version: 1,
                    idempotency_key: "delete".into(),
                    operation: MemoryOperation::Delete {
                        record_id: source.record_id,
                        expected_revision: 1,
                    },
                },
            )
            .unwrap();
        }
        let mut snap = db.memory_snapshot();
        if action == "expire" {
            snap.now = expiry;
        }
        let mut q = query(&[root.record_id, source.record_id]);
        q.max_depth = 0;
        let graph = snap.evidence_graph("scope", &q).unwrap();
        assert!(graph.nodes.is_empty(), "{action}");
        assert!(graph.edges.is_empty());
        assert!(
            graph
                .roots
                .iter()
                .all(|r| r.status == MemoryGraphRootStatus::Unavailable)
        );
    }
}

#[test]
fn evidence_graph_rejects_malformed_queries_and_fails_entirely_on_corrupt_dependencies() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    for ids in [
        vec![],
        vec![Uuid::nil(); 2],
        (0..17).map(|_| Uuid::new_v4()).collect(),
    ] {
        assert!(db.read_memory_graph("scope", &query(&ids)).is_err());
    }
    for (depth, nodes) in [(9, 1), (0, 0), (0, 257)] {
        let mut q = query(&[Uuid::nil()]);
        q.max_depth = depth;
        q.node_limit = nodes;
        assert!(db.read_memory_graph("scope", &q).is_err());
    }
    let (root, rest) = diamond(&db);
    let live = create(&db, "scope", "live");
    db.db
        .delete_cf(
            db.cf(crate::storage::cf::EPISODE_INDEX).unwrap(),
            record_key(0x11, "scope", rest.last().unwrap().record_id),
        )
        .unwrap();
    let mut q = query(&[live.record_id, root.record_id]);
    q.max_depth = 0;
    assert!(matches!(
        db.read_memory_graph("scope", &q),
        Err(Error::DataCorruption(_))
    ));
}

#[test]
fn evidence_graph_bytes_are_bounded_without_returning_a_partial_success() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let mut ids = vec![];
    for n in 0..2 {
        let receipt = db
            .apply_memory_command(
                "scope",
                &actor(),
                &MemoryCommand {
                    contract_version: 1,
                    idempotency_key: format!("large-{n}"),
                    operation: MemoryOperation::Create {
                        record: input(&"x".repeat(4 * 1024 * 1024)),
                    },
                },
            )
            .unwrap();
        ids.push(receipt.record_id);
    }
    let mut q = query(&ids);
    q.node_limit = 1;
    let cut = db.read_memory_graph("scope", &q).unwrap();
    assert_eq!(cut.nodes.len(), 1);
    assert!(!cut.coverage.complete);
    q.node_limit = 2;
    assert!(matches!(
        db.read_memory_graph("scope", &q),
        Err(Error::ValidationError(_))
    ));
}

#[test]
fn evidence_graph_aggregate_dependency_exhaustion_never_reports_complete() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let mut roots = Vec::new();
    for n in 0..5 {
        let source = db
            .apply_memory_command(
                "scope",
                &actor(),
                &MemoryCommand {
                    contract_version: 1,
                    idempotency_key: format!("large-source-{n}"),
                    operation: MemoryOperation::Create {
                        record: input(&"x".repeat(4 * 1024 * 1024)),
                    },
                },
            )
            .unwrap();
        roots.push(derive(&db, &format!("root-{n}"), &[source]).record_id);
    }
    let mut q = query(&roots);
    q.max_depth = 0;
    assert!(matches!(
        db.read_memory_graph("scope", &q),
        Err(Error::ValidationError(_))
    ));
}
