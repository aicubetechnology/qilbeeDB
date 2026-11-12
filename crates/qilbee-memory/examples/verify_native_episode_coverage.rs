//! Acceptance on actual stopped RocksDB and the actual verify-store executable.
use qilbee_core::temporal::EventTime;
use qilbee_memory::episode::EpisodeId;
use qilbee_memory::{
    Episode, EpisodeContent, EpisodeType, MemoryStorage, MemoryStorageConfig, RocksDbMemoryStorage,
};
use std::path::Path;

const EPISODES: &str = "memory_episodes";
const INDEX: &str = "memory_episode_index";

fn primary(episode: &Episode) -> Vec<u8> {
    let mut key = vec![1];
    key.extend_from_slice(&(episode.agent_id.len() as u16).to_be_bytes());
    key.extend_from_slice(episode.agent_id.as_bytes());
    key.extend_from_slice(&episode.event_time.as_millis().to_be_bytes());
    key.extend_from_slice(episode.id.as_uuid().as_bytes());
    key
}
fn index(id: EpisodeId) -> Vec<u8> {
    let mut key = vec![2];
    key.extend_from_slice(id.as_uuid().as_bytes());
    key
}
fn location(agent: &str, time: i64) -> Vec<u8> {
    let mut value = (agent.len() as u16).to_be_bytes().to_vec();
    value.extend_from_slice(agent.as_bytes());
    value.extend_from_slice(&time.to_be_bytes());
    value
}
fn raw(path: &Path) -> rocksdb::DB {
    let options = rocksdb::Options::default();
    let families = rocksdb::DB::list_cf(&options, path).unwrap();
    rocksdb::DB::open_cf(&options, path, families).unwrap()
}
fn inventory(path: &Path) -> Vec<qilbee_storage::FamilyInventory> {
    let families = ["default", EPISODES, INDEX, "memory_agent_meta"];
    let database = qilbee_storage::verification::open_read_only(path, &families).unwrap();
    families
        .iter()
        .map(|family| {
            qilbee_storage::verification::family_inventory(&database, family, |_| false).unwrap()
        })
        .collect()
}
fn replace(path: &Path, family: &str, key: &[u8], value: Option<&[u8]>) -> Option<Vec<u8>> {
    let database = raw(path);
    let family = database.cf_handle(family).unwrap();
    let original = database.get_cf(family, key).unwrap();
    let mut options = rocksdb::WriteOptions::default();
    options.set_sync(true);
    match value {
        Some(value) => database.put_cf_opt(family, key, value, &options).unwrap(),
        None => database.delete_cf_opt(family, key, &options).unwrap(),
    }
    original
}
fn read(path: &Path, family: &str, key: &[u8]) -> Vec<u8> {
    let database = raw(path);
    database
        .get_cf(database.cf_handle(family).unwrap(), key)
        .unwrap()
        .unwrap()
}
fn verify(path: &Path, healthy: bool) {
    let before = inventory(path);
    let store = RocksDbMemoryStorage::open_read_only(path).unwrap();
    let result = store.verify_memory_projections();
    if healthy {
        result.unwrap();
    } else {
        assert!(
            matches!(result, Err(qilbee_core::Error::DataCorruption(_))),
            "offline verification accepted an inconsistent native episode or UUID index"
        );
    }
    drop(store);
    assert_eq!(
        inventory(path),
        before,
        "read-only verification mutated records"
    );
}
fn fault(path: &Path, family: &str, key: &[u8], value: Option<&[u8]>) {
    let before = inventory(path);
    let original = replace(path, family, key, value);
    verify(path, false);
    replace(path, family, key, original.as_deref());
    verify(path, true);
    assert_eq!(
        inventory(path),
        before,
        "owned fault recovery did not restore original bytes"
    );
}
async fn fixture(path: &Path) -> Vec<Episode> {
    let store = RocksDbMemoryStorage::open(MemoryStorageConfig {
        path: path.to_string_lossy().into_owned(),
        sync_writes: true,
        ..Default::default()
    })
    .unwrap();
    let mut episodes = Vec::new();
    for (agent, time) in [
        ("coverage-a", -1),
        ("coverage-b", 42),
        ("coverage-a", i64::MIN),
        ("coverage-\u{03b1}", i64::MAX),
    ] {
        let episode = Episode::with_event_time(
            agent,
            EpisodeType::Observation,
            EpisodeContent::new("disposable native verification fixture")
                .with_data(serde_json::json!({"fixture": true})),
            EventTime::from_millis(time),
        );
        store.store_episode(agent, &episode).await.unwrap();
        episodes.push(episode);
    }
    // Native and platform records coexist in the same families.
    let author = qilbee_memory::storage::platform::RecordAuthor {
        credential_id: uuid::Uuid::new_v4(),
        subject_id: "coverage-owner".into(),
    };
    let command = serde_json::from_value(serde_json::json!({
        "contract_version":1, "idempotency_key":"platform-coexistence",
        "operation":{"type":"create", "record":{"episode_type":"Observation",
            "event_time_millis":0, "content":{"primary":"disposable platform fixture"}}}
    }))
    .unwrap();
    store
        .apply_memory_command("coverage", &author, &command)
        .unwrap();
    drop(store);
    episodes
}
fn current_bytes(episode: &Episode) -> Vec<u8> {
    let mut binary = episode.clone();
    let data = binary
        .content
        .data
        .take()
        .map(|value| serde_json::to_string(&value).unwrap());
    let mut bytes = b"QMEP\0\x02".to_vec();
    bytes.extend(bincode::serialize(&(binary, data)).unwrap());
    bytes
}
fn legacy_bytes(episode: &Episode, envelope: bool) -> Vec<u8> {
    let mut content = episode.content.clone();
    let data = content
        .data
        .take()
        .map(|value| serde_json::to_string(&value).unwrap());
    // Exact pre-accounting wire fields; tuples and structs share this binary order.
    let legacy = (
        episode.id,
        &episode.agent_id,
        &episode.episode_type,
        episode.event_time,
        episode.transaction_time,
        content,
        &episode.metadata,
        (
            episode.relevance.score,
            episode.relevance.access_count,
            episode.relevance.last_accessed,
        ),
        episode.consolidated,
        episode.invalidated_at,
    );
    if envelope {
        let mut bytes = b"QMEP\0\x01".to_vec();
        bytes.extend(bincode::serialize(&(legacy, data)).unwrap());
        bytes
    } else {
        bincode::serialize(&legacy).unwrap()
    }
}
async fn native_acceptance(root: &Path) {
    let path = root.join("native");
    let episodes = fixture(&path).await;
    verify(&path, true);
    let episode = &episodes[0];
    let key = primary(episode);
    let lookup = index(episode.id);
    let bytes = read(&path, EPISODES, &key);
    // The first case reproduces the pre-fix false success on unchanged main.
    fault(&path, INDEX, &lookup, None);
    fault(&path, EPISODES, &key, None);
    for value in [
        location("coverage-b", -1),
        location(&episode.agent_id, 1),
        location("", -1),
        vec![0],
        vec![0, 1, 0xff, 0, 0, 0, 0, 0, 0, 0, 1],
        [location(&episode.agent_id, -1), vec![0]].concat(),
    ] {
        fault(&path, INDEX, &lookup, Some(&value));
    }
    for malformed in [
        vec![2],
        lookup[..lookup.len() - 1].to_vec(),
        [lookup.clone(), vec![0]].concat(),
        index(EpisodeId::new()),
    ] {
        fault(
            &path,
            INDEX,
            &malformed,
            Some(&location(&episode.agent_id, -1)),
        );
    }
    for malformed in [
        vec![1],
        key[..key.len() - 1].to_vec(),
        [key.clone(), vec![0]].concat(),
        [vec![1, 0, 0], key[key.len() - 24..].to_vec()].concat(),
        [vec![1, 0, 1, 0xff], key[key.len() - 24..].to_vec()].concat(),
    ] {
        fault(&path, EPISODES, &malformed, Some(&bytes));
    }
    // A second physical location for the same UUID must not be accepted.
    let mut moved = episode.clone();
    moved.event_time = EventTime::from_millis(77);
    fault(
        &path,
        EPISODES,
        &primary(&moved),
        Some(&current_bytes(&moved)),
    );
    let mut changed = episode.clone();
    changed.agent_id = "other-owner".into();
    fault(&path, EPISODES, &key, Some(&current_bytes(&changed)));
    let mut changed = episode.clone();
    changed.id = EpisodeId::new();
    fault(&path, EPISODES, &key, Some(&current_bytes(&changed)));
    let mut changed = episode.clone();
    changed.event_time = EventTime::from_millis(77);
    fault(&path, EPISODES, &key, Some(&current_bytes(&changed)));
    for malformed in [
        b"QMEP\0\x03".to_vec(),
        vec![0xff],
        bytes[..bytes.len() - 1].to_vec(),
    ] {
        fault(&path, EPISODES, &key, Some(&malformed));
    }
    // Historical bytes are validated without rewriting their representation.
    for envelope in [false, true] {
        let original = replace(
            &path,
            EPISODES,
            &key,
            Some(&legacy_bytes(episode, envelope)),
        )
        .unwrap();
        verify(&path, true);
        let store = RocksDbMemoryStorage::open_read_only(&path).unwrap();
        let decoded = store
            .get_episode(&episode.agent_id, episode.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(decoded.id, episode.id);
        assert_eq!(decoded.agent_id, episode.agent_id);
        assert_eq!(decoded.event_time, episode.event_time);
        assert_eq!(decoded.content.data.is_some(), envelope);
        drop(store);
        let mut foreign = episode.clone();
        foreign.agent_id = "foreign-owner".into();
        fault(
            &path,
            EPISODES,
            &key,
            Some(&legacy_bytes(&foreign, envelope)),
        );
        replace(&path, EPISODES, &key, Some(&original));
    }
    let store = RocksDbMemoryStorage::open(MemoryStorageConfig {
        path: path.to_string_lossy().into_owned(),
        sync_writes: true,
        ..Default::default()
    })
    .unwrap();
    let mut moved = episode.clone();
    moved.event_time = EventTime::from_millis(123);
    store.update_episode(&moved.agent_id, &moved).await.unwrap();
    store
        .invalidate_current_episode(&episodes[1].agent_id, episodes[1].id)
        .await
        .unwrap();
    assert!(
        store
            .delete_episode(&episodes[2].agent_id, episodes[2].id)
            .await
            .unwrap()
    );
    drop(store);
    verify(&path, true);
    let reopened = RocksDbMemoryStorage::open_read_only(&path).unwrap();
    assert_eq!(
        reopened
            .get_episode(&moved.agent_id, moved.id)
            .await
            .unwrap()
            .unwrap()
            .event_time,
        moved.event_time
    );
    assert!(
        reopened
            .get_episode(&episodes[1].agent_id, episodes[1].id)
            .await
            .unwrap()
            .unwrap()
            .invalidated_at
            .is_some()
    );
    assert!(
        reopened
            .get_episode(&episodes[2].agent_id, episodes[2].id)
            .await
            .unwrap()
            .is_none()
    );
    drop(reopened);
    let empty = root.join("empty");
    drop(
        RocksDbMemoryStorage::open(MemoryStorageConfig {
            path: empty.to_string_lossy().into_owned(),
            ..Default::default()
        })
        .unwrap(),
    );
    verify(&empty, true);
    println!(
        "{{\"native_identity_index_faults_and_recovery\":\"passed\",\"legacy_layouts_and_real_reopen\":\"passed\"}}"
    );
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
async fn operator_acceptance(binary: &Path, root: &Path) {
    let source = root.join("source");
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
    let episodes = fixture(&memory).await;
    let before = inventory(&memory);
    let healthy = operator(binary, &source);
    assert!(
        healthy.status.success(),
        "healthy verify-store failed: {}",
        String::from_utf8_lossy(&healthy.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&healthy.stdout).unwrap()["status"],
        "verified"
    );
    let key = index(episodes[0].id);
    let original = replace(&memory, INDEX, &key, None).unwrap();
    let corrupt = inventory(&memory);
    let failed = operator(binary, &source);
    assert!(!failed.status.success());
    assert!(
        failed.stdout.is_empty(),
        "failed verification printed partial success"
    );
    assert!(
        String::from_utf8_lossy(&failed.stderr)
            .contains("Native episode or UUID index is inconsistent")
    );
    assert_eq!(inventory(&memory), corrupt);
    replace(&memory, INDEX, &key, Some(&original));
    assert!(operator(binary, &source).status.success());
    assert_eq!(inventory(&memory), before);
    let copy = root.join("copy");
    copy_directory(&source, &copy);
    let compared = std::process::Command::new(binary)
        .arg("verify-store")
        .arg(&copy)
        .arg("--source")
        .arg(&source)
        .output()
        .unwrap();
    assert!(
        compared.status.success(),
        "copy comparison failed: {}",
        String::from_utf8_lossy(&compared.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&compared.stdout).unwrap()["status"],
        "verified_equal"
    );
    println!("{{\"actual_operator_failure_recovery_and_copy\":\"passed\"}}");
}
#[tokio::main(flavor = "current_thread")]
async fn main() {
    let root = tempfile::tempdir().unwrap();
    native_acceptance(root.path()).await;
    if let Some(binary) = std::env::args_os().nth(1) {
        operator_acceptance(Path::new(&binary), root.path()).await;
    }
}
