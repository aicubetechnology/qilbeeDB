use super::*;
use qilbee_core::Direction;
use tempfile::TempDir;

#[test]
fn new_graph_nodes_must_not_replace_records_after_reopen() {
    let directory = TempDir::new().unwrap();
    let original_id = {
        let db = Database::open(directory.path()).unwrap();
        let graph = db.create_graph("durable").unwrap();
        let original = graph.create_node(["Original"]).unwrap();
        db.flush().unwrap();
        original.id
    };
    let db = Database::open(directory.path()).unwrap();
    let graph = db.graph("durable").unwrap();
    let added = graph.create_node(["AddedAfterRestart"]).unwrap();
    assert_ne!(
        added.id, original_id,
        "A restarted graph reused a stored node ID"
    );
    assert_eq!(graph.get_all_nodes().unwrap().len(), 2);
    assert!(
        graph
            .get_node(original_id)
            .unwrap()
            .unwrap()
            .has_label(&qilbee_core::Label::new("Original"))
    );
}

#[test]
fn new_graph_relationships_must_not_replace_edges_after_reopen() {
    let directory = TempDir::new().unwrap();
    let (source, target, edge) = {
        let db = Database::open(directory.path()).unwrap();
        let graph = db.create_graph("durable").unwrap();
        let a = graph.create_node(["A"]).unwrap();
        let b = graph.create_node(["B"]).unwrap();
        let edge = graph.create_relationship(a.id, "ORIGINAL", b.id).unwrap();
        db.flush().unwrap();
        (a.id, b.id, edge.id)
    };
    let db = Database::open(directory.path()).unwrap();
    let graph = db.graph("durable").unwrap();
    let added = graph.create_relationship(source, "ADDED", target).unwrap();
    assert_ne!(
        added.id, edge,
        "A restarted graph reused a stored relationship ID"
    );
    assert_eq!(
        graph
            .get_relationships(source, Direction::Outgoing)
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn graph_recreation_must_not_reveal_deleted_graph_data() {
    let directory = TempDir::new().unwrap();
    let db = Database::open(directory.path()).unwrap();
    let old = db.create_graph("reusable-name").unwrap();
    old.create_node(["OldData"]).unwrap();
    drop(old);
    assert!(db.delete_graph("reusable-name").unwrap());
    let replacement = db.create_graph("reusable-name").unwrap();
    assert!(
        replacement.get_all_nodes().unwrap().is_empty(),
        "Deleting and recreating a graph restored its old records"
    );
}

#[test]
fn retired_handles_and_transactions_cannot_access_or_mutate_a_replacement() {
    use qilbee_core::{EntityId, Node, NodeId};
    let directory = TempDir::new().unwrap();
    let db = Database::open(directory.path()).unwrap();
    let old = db.graph("generation").unwrap();
    let first = old.create_node(["Old"]).unwrap();
    let rel = old.create_relationship(first.id, "SELF", first.id).unwrap();
    let mut cached = old.begin_transaction();
    assert!(cached.get_node(first.id).unwrap().is_some());
    let mut pending = old.begin_transaction();
    pending
        .put_node(Node::with_labels(NodeId::from_internal(80), ["Pending"]))
        .unwrap();
    assert!(db.delete_graph("generation").unwrap());
    let current = db.graph("generation").unwrap();
    assert_ne!(old.id(), current.id());
    assert!(old.get_node(first.id).is_err());
    assert!(old.get_relationship(rel.id).is_err());
    assert!(old.get_all_nodes().is_err());
    assert!(old.find_nodes_by_label("Old").is_err());
    assert!(old.get_relationships(first.id, Direction::Both).is_err());
    assert!(old.create_node(["Stale"]).is_err());
    assert!(old.update_node(&first).is_err());
    assert!(old.delete_node(first.id).is_err());
    assert!(old.detach_delete_node(first.id).is_err());
    assert!(
        old.create_relationship(first.id, "STALE", first.id)
            .is_err()
    );
    assert!(old.update_relationship(&rel).is_err());
    assert!(old.delete_relationship(rel.id).is_err());
    assert!(cached.get_node(first.id).is_err());
    cached.rollback().unwrap();
    assert!(pending.commit().is_err());
    assert!(old.begin_transaction().commit().is_err());
    assert!(old.storage().put_node(old.id(), &first).is_err());
    assert!(current.get_all_nodes().unwrap().is_empty());
    // Retirement preserves storage history; trusted physical reads are not an ACL.
    assert!(db.storage().get_node(old.id(), first.id).unwrap().is_some());
}

#[test]
fn deleted_highest_ids_are_not_reused_after_restart() {
    use qilbee_core::EntityId;
    let directory = TempDir::new().unwrap();
    let (node_high, edge_high) = {
        let db = Database::open(directory.path()).unwrap();
        let graph = db.graph("monotonic").unwrap();
        let a = graph.create_node(["A"]).unwrap();
        let b = graph.create_node(["B"]).unwrap();
        let rel = graph.create_relationship(a.id, "LAST", b.id).unwrap();
        graph.detach_delete_node(b.id).unwrap();
        (b.id.as_internal(), rel.id.as_internal())
    };
    let db = Database::open(directory.path()).unwrap();
    let graph = db.graph("monotonic").unwrap();
    let node = graph.create_node(["AfterRestart"]).unwrap();
    let rel = graph
        .create_relationship(node.id, "AFTER", node.id)
        .unwrap();
    assert!(node.id.as_internal() > node_high);
    assert!(rel.id.as_internal() > edge_high);
}

#[test]
fn concurrent_database_wrappers_share_catalog_and_entity_allocation() {
    use std::sync::{Arc, Barrier};
    let directory = TempDir::new().unwrap();
    let db = Database::open(directory.path()).unwrap();
    let gate = Arc::new(Barrier::new(12));
    let threads: Vec<_> = (0..12)
        .map(|index| {
            let independent = Database::new(db.storage().clone());
            let gate = gate.clone();
            std::thread::spawn(move || {
                gate.wait();
                let shared = independent.graph("shared").unwrap();
                let a = shared.create_node(["Concurrent"]).unwrap();
                let rel = shared.create_relationship(a.id, "SELF", a.id).unwrap();
                independent.create_graph(&format!("graph-{index}")).unwrap();
                (shared.id(), a.id, rel.id)
            })
        })
        .collect();
    let identities: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    assert!(identities.iter().all(|v| v.0 == identities[0].0));
    assert_eq!(
        identities
            .iter()
            .map(|v| v.1)
            .collect::<std::collections::HashSet<_>>()
            .len(),
        12
    );
    assert_eq!(
        identities
            .iter()
            .map(|v| v.2)
            .collect::<std::collections::HashSet<_>>()
            .len(),
        12
    );
    assert_eq!(
        db.graph("shared").unwrap().get_all_nodes().unwrap().len(),
        12
    );
    let names = db.list_graphs().unwrap();
    assert_eq!(names.len(), 13);
    drop(db);
    let reopened = Database::open(directory.path()).unwrap();
    assert_eq!(reopened.list_graphs().unwrap(), names);
}

#[test]
fn concurrent_create_only_and_catalog_capacity_have_no_extra_winners() {
    use std::sync::{Arc, Barrier};
    let directory = TempDir::new().unwrap();
    let db = Database::open(directory.path()).unwrap();
    for same_name in [true, false] {
        let gate = Arc::new(Barrier::new(12));
        let threads: Vec<_> = (0..12)
            .map(|index| {
                let mut db = Database::new(db.storage().clone());
                db.config.max_graphs = 5;
                let gate = gate.clone();
                std::thread::spawn(move || {
                    gate.wait();
                    db.create_graph(&if same_name {
                        "same-name".into()
                    } else {
                        format!("capacity-{index}")
                    })
                    .is_ok()
                })
            })
            .collect();
        let winners = threads
            .into_iter()
            .filter_map(|t| t.join().ok())
            .filter(|won| *won)
            .count();
        assert_eq!(winners, if same_name { 1 } else { 4 });
    }
    assert_eq!(db.graph_count().unwrap(), 5);
}

#[test]
fn same_handle_unique_checks_and_relationship_lifecycle_share_the_writer_lock() {
    use crate::schema::Constraint;
    use qilbee_core::Property;
    use std::sync::{Arc, Barrier};
    let directory = TempDir::new().unwrap();
    let db = Database::open(directory.path()).unwrap();
    let graph = db.graph("constraints").unwrap();
    graph
        .schema()
        .write()
        .unwrap()
        .add_constraint(Constraint::unique("email", "User", "email"));
    let gate = Arc::new(Barrier::new(8));
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let graph = graph.clone();
            let gate = gate.clone();
            std::thread::spawn(move || {
                let mut properties = Property::new();
                properties.set("email", "same@example.test");
                gate.wait();
                graph
                    .create_node_with_properties(["User"], properties)
                    .is_ok()
            })
        })
        .collect();
    assert_eq!(
        threads
            .into_iter()
            .map(|t| t.join().unwrap())
            .filter(|v| *v)
            .count(),
        1
    );
    for _ in 0..16 {
        let source = graph.create_node(["Source"]).unwrap();
        let target = graph.create_node(["Target"]).unwrap();
        let gate = Arc::new(Barrier::new(2));
        let cloned = graph.clone();
        let other_gate = gate.clone();
        let deletion = std::thread::spawn(move || {
            other_gate.wait();
            cloned.delete_node(source.id).is_ok()
        });
        gate.wait();
        let created = graph
            .create_relationship(source.id, "RACE", target.id)
            .is_ok();
        let deleted = deletion.join().unwrap();
        assert_ne!(
            created, deleted,
            "Only the endpoint-preserving outcome may succeed"
        );
    }
}
