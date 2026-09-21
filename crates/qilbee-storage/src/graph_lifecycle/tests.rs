use super::*;
use crate::StorageOptions;
use tempfile::TempDir;

fn open(directory: &TempDir) -> StorageEngine {
    StorageEngine::open(StorageOptions::for_testing(directory.path())).unwrap()
}
fn node(id: u64) -> Node {
    Node::with_labels(NodeId::from_internal(id), ["Record"])
}

#[test]
fn legacy_catalog_migration_keeps_ids_and_discovers_highest_retained_entities() {
    let directory = TempDir::new().unwrap();
    let db = open(&directory);
    let graph = GraphId::from_name("legacy");
    // Construct the pre-allocator layout directly, not through the upgraded writer.
    let first = node(51);
    let second = node(90);
    for value in [&first, &second] {
        db.db
            .put_cf(
                db.cf(cf::NODES).unwrap(),
                KeyBuilder::node(graph, value.id),
                bincode::serialize(value).unwrap(),
            )
            .unwrap();
    }
    let relationship = Relationship::new(
        RelationshipId::from_internal(200),
        "EDGE",
        first.id,
        second.id,
    );
    db.db
        .put_cf(
            db.cf(cf::RELATIONSHIPS).unwrap(),
            KeyBuilder::relationship(graph, relationship.id),
            bincode::serialize(&relationship).unwrap(),
        )
        .unwrap();
    db.put_meta(CATALOG, br#"["legacy"]"#).unwrap();
    let identity = db.named_graphs().unwrap().pop().unwrap();
    assert_eq!(identity.id(), graph);
    let added = db
        .create_graph_node(&identity, node(0), |_| Ok(()))
        .unwrap();
    assert_eq!(added.id.as_internal(), 91);
    let next = db
        .create_graph_relationship(&identity, relationship)
        .unwrap();
    assert_eq!(next.id.as_internal(), 201);
    assert_eq!(db.get_node(graph, first.id).unwrap().unwrap().id, first.id);
    assert_eq!(
        decode::<Catalog>(&db.get_meta(CATALOG).unwrap().unwrap())
            .unwrap()
            .schema_version,
        2
    );
}

#[test]
fn invalid_legacy_catalog_never_publishes_partial_migration() {
    let directory = TempDir::new().unwrap();
    let db = open(&directory);
    for raw in [
        br#"["valid","valid"]"#.as_slice(),
        br#"["valid",""]"#,
        br#"{"schema_version":77,"graphs":{}}"#,
    ] {
        db.put_meta(CATALOG, raw).unwrap();
        assert!(db.named_graphs().is_err());
        assert_eq!(db.get_meta(CATALOG).unwrap().unwrap(), raw);
        assert!(
            db.scan_meta_keys("graph/v2/identity/", None, 100)
                .unwrap()
                .0
                .is_empty()
        );
    }
}

#[test]
fn id_reservation_excludes_retired_generations_and_legacy_orphan_indexes() {
    let directory = TempDir::new().unwrap();
    let db = open(&directory);
    let identity = db.open_named_graph("retired", 10, true).unwrap();
    db.retire_named_graph("retired").unwrap();
    assert!(!db.graph_id_available(identity.id()).unwrap());
    let orphan = GraphId::from_internal(500);
    db.db
        .put_cf(
            db.cf(cf::ADJACENCY_OUT).unwrap(),
            KeyBuilder::adjacency_out(
                orphan,
                NodeId::from_internal(1),
                "ORPHAN",
                RelationshipId::from_internal(1),
            ),
            [],
        )
        .unwrap();
    assert!(!db.graph_id_available(orphan).unwrap());
    let allocated = GraphId::from_internal(600);
    db.put_node(allocated, &node(2)).unwrap();
    db.delete_node(allocated, NodeId::from_internal(2)).unwrap();
    assert!(!db.graph_id_available(allocated).unwrap());
}

#[test]
fn corrupt_missing_or_regressed_allocators_cannot_overwrite_entities() {
    let directory = TempDir::new().unwrap();
    let db = open(&directory);
    let identity = db.open_named_graph("protected", 10, true).unwrap();
    let first = db
        .create_graph_node(&identity, node(0), |_| Ok(()))
        .unwrap();
    let key = allocation::water_key(identity.id());
    let original = db.get_meta(&key).unwrap().unwrap();
    for bytes in [b"corrupt".to_vec(), encode(&serde_json::json!({"schema_version":1,"graph_id":identity.id(),"node":0,"relationship":0})).unwrap()] {
        db.put_meta(&key, &bytes).unwrap();
        assert!(db.create_graph_node(&identity, node(0), |_| Ok(())).is_err());
        assert_eq!(db.get_node(identity.id(), first.id).unwrap().unwrap().id, first.id);
    }
    db.db
        .delete_cf(db.cf(cf::META).unwrap(), KeyBuilder::meta(&key))
        .unwrap();
    assert!(
        db.create_graph_node(&identity, node(0), |_| Ok(()))
            .is_err()
    );
    db.put_meta(&key, &original).unwrap();
    assert_eq!(
        db.create_graph_node(&identity, node(0), |_| Ok(()))
            .unwrap()
            .id
            .as_internal(),
        2
    );
}

#[test]
fn failed_batch_does_not_advance_allocator_or_publish_records() {
    let directory = TempDir::new().unwrap();
    let db = open(&directory);
    let identity = db.open_named_graph("atomic", 10, true).unwrap();
    let before = db.get_meta(&allocation::water_key(identity.id())).unwrap();
    db.db
        .put_cf(
            db.cf(cf::NODES).unwrap(),
            KeyBuilder::node(identity.id(), NodeId::from_internal(2)),
            b"invalid",
        )
        .unwrap();
    assert!(
        db.apply_graph_operations(
            &identity,
            &[
                TransactionOperation::PutNode(node(1)),
                TransactionOperation::DeleteNode(NodeId::from_internal(2))
            ]
        )
        .is_err()
    );
    assert!(
        db.get_node(identity.id(), NodeId::from_internal(1))
            .unwrap()
            .is_none()
    );
    assert_eq!(
        before,
        db.get_meta(&allocation::water_key(identity.id())).unwrap()
    );
}

#[test]
fn allocator_exhaustion_and_explicit_transaction_ids_never_wrap() {
    let directory = TempDir::new().unwrap();
    let db = open(&directory);
    let identity = db.open_named_graph("explicit", 10, true).unwrap();
    db.apply_graph_operations(
        &identity,
        &[
            TransactionOperation::PutNode(node(700)),
            TransactionOperation::DeleteNode(NodeId::from_internal(700)),
        ],
    )
    .unwrap();
    assert_eq!(
        db.create_graph_node(&identity, node(0), |_| Ok(()))
            .unwrap()
            .id
            .as_internal(),
        701
    );
    db.put_node(identity.id(), &node(u64::MAX)).unwrap();
    db.delete_node(identity.id(), NodeId::from_internal(u64::MAX))
        .unwrap();
    assert!(
        db.create_graph_node(&identity, node(0), |_| Ok(()))
            .is_err()
    );
    let rel = Relationship::new(
        RelationshipId::from_internal(u64::MAX),
        "LAST",
        NodeId::from_internal(701),
        NodeId::from_internal(701),
    );
    db.put_relationship(identity.id(), &rel).unwrap();
    db.delete_relationship(identity.id(), rel.id).unwrap();
    assert!(db.create_graph_relationship(&identity, rel).is_err());
    assert_eq!(db.get_all_nodes(identity.id()).unwrap().len(), 1);
}

#[test]
fn malformed_generation_or_missing_catalog_fails_closed() {
    let directory = TempDir::new().unwrap();
    let db = open(&directory);
    let identity = db.open_named_graph("generation", 10, true).unwrap();
    let catalog = db.get_meta(CATALOG).unwrap().unwrap();
    db.db
        .delete_cf(db.cf(cf::META).unwrap(), KeyBuilder::meta(CATALOG))
        .unwrap();
    assert!(db.named_graphs().is_err());
    db.put_meta(CATALOG, b"[]").unwrap();
    assert!(db.named_graphs().is_err());
    db.put_meta(CATALOG, &catalog).unwrap();
    let mut wrong = identity.clone();
    wrong.name = "different".into();
    db.put_meta(&identity_key(identity.id()), &encode(&wrong).unwrap())
        .unwrap();
    assert!(db.validate_graph_identity(&identity).is_err());
    assert!(db.named_graphs().is_err());
    assert!(
        db.create_graph_node(&identity, node(0), |_| Ok(()))
            .is_err()
    );
}
