//! Local operator command: offline verification of a stopped data directory.
//!
//! `qilbeedb verify-store <data-directory>` opens the graph, agent-memory and
//! procedural-learning stores read-only, reports deterministic per-family
//! inventories, walks every memory journal chain and checks the learning
//! store's derived knowledge index against its authoritative ledgers. It never writes, never repairs, and refuses a
//! directory that another process holds open. A failure produces no report.
use qilbee_core::{Error, Result};
use qilbee_memory::RocksDbMemoryStorage;
use qilbee_memory::learning::LearningMemory;
use qilbee_storage::StorageEngine;
use qilbee_storage::verification::require_stopped;
use serde_json::{Value, json};
use std::path::Path;

/// Command word recognized on the process command line.
pub const COMMAND: &str = "verify-store";

/// Agent memory store directory inside the data directory.
pub const AGENT_MEMORY_DIRECTORY: &str = "agent-memory";

/// Procedural learning store directory inside the data directory.
pub const PROCEDURAL_LEARNING_DIRECTORY: &str = "procedural-learning";

/// Parse an explicit local verification command. Returns `Ok(None)` for other
/// command lines so the caller can continue its normal dispatch.
pub fn verify_store_command(args: &[String]) -> Result<Option<Value>> {
    if args.first().map(String::as_str) != Some(COMMAND) {
        return Ok(None);
    }
    if args.len() != 2 {
        return Err(Error::Configuration(
            "Usage: qilbeedb verify-store <data-directory>".into(),
        ));
    }
    verify_data_directory(Path::new(&args[1])).map(Some)
}

/// Verify every store of a stopped platform data directory.
pub fn verify_data_directory(data_directory: &Path) -> Result<Value> {
    let data_directory = data_directory.canonicalize().map_err(|error| {
        Error::Configuration(format!(
            "Data directory {} is unavailable: {error}",
            data_directory.display()
        ))
    })?;
    let memory_path = data_directory.join(AGENT_MEMORY_DIRECTORY);
    let learning_path = data_directory.join(PROCEDURAL_LEARNING_DIRECTORY);
    // Probe every store before opening any: a partially verified directory is
    // not a result, and a live server must be reported before any inventory.
    for path in [&data_directory, &memory_path, &learning_path] {
        require_stopped(path)?;
    }
    let graph = StorageEngine::open_read_only(&data_directory)?;
    let graph_families = graph.inventory()?;
    drop(graph);
    let memory = RocksDbMemoryStorage::open_read_only(&memory_path)?;
    let memory_families = memory.inventory()?;
    let memory_journals = memory.verify_memory_journals()?;
    drop(memory);
    let learning = LearningMemory::open_read_only(&learning_path)?;
    let learning_families = learning.inventory()?;
    let knowledge_index = learning.verify_knowledge_index()?;
    drop(learning);
    Ok(json!({
        "contract_version": 1,
        "status": "verified",
        "verifier_version": env!("CARGO_PKG_VERSION"),
        "data_directory": data_directory,
        "stores": {
            "graph": {
                "path": data_directory,
                "families": graph_families,
            },
            "agent_memory": {
                "path": memory_path,
                "families": memory_families,
                "journals": memory_journals,
            },
            "procedural_learning": {
                "path": learning_path,
                "families": learning_families,
                "knowledge_index": knowledge_index,
            },
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use qilbee_memory::MemoryStorageConfig;
    use qilbee_storage::StorageOptions;
    use std::time::{Duration, Instant};
    use tempfile::TempDir;

    const WRITER_FIXTURE: &str = "QILBEEDB_VERIFY_STORE_WRITER_FIXTURE";

    fn populate(dir: &Path) {
        drop(StorageEngine::open(StorageOptions::for_testing(dir)).unwrap());
        drop(
            RocksDbMemoryStorage::open(MemoryStorageConfig::for_testing(
                &dir.join(AGENT_MEMORY_DIRECTORY),
            ))
            .unwrap(),
        );
        drop(LearningMemory::open(dir.join(PROCEDURAL_LEARNING_DIRECTORY)).unwrap());
    }

    fn command(dir: &Path) -> Result<Option<Value>> {
        verify_store_command(&[COMMAND.into(), dir.to_str().unwrap().into()])
    }

    #[test]
    fn verify_store_reports_every_store_without_writing() {
        let dir = TempDir::new().unwrap();
        populate(dir.path());
        let first = command(dir.path()).unwrap().unwrap();
        assert_eq!(first["contract_version"], 1);
        assert_eq!(first["status"], "verified");
        assert_eq!(first["verifier_version"], env!("CARGO_PKG_VERSION"));
        let stores = first["stores"].as_object().unwrap();
        assert_eq!(stores.len(), 3);
        assert_eq!(stores["graph"]["families"].as_array().unwrap().len(), 10);
        assert_eq!(
            stores["agent_memory"]["families"].as_array().unwrap().len(),
            4
        );
        assert_eq!(stores["agent_memory"]["journals"]["namespaces"], 0);
        assert_eq!(stores["agent_memory"]["journals"]["links_checked"], 0);
        let learning = &stores["procedural_learning"];
        assert_eq!(learning["families"].as_array().unwrap().len(), 1);
        assert_eq!(learning["knowledge_index"]["knowledge_receipts"], 0);
        assert_eq!(learning["knowledge_index"]["derived_entries"], 1);
        assert!(learning["knowledge_index"]["generation"].is_string());
        // A second verification observes identical bytes: nothing was written.
        let second = command(dir.path()).unwrap().unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn verify_store_rejects_incomplete_directories_and_malformed_commands() {
        assert!(verify_store_command(&[]).unwrap().is_none());
        assert!(
            verify_store_command(&["bootstrap-master".into(), "x".into()])
                .unwrap()
                .is_none()
        );
        assert!(matches!(
            verify_store_command(&[COMMAND.into()]),
            Err(Error::Configuration(_))
        ));
        assert!(matches!(
            verify_store_command(&[COMMAND.into(), "a".into(), "b".into()]),
            Err(Error::Configuration(_))
        ));
        let dir = TempDir::new().unwrap();
        assert!(matches!(
            command(&dir.path().join("absent")),
            Err(Error::Configuration(_))
        ));
        drop(StorageEngine::open(StorageOptions::for_testing(dir.path())).unwrap());
        let incomplete = command(dir.path());
        assert!(
            matches!(&incomplete, Err(Error::Storage(message)) if message.contains(AGENT_MEMORY_DIRECTORY)),
            "{incomplete:?}"
        );
        // A store from another crate at the expected path is not accepted.
        drop(LearningMemory::open(dir.path().join(AGENT_MEMORY_DIRECTORY)).unwrap());
        drop(LearningMemory::open(dir.path().join(PROCEDURAL_LEARNING_DIRECTORY)).unwrap());
        assert!(matches!(command(dir.path()), Err(Error::DataCorruption(_))));
    }

    #[test]
    fn verify_store_refuses_a_directory_held_by_another_process() {
        if let Some(directory) = std::env::var_os(WRITER_FIXTURE) {
            let directory = std::path::PathBuf::from(directory);
            let _memory = RocksDbMemoryStorage::open(MemoryStorageConfig::for_testing(
                &directory.join("data").join(AGENT_MEMORY_DIRECTORY),
            ))
            .unwrap();
            std::fs::write(directory.join("ready"), b"1").unwrap();
            while !directory.join("stop").exists() {
                std::thread::sleep(Duration::from_millis(20));
            }
            std::process::exit(0);
        }
        let dir = TempDir::new().unwrap();
        let data = dir.path().join("data");
        populate(&data);
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "store_verification::tests::verify_store_refuses_a_directory_held_by_another_process",
                "--nocapture",
            ])
            .env(WRITER_FIXTURE, dir.path())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(60);
        while !dir.path().join("ready").exists() {
            assert!(Instant::now() < deadline, "writer never became ready");
            std::thread::sleep(Duration::from_millis(20));
        }
        let held = command(&data);
        std::fs::write(dir.path().join("stop"), b"1").unwrap();
        assert!(child.wait().unwrap().success());
        assert!(
            matches!(&held, Err(Error::Storage(message)) if message.contains("open by process")),
            "{held:?}"
        );
        assert_eq!(command(&data).unwrap().unwrap()["status"], "verified");
    }
}
