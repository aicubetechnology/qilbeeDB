//! Real stopped-RocksDB acceptance for complete physical journal coverage.
use qilbee_memory::{
    MemoryStorageConfig, RocksDbMemoryStorage,
    storage::platform::{MemoryCommand, RecordAuthor},
};
use std::path::Path;

fn key(kind: u8, namespace: &str, sequence: Option<u64>) -> Vec<u8> {
    let mut key = vec![kind];
    key.extend_from_slice(&(namespace.len() as u16).to_be_bytes());
    key.extend_from_slice(namespace.as_bytes());
    if let Some(sequence) = sequence {
        key.extend_from_slice(&sequence.to_be_bytes());
    }
    key
}
fn raw(path: &Path) -> rocksdb::DB {
    let options = rocksdb::Options::default();
    let families = rocksdb::DB::list_cf(&options, path).unwrap();
    rocksdb::DB::open_cf(&options, path, families).unwrap()
}
fn fixture(path: &Path) {
    let store = RocksDbMemoryStorage::open(MemoryStorageConfig {
        path: path.to_string_lossy().into_owned(),
        sync_writes: true,
        ..Default::default()
    })
    .unwrap();
    let author = RecordAuthor {
        credential_id: uuid::Uuid::new_v4(),
        subject_id: "coverage-owner".into(),
    };
    for n in 0..3 {
        let command: MemoryCommand = serde_json::from_value(serde_json::json!({
            "contract_version":1,"idempotency_key":format!("coverage-{n}"),
            "operation":{"type":"create","record":{"episode_type":"Observation",
                "event_time_millis":0,"content":{"primary":"disposable journal coverage fixture"}}}
        }))
        .unwrap();
        store
            .apply_memory_command("coverage", &author, &command)
            .unwrap();
    }
}
fn inventory(path: &Path) -> Vec<qilbee_storage::FamilyInventory> {
    let families = [
        "default",
        "memory_episodes",
        "memory_episode_index",
        "memory_agent_meta",
    ];
    let database = qilbee_storage::verification::open_read_only(path, &families).unwrap();
    families
        .iter()
        .map(|family| {
            qilbee_storage::verification::family_inventory(&database, family, |_| false).unwrap()
        })
        .collect()
}
fn verify(path: &Path, links: Option<u64>) {
    let before = inventory(path);
    let store = RocksDbMemoryStorage::open_read_only(path).unwrap();
    let result = store.verify_memory_journals();
    if let Some(links) = links {
        assert_eq!(result.unwrap().links_checked, links);
    } else {
        assert!(
            matches!(result, Err(qilbee_core::Error::DataCorruption(_))),
            "offline verification accepted an unaccounted or inconsistent physical journal row"
        );
    }
    drop(store);
    assert_eq!(
        inventory(path),
        before,
        "verification changed stored records"
    );
}
fn read(path: &Path, key: &[u8]) -> Vec<u8> {
    let database = raw(path);
    database
        .get_cf(database.cf_handle("memory_agent_meta").unwrap(), key)
        .unwrap()
        .unwrap()
}
fn replace(path: &Path, key: &[u8], value: Option<&[u8]>) -> Option<Vec<u8>> {
    let database = raw(path);
    let meta = database.cf_handle("memory_agent_meta").unwrap();
    let original = database.get_cf(meta, key).unwrap();
    let mut options = rocksdb::WriteOptions::default();
    options.set_sync(true);
    if let Some(value) = value {
        database.put_cf_opt(meta, key, value, &options).unwrap();
    } else {
        database.delete_cf_opt(meta, key, &options).unwrap();
    }
    original
}
fn fault(path: &Path, key: &[u8], value: Option<&[u8]>, links: u64) {
    let before = inventory(path);
    let original = replace(path, key, value);
    verify(path, None);
    replace(path, key, original.as_deref());
    verify(path, Some(links));
    assert_eq!(
        inventory(path),
        before,
        "fault recovery did not restore original records"
    );
}
fn strip_verified(path: &Path) {
    replace(path, &key(0x26, "coverage", None), None);
    for sequence in 1..=3 {
        replace(path, &key(0x27, "coverage", Some(sequence)), None);
    }
}
fn native_faults(root: &Path) {
    let normal = root.join("normal");
    fixture(&normal);
    verify(&normal, Some(3));
    let event = read(&normal, &key(0x21, "coverage", Some(3)));
    let anchor = read(&normal, &key(0x27, "coverage", Some(3)));
    // A gap before an orphan must not hide either event or anchor rows.
    for sequence in [5, u64::MAX] {
        fault(
            &normal,
            &key(0x21, "coverage", Some(sequence)),
            Some(&event),
            3,
        );
        fault(
            &normal,
            &key(0x27, "coverage", Some(sequence)),
            Some(&anchor),
            3,
        );
    }
    for kind in [0x20, 0x21, 0x26, 0x27] {
        let source = if kind == 0x20 || kind == 0x26 {
            key(kind, "coverage", None)
        } else {
            key(kind, "coverage", Some(3))
        };
        let value = read(&normal, &source);
        let mut malformed = source.clone();
        malformed.push(0);
        fault(&normal, &malformed, Some(&value), 3);
    }
    // Missing/truncated suffix, sequence zero, invalid UTF-8 and empty namespace.
    for malformed in [
        key(0x21, "coverage", None),
        key(0x21, "coverage", Some(0)),
        key(0x27, "coverage", Some(0)),
        vec![0x20],
        vec![0x20, 0, 1, 0xff],
        key(0x20, "", None),
    ] {
        fault(&normal, &malformed, Some(&event), 3);
    }
    fault(&normal, &key(0x20, "coverage", None), None, 3);
    fault(&normal, &key(0x26, "coverage", None), None, 3);

    let legacy = root.join("legacy");
    fixture(&legacy);
    strip_verified(&legacy);
    verify(&legacy, Some(0));
    fault(&legacy, &key(0x21, "coverage", Some(1)), None, 0);
    let original = read(&legacy, &key(0x21, "coverage", Some(1)));
    let mut damaged: serde_json::Value = serde_json::from_slice(&original).unwrap();
    damaged["record_revision"] = serde_json::json!(17);
    let damaged = serde_json::to_vec(&damaged).unwrap();
    fault(&legacy, &key(0x21, "coverage", Some(1)), Some(&damaged), 0);
    // A valid event from another real journal must fail its journal identity binding.
    let foreign = root.join("foreign");
    fixture(&foreign);
    let foreign_event = read(&foreign, &key(0x21, "coverage", Some(1)));
    fault(
        &legacy,
        &key(0x21, "coverage", Some(1)),
        Some(&foreign_event),
        0,
    );

    let adopted = root.join("adopted");
    fixture(&adopted);
    strip_verified(&adopted);
    let store = RocksDbMemoryStorage::open(MemoryStorageConfig {
        path: adopted.to_string_lossy().into_owned(),
        sync_writes: true,
        ..Default::default()
    })
    .unwrap();
    assert_eq!(
        store
            .activate_verified_memory_journal("coverage")
            .unwrap()
            .sequence,
        3
    );
    let author = RecordAuthor {
        credential_id: uuid::Uuid::new_v4(),
        subject_id: "coverage-owner".into(),
    };
    let command: MemoryCommand = serde_json::from_value(serde_json::json!({
        "contract_version":1,"idempotency_key":"after-baseline","operation":{"type":"create",
        "record":{"episode_type":"Observation","event_time_millis":0,
        "content":{"primary":"disposable post-baseline fixture"}}}}))
    .unwrap();
    store
        .apply_memory_command("coverage", &author, &command)
        .unwrap();
    drop(store);
    verify(&adopted, Some(1));
    fault(&adopted, &key(0x21, "coverage", Some(1)), None, 1);
    fault(&adopted, &key(0x21, "coverage", Some(1)), Some(&damaged), 1);
    let post_baseline_anchor = read(&adopted, &key(0x27, "coverage", Some(4)));
    fault(
        &adopted,
        &key(0x27, "coverage", Some(1)),
        Some(&post_baseline_anchor),
        1,
    );
    println!(
        "{{\"physical_rows_legacy_prefix_baseline_faults\":\"passed\",\"read_only_recovery\":\"passed\"}}"
    );
}
fn healthy_ranges(root: &Path) {
    let empty = root.join("empty");
    let store = RocksDbMemoryStorage::open(MemoryStorageConfig {
        path: empty.to_string_lossy().into_owned(),
        sync_writes: true,
        ..Default::default()
    })
    .unwrap();
    assert_eq!(store.verify_memory_journals().unwrap().namespaces, 0);
    store
        .activate_verified_memory_journal("empty-baseline")
        .unwrap();
    drop(store);
    verify(&empty, Some(0));
    let paged = root.join("paged");
    let store = RocksDbMemoryStorage::open(MemoryStorageConfig {
        path: paged.to_string_lossy().into_owned(),
        sync_writes: true,
        ..Default::default()
    })
    .unwrap();
    let author = RecordAuthor {
        credential_id: uuid::Uuid::new_v4(),
        subject_id: "coverage-owner".into(),
    };
    store
        .activate_verified_memory_journal("empty-baseline")
        .unwrap();
    for (namespace, count) in [("paged-a", 300), ("paged-b", 2)] {
        for n in 0..count {
            let command: MemoryCommand = serde_json::from_value(serde_json::json!({
                "contract_version":1,"idempotency_key":format!("page-{n}"),
                "operation":{"type":"create","record":{"episode_type":"Observation",
                    "event_time_millis":0,"content":{"primary":"disposable multi-page fixture"}}}
            }))
            .unwrap();
            store
                .apply_memory_command(namespace, &author, &command)
                .unwrap();
        }
    }
    drop(store);
    verify(&paged, Some(302));
    let store = RocksDbMemoryStorage::open_read_only(&paged).unwrap();
    let report = store.verify_memory_journals().unwrap();
    assert_eq!(
        (
            report.namespaces,
            report.legacy_journals,
            report.verified_journals
        ),
        (3, 2, 3)
    );
    println!("{{\"empty_multi_namespace_multi_page\":\"passed\"}}");
}
fn copy_directory(source: &Path, target: &Path) {
    std::fs::create_dir_all(target).unwrap();
    for entry in std::fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let destination = target.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_directory(&entry.path(), &destination);
        } else {
            assert!(entry.file_type().unwrap().is_file());
            std::fs::copy(entry.path(), destination).unwrap();
        }
    }
}
fn operator(binary: &Path, root: &Path) -> std::process::Output {
    std::process::Command::new(binary)
        .arg("verify-store")
        .arg(root)
        .output()
        .unwrap()
}
fn operator_acceptance(binary: &Path, root: &Path) {
    let source = root.join("platform-source");
    drop(
        qilbee_storage::StorageEngine::open(qilbee_storage::StorageOptions::for_production(
            &source,
        ))
        .unwrap(),
    );
    drop(
        qilbee_memory::learning::LearningMemory::open(source.join("procedural-learning")).unwrap(),
    );
    let memory = source.join("agent-memory");
    fixture(&memory);
    let healthy = operator(binary, &source);
    assert!(
        healthy.status.success(),
        "actual verify-store rejected the healthy fixture: {}",
        String::from_utf8_lossy(&healthy.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&healthy.stdout).unwrap();
    assert_eq!(report["status"], "verified");
    let before = inventory(&memory);
    let event = read(&memory, &key(0x21, "coverage", Some(3)));
    let orphan = key(0x21, "coverage", Some(5));
    replace(&memory, &orphan, Some(&event));
    let failed = operator(binary, &source);
    assert!(!failed.status.success());
    assert!(
        failed.stdout.is_empty(),
        "failed verification printed a partial success report"
    );
    assert!(String::from_utf8_lossy(&failed.stderr).contains("Unsupported or inconsistent memory"));
    replace(&memory, &orphan, None);
    assert_eq!(inventory(&memory), before);
    assert!(operator(binary, &source).status.success());
    let candidate = root.join("platform-copy");
    copy_directory(&source, &candidate);
    let compared = std::process::Command::new(binary)
        .arg("verify-store")
        .arg(&candidate)
        .arg("--source")
        .arg(&source)
        .output()
        .unwrap();
    assert!(
        compared.status.success(),
        "actual source/copy comparison failed: {}",
        String::from_utf8_lossy(&compared.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&compared.stdout).unwrap();
    assert_eq!(report["status"], "verified_equal");
    println!("{{\"actual_verify_store_failure_recovery_and_copy\":\"passed\"}}");
}
fn main() {
    let directory = tempfile::tempdir().unwrap();
    native_faults(directory.path());
    healthy_ranges(directory.path());
    if let Some(binary) = std::env::args_os().nth(1) {
        operator_acceptance(Path::new(&binary), directory.path());
    }
}
