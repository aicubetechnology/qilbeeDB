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
use qilbee_storage::verification::WriterExclusion;
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
    match args {
        [_, directory] => verify_data_directory(Path::new(directory)).map(Some),
        [_, candidate, flag, source] if flag == SOURCE_FLAG => {
            compare_data_directories(Path::new(candidate), Path::new(source)).map(Some)
        }
        _ => Err(Error::Configuration(
            "Usage: qilbeedb verify-store <data-directory> [--source <source-directory>]".into(),
        )),
    }
}

/// Flag selecting the comparison form of the command.
pub const SOURCE_FLAG: &str = "--source";

/// Stores reported by every verification, in report order.
const STORES: [&str; 3] = ["graph", "agent_memory", "procedural_learning"];

fn canonical(directory: &Path) -> Result<std::path::PathBuf> {
    directory.canonicalize().map_err(|error| {
        Error::Configuration(format!(
            "Data directory {} is unavailable: {error}",
            directory.display()
        ))
    })
}

fn families(report: &Value, store: &str) -> std::collections::BTreeMap<String, (u64, String)> {
    report["stores"][store]["families"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|family| {
            Some((
                family["family"].as_str()?.to_owned(),
                (
                    family["records"].as_u64()?,
                    family["sha256"].as_str()?.to_owned(),
                ),
            ))
        })
        .collect()
}

/// Verify a candidate directory and a source directory, then require that
/// they hold the same data: every column family must have equal record counts
/// and digests, and the memory journal and knowledge index counts must match.
/// Knowledge index generations may differ. Any difference is a failure that
/// names each differing store and family; no report is produced.
pub fn compare_data_directories(candidate: &Path, source: &Path) -> Result<Value> {
    let candidate_path = canonical(candidate)?;
    let source_path = canonical(source)?;
    if candidate_path == source_path
        || candidate_path.starts_with(&source_path)
        || source_path.starts_with(&candidate_path)
    {
        return Err(Error::Configuration(
            "Source and candidate must be distinct directories that do not contain each other"
                .into(),
        ));
    }
    let mut paths = store_paths(&source_path);
    paths.extend(store_paths(&candidate_path));
    let mut guard = WriterExclusion::acquire(&paths)?;
    let source_report = verify_data_directory_guarded(&source_path, &mut guard)?;
    let candidate_report = verify_data_directory_guarded(&candidate_path, &mut guard)?;
    let mut differences = Vec::new();
    let mut compared = 0u64;
    for store in STORES {
        let expected = families(&source_report, store);
        let actual = families(&candidate_report, store);
        for (family, inventory) in &expected {
            compared += 1;
            if actual.get(family) != Some(inventory) {
                differences.push(format!("{store}/{family}"));
            }
        }
        for family in actual.keys() {
            if !expected.contains_key(family) {
                differences.push(format!("{store}/{family}"));
            }
        }
    }
    let journals_equal = source_report["stores"]["agent_memory"]["journals"]
        == candidate_report["stores"]["agent_memory"]["journals"];
    if !journals_equal {
        differences.push("agent_memory/journals".into());
    }
    let index = |report: &Value| {
        let index = &report["stores"]["procedural_learning"]["knowledge_index"];
        (
            index["knowledge_receipts"].clone(),
            index["combined_origins"].clone(),
            index["active_entries"].clone(),
            index["derived_entries"].clone(),
        )
    };
    let knowledge_index_equal = index(&source_report) == index(&candidate_report);
    if !knowledge_index_equal {
        differences.push("procedural_learning/knowledge_index".into());
    }
    if !differences.is_empty() {
        return Err(Error::ConstraintViolation(format!(
            "Candidate {} differs from source {} in: {}",
            candidate_path.display(),
            source_path.display(),
            differences.join(", ")
        )));
    }
    guard.finish()?;
    Ok(json!({
        "contract_version": 1,
        "status": "verified_equal",
        "verifier_version": env!("CARGO_PKG_VERSION"),
        "source": source_report,
        "candidate": candidate_report,
        "comparison": {
            "families_compared": compared,
            "journals_equal": true,
            "knowledge_index_equal": true,
        },
    }))
}

/// Verify every store of a stopped platform data directory.
pub fn verify_data_directory(data_directory: &Path) -> Result<Value> {
    let data_directory = canonical(data_directory)?;
    let mut guard = WriterExclusion::acquire(&store_paths(&data_directory))?;
    let report = verify_data_directory_guarded(&data_directory, &mut guard)?;
    guard.finish()?;
    Ok(report)
}

fn store_paths(directory: &Path) -> Vec<std::path::PathBuf> {
    vec![
        directory.to_owned(),
        directory.join(AGENT_MEMORY_DIRECTORY),
        directory.join(PROCEDURAL_LEARNING_DIRECTORY),
    ]
}

/// Verify using an existing exclusion spanning every source and candidate store.
/// The caller must successfully finish the guard before publishing this report.
pub fn verify_data_directory_guarded(
    data_directory: &Path,
    guard: &mut WriterExclusion,
) -> Result<Value> {
    let data_directory = canonical(data_directory)?;
    let memory_path = data_directory.join(AGENT_MEMORY_DIRECTORY);
    let learning_path = data_directory.join(PROCEDURAL_LEARNING_DIRECTORY);
    let graph = StorageEngine::open_read_only_guarded(&data_directory, guard)?;
    let graph_property_index = graph.verify_property_index()?;
    let graph_families = graph.inventory()?;
    drop(graph);
    let memory = RocksDbMemoryStorage::open_read_only_guarded(&memory_path, guard)?;
    let memory_families = memory.inventory()?;
    let memory_journals = memory.verify_memory_journals()?;
    let memory_projections = memory.verify_memory_projections()?;
    drop(memory);
    let learning = LearningMemory::open_read_only_guarded(&learning_path, guard)?;
    let learning_families = learning.inventory()?;
    let knowledge_index = learning.verify_knowledge_index()?;
    drop(learning);
    guard.check()?;
    Ok(json!({
        "contract_version": 1,
        "status": "verified",
        "verifier_version": env!("CARGO_PKG_VERSION"),
        "data_directory": data_directory,
        "stores": {
            "graph": {
                "path": data_directory,
                "families": graph_families,
                "property_index": graph_property_index,
            },
            "agent_memory": {
                "path": memory_path,
                "families": memory_families,
                "journals": memory_journals,
                "projections": memory_projections,
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

    fn compare(candidate: &Path, source: &Path) -> Result<Option<Value>> {
        verify_store_command(&[
            COMMAND.into(),
            candidate.to_str().unwrap().into(),
            SOURCE_FLAG.into(),
            source.to_str().unwrap().into(),
        ])
    }

    fn copy_directory(source: &Path, target: &Path) {
        std::fs::create_dir_all(target).unwrap();
        for entry in std::fs::read_dir(source).unwrap() {
            let entry = entry.unwrap();
            let destination = target.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy_directory(&entry.path(), &destination);
            } else {
                std::fs::copy(entry.path(), destination).unwrap();
            }
        }
    }

    #[test]
    fn verify_store_compares_a_faithful_copy_and_names_every_difference() {
        let dir = TempDir::new().unwrap();
        let source = dir.path().join("source");
        let copy = dir.path().join("copy");
        populate(&source);
        copy_directory(&source, &copy);
        let equal = compare(&copy, &source).unwrap().unwrap();
        assert_eq!(equal["status"], "verified_equal");
        assert_eq!(equal["comparison"]["families_compared"], 15);
        assert_eq!(equal["comparison"]["journals_equal"], true);
        assert_eq!(equal["comparison"]["knowledge_index_equal"], true);
        assert_eq!(equal["source"]["status"], "verified");
        assert_eq!(equal["candidate"]["status"], "verified");
        // One real write in the copy changes a family and the journal summary.
        let memory = RocksDbMemoryStorage::open(MemoryStorageConfig::for_testing(
            &copy.join(AGENT_MEMORY_DIRECTORY),
        ))
        .unwrap();
        memory.activate_verified_memory_journal("scope").unwrap();
        drop(memory);
        let differs = compare(&copy, &source);
        match differs {
            Err(Error::ConstraintViolation(message)) => {
                assert!(
                    message.contains("agent_memory/memory_agent_meta"),
                    "{message}"
                );
                assert!(message.contains("agent_memory/journals"), "{message}");
                assert!(!message.contains("graph/"), "{message}");
                assert!(!message.contains("procedural_learning"), "{message}");
            }
            other => panic!("expected a named difference, got {other:?}"),
        }
        // The single-directory form still verifies each side on its own.
        assert_eq!(command(&copy).unwrap().unwrap()["status"], "verified");
    }

    #[test]
    fn verify_store_comparison_rejects_same_nested_or_malformed_arguments() {
        let dir = TempDir::new().unwrap();
        let source = dir.path().join("source");
        populate(&source);
        assert!(matches!(
            compare(&source, &source),
            Err(Error::Configuration(_))
        ));
        assert!(matches!(
            compare(&source.join(AGENT_MEMORY_DIRECTORY), &source),
            Err(Error::Configuration(_))
        ));
        assert!(matches!(
            compare(&source, &source.join(PROCEDURAL_LEARNING_DIRECTORY)),
            Err(Error::Configuration(_))
        ));
        let path = source.to_str().unwrap().to_owned();
        assert!(matches!(
            verify_store_command(&[COMMAND.into(), path.clone(), "--sauce".into(), path.clone()]),
            Err(Error::Configuration(_))
        ));
        assert!(matches!(
            verify_store_command(&[COMMAND.into(), path.clone(), SOURCE_FLAG.into()]),
            Err(Error::Configuration(_))
        ));
        // A source that fails its own verification fails the comparison.
        let copy = dir.path().join("copy");
        copy_directory(&source, &copy);
        std::fs::remove_dir_all(copy.join(AGENT_MEMORY_DIRECTORY)).unwrap();
        assert!(matches!(compare(&source, &copy), Err(Error::Storage(_))));
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
        assert_eq!(stores["agent_memory"]["projections"]["namespaces"], 0);
        assert_eq!(
            stores["agent_memory"]["projections"]["candidate_entries"],
            0
        );
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
