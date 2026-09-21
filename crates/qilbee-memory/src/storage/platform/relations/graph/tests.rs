use super::super::super::semantic_tests::{actor, create, input, open};
use super::*;
use tempfile::TempDir;
#[path = "tests_bounds.rs"]
mod bounds;
#[path = "tests_integrity.rs"]
mod integrity;
#[path = "tests_semantics.rs"]
mod semantics;
fn query(ids: Vec<Uuid>) -> TypedMemoryGraphQuery {
    serde_json::from_value(serde_json::json!({"root_record_ids":ids,"max_depth":8})).unwrap()
}
fn link(
    db: &RocksDbMemoryStorage,
    scope: &str,
    a: &CommandReceipt,
    b: &CommandReceipt,
    kind: MemoryRelationKind,
    key: &str,
) -> MemoryRelationReceipt {
    db.apply_memory_relation_command(
        scope,
        &actor(),
        &MemoryRelationCommand {
            contract_version: 1,
            idempotency_key: key.into(),
            operation: MemoryRelationOperation::Assert {
                relation: MemoryRelationInput {
                    source: MemorySourceRef {
                        record_id: a.record_id,
                        revision: a.revision,
                    },
                    target: MemorySourceRef {
                        record_id: b.record_id,
                        revision: b.revision,
                    },
                    kind,
                    provenance: RelationProvenance {
                        origin: RelationOrigin::ToolObservation,
                        method: "fixture".into(),
                        method_revision: "v1".into(),
                        evidence_ref: "trace://graph".into(),
                        model: None,
                    },
                    valid_from_millis: None,
                    valid_until_millis: None,
                },
            },
        },
    )
    .unwrap()
}
#[test]
fn typed_graph_directions_cycles_parallel_claims_and_kind_filters_are_deterministic() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let a = create(&db, "scope", "a");
    let b = create(&db, "scope", "b");
    let c = create(&db, "scope", "c");
    let ab = link(&db, "scope", &a, &b, MemoryRelationKind::Supports, "ab");
    let bc = link(&db, "scope", &b, &c, MemoryRelationKind::Contradicts, "bc");
    let ca = link(
        &db,
        "scope",
        &c,
        &a,
        MemoryRelationKind::SemanticRelated,
        "ca",
    );
    let parallel = link(
        &db,
        "scope",
        &a,
        &b,
        MemoryRelationKind::Supports,
        "duplicate-assertion",
    );
    let graph = db
        .read_memory_typed_graph("scope", &query(vec![a.record_id]))
        .unwrap();
    assert_eq!(graph.nodes.len(), 3);
    assert_eq!(graph.edges.len(), 4);
    assert!(graph.coverage.complete);
    assert_eq!(graph.coverage.adjacency_entries_examined, 8);
    assert_eq!(graph.coverage.relations_examined, 4);
    assert_eq!(graph.coverage.records_examined, 3);
    assert_eq!(
        graph
            .edges
            .iter()
            .map(|e| e.relation_id)
            .collect::<BTreeSet<_>>(),
        [
            ab.relation_id,
            bc.relation_id,
            ca.relation_id,
            parallel.relation_id
        ]
        .into()
    );
    let repeated = db
        .read_memory_typed_graph("scope", &query(vec![a.record_id]))
        .unwrap();
    assert_eq!(
        serde_json::to_value(&graph.nodes).unwrap(),
        serde_json::to_value(&repeated.nodes).unwrap()
    );
    assert_eq!(graph.edges, repeated.edges);
    for (direction, first_neighbor) in [
        (TypedGraphDirection::Outgoing, b.record_id),
        (TypedGraphDirection::Incoming, c.record_id),
    ] {
        let mut q = query(vec![a.record_id]);
        q.direction = direction;
        let g = db.read_memory_typed_graph("scope", &q).unwrap();
        assert!(g.coverage.complete);
        assert_eq!(g.nodes[1].record.record_id, first_neighbor);
        assert_eq!(g.edges.len(), 4);
        assert_eq!(g.coverage.adjacency_entries_examined, 4);
    }
    let mut q = query(vec![b.record_id]);
    q.relation_kinds = vec![MemoryRelationKind::Supports];
    q.direction = TypedGraphDirection::Outgoing;
    let g = db.read_memory_typed_graph("scope", &q).unwrap();
    assert_eq!(g.nodes.len(), 1);
    assert!(g.edges.is_empty());
    assert!(g.coverage.complete);
    q.direction = TypedGraphDirection::Incoming;
    let g = db.read_memory_typed_graph("scope", &q).unwrap();
    assert_eq!(g.nodes.len(), 2);
    assert_eq!(g.edges.len(), 2);
    assert!(g.coverage.complete);
    assert_eq!(g.edges[0].input.source.record_id, a.record_id);
    assert_eq!(g.edges[0].input.target.record_id, b.record_id);
}
