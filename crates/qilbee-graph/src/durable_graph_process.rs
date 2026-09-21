//! Acknowledged managed graph state survives SIGKILL, including with raw WAL disabled.
use crate::Database;
use qilbee_core::{EntityId, GraphId, NodeId, RelationshipId};
use qilbee_storage::{StorageEngine, StorageOptions};
use serde_json::{Value, json};
use std::{
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn child(directory: &Path, stage: &str) -> Value {
    let marker = directory.join(format!("{stage}.json"));
    let mut process = Process(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "durable_graph_process::durable_graph_lifecycle_survives_process_kill",
                "--nocapture",
            ])
            .env("QILBEE_GRAPH_CRASH_ROOT", directory)
            .env("QILBEE_GRAPH_CRASH_STAGE", stage)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(20);
    while !marker.exists() {
        assert!(
            process.0.try_wait().unwrap().is_none(),
            "Child ended before acknowledgement"
        );
        assert!(
            Instant::now() < deadline,
            "Child did not acknowledge graph writes"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let receipt = serde_json::from_slice(&std::fs::read(marker).unwrap()).unwrap();
    process.0.kill().unwrap();
    process.0.wait().unwrap();
    receipt
}
#[test]
fn durable_graph_lifecycle_survives_process_kill() {
    if let Ok(root) = std::env::var("QILBEE_GRAPH_CRASH_ROOT") {
        let root = Path::new(&root);
        let stage = std::env::var("QILBEE_GRAPH_CRASH_STAGE").unwrap();
        let mut options = StorageOptions::for_testing(root.join("db"));
        options.enable_wal = false;
        options.sync_wal = false;
        let db = Database::new(StorageEngine::open(options).unwrap());
        let graph = db.graph("durable").unwrap();
        let receipt = if stage == "created" {
            let a = graph.create_node(["BeforeCrash"]).unwrap();
            let b = graph.create_node(["DeletedHighest"]).unwrap();
            let edge = graph.create_relationship(a.id, "EDGE", a.id).unwrap();
            let deleted = graph.create_relationship(a.id, "DELETED", b.id).unwrap();
            graph.detach_delete_node(b.id).unwrap();
            json!({"graph":graph.id().as_internal(),"node":a.id.as_internal(),"node_high":b.id.as_internal(),"edge":edge.id.as_internal(),"edge_high":deleted.id.as_internal()})
        } else {
            assert_eq!(stage, "retired");
            assert!(db.delete_graph("durable").unwrap());
            json!({"retired":graph.id().as_internal()})
        };
        let temp = root.join(format!("{stage}.tmp"));
        std::fs::write(&temp, serde_json::to_vec(&receipt).unwrap()).unwrap();
        std::fs::rename(temp, root.join(format!("{stage}.json"))).unwrap();
        loop {
            std::thread::park();
        }
    }
    let directory = tempfile::TempDir::new().unwrap();
    let first = child(directory.path(), "created");
    {
        let db = Database::open(directory.path().join("db")).unwrap();
        let graph = db.graph("durable").unwrap();
        assert_eq!(graph.id().as_internal(), first["graph"].as_u64().unwrap());
        assert!(
            graph
                .get_node(NodeId::from_internal(first["node"].as_u64().unwrap()))
                .unwrap()
                .is_some()
        );
        assert!(
            graph
                .get_relationship(RelationshipId::from_internal(
                    first["edge"].as_u64().unwrap()
                ))
                .unwrap()
                .is_some()
        );
        assert!(
            graph
                .get_node(NodeId::from_internal(first["node_high"].as_u64().unwrap()))
                .unwrap()
                .is_none()
        );
        let added = graph.create_node(["AfterCrash"]).unwrap();
        assert!(added.id.as_internal() > first["node_high"].as_u64().unwrap());
        let edge = graph
            .create_relationship(added.id, "AFTER", added.id)
            .unwrap();
        assert!(edge.id.as_internal() > first["edge_high"].as_u64().unwrap());
    }
    let retired = child(directory.path(), "retired");
    let db = Database::open(directory.path().join("db")).unwrap();
    assert!(!db.graph_exists("durable").unwrap());
    let current = db.graph("durable").unwrap();
    assert_ne!(
        current.id().as_internal(),
        retired["retired"].as_u64().unwrap()
    );
    assert!(current.get_all_nodes().unwrap().is_empty());
    assert!(
        db.storage()
            .get_node(
                GraphId::from_internal(first["graph"].as_u64().unwrap()),
                NodeId::from_internal(first["node"].as_u64().unwrap())
            )
            .unwrap()
            .is_some()
    );
}
