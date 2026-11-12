//! Native admission acceptance using the real RocksDB backend and disposable data.
use qilbee_core::temporal::EventTime;
use qilbee_memory::{
    Episode, MemoryConfig, MemoryStorage, MemoryStorageConfig, PersistentAgentMemory,
    RocksDbMemoryStorage,
};
use qilbee_memory::{episode::EpisodeId, relevance_accounting::RelevanceChange};
use std::{
    io::{BufRead, Write},
    process::{Command, Stdio},
    sync::Arc,
    time::Duration,
};

fn config(path: &std::path::Path) -> MemoryStorageConfig {
    MemoryStorageConfig {
        path: path.to_string_lossy().into_owned(),
        sync_writes: true,
        ..Default::default()
    }
}

async fn rejected_write_preserves_existing(path: &std::path::Path) {
    let storage = Arc::new(RocksDbMemoryStorage::open(config(path)).unwrap());
    let existing = Episode::observation("owner-a", "disposable capacity survivor");
    let foreign = Episode::observation("owner-b", "disposable foreign UUID");
    storage.store_episode("owner-a", &existing).await.unwrap();
    storage.store_episode("owner-b", &foreign).await.unwrap();
    let manager = PersistentAgentMemory::with_storage(
        MemoryConfig::new("owner-a").max_episodes(1),
        storage.clone(),
    );
    let mut rejected = Episode::observation("owner-a", "disposable rejected write");
    rejected.id = foreign.id;
    assert!(manager.store_episode(rejected).await.is_err());
    assert!(
        storage
            .get_episode("owner-a", existing.id)
            .await
            .unwrap()
            .is_some(),
        "rejected foreign UUID write evicted an unrelated episode"
    );
    assert!(
        storage
            .get_episode("owner-b", foreign.id)
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(storage.episode_count("owner-a").await.unwrap(), 1);
    println!("{{\"rejected_write_preserves_existing\":\"passed\"}}");
}

async fn replacement_and_capacity(path: &std::path::Path) {
    let storage = Arc::new(RocksDbMemoryStorage::open(config(path)).unwrap());
    let manager = PersistentAgentMemory::with_storage(
        MemoryConfig::new("owner").max_episodes(2),
        storage.clone(),
    );
    let mut first = Episode::observation("owner", "disposable first record");
    first.relevance.score = 0.1;
    let mut second = Episode::observation("owner", "disposable second record");
    second.relevance.score = 0.2;
    manager.store_episode(first.clone()).await.unwrap();
    manager.store_episode(second.clone()).await.unwrap();
    first.event_time = EventTime::from_millis(100);
    manager.store_episode(first.clone()).await.unwrap();
    assert_eq!(storage.episode_count("owner").await.unwrap(), 2);
    assert!(
        storage
            .get_episode("owner", second.id)
            .await
            .unwrap()
            .is_some()
    );
    storage
        .apply_relevance_change("owner", first.id, RelevanceChange::AccessNow { boost: 0.5 })
        .await
        .unwrap();
    let third = Episode::observation("owner", "disposable admitted record");
    manager.store_episode(third.clone()).await.unwrap();
    assert!(
        storage
            .get_episode("owner", first.id)
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        storage
            .get_episode("owner", second.id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        storage
            .get_episode("owner", third.id)
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(storage.get_all_episodes("owner").await.unwrap().len(), 2);
    // A lower limit does not authorize deleting multiple existing records.
    let smaller = PersistentAgentMemory::with_storage(
        MemoryConfig::new("owner").max_episodes(1),
        storage.clone(),
    );
    assert!(
        smaller
            .store_episode(Episode::observation(
                "owner",
                "disposable refused admission"
            ))
            .await
            .is_err()
    );
    smaller.store_episode(third.clone()).await.unwrap();
    assert_eq!(storage.episode_count("owner").await.unwrap(), 2);
    let zero = PersistentAgentMemory::with_storage(
        MemoryConfig::new("owner").max_episodes(0),
        storage.clone(),
    );
    assert!(zero.store_episode(third.clone()).await.is_err());
    let mut malformed = Episode::observation("owner", "disposable invalid score");
    malformed.relevance.score = 1.5;
    assert!(manager.store_episode(malformed).await.is_err());
    assert_eq!(storage.episode_count("owner").await.unwrap(), 2);
    let mut invalidated = Episode::observation("owner", "disposable invalidated record");
    invalidated.invalidate();
    manager.store_episode(invalidated.clone()).await.unwrap();
    assert_eq!(storage.episode_count("owner").await.unwrap(), 2);
    invalidated.invalidated_at = None;
    manager.store_episode(invalidated.clone()).await.unwrap();
    assert_eq!(storage.episode_count("owner").await.unwrap(), 2);
    assert!(
        storage
            .get_episode("owner", invalidated.id)
            .await
            .unwrap()
            .is_some()
    );
    drop((zero, smaller, manager, storage));
    let reopened = RocksDbMemoryStorage::open(config(path)).unwrap();
    assert_eq!(reopened.episode_count("owner").await.unwrap(), 2);
    assert_eq!(reopened.get_all_episodes("owner").await.unwrap().len(), 2);
    println!("{{\"current_ranking_capacity_updates_reactivation_reopen\":\"passed\"}}");
}

async fn deterministic_eviction(path: &std::path::Path) {
    let storage = RocksDbMemoryStorage::open(config(path)).unwrap();
    let mut older = Episode::observation("owner", "disposable tie oldest");
    older.relevance.score = 0.3;
    older.event_time = EventTime::from_millis(10);
    let mut newer = older.clone();
    newer.id = EpisodeId::new();
    newer.event_time = EventTime::from_millis(20);
    storage
        .store_episode_with_capacity("owner", &newer, 2)
        .await
        .unwrap();
    storage
        .store_episode_with_capacity("owner", &older, 2)
        .await
        .unwrap();
    let incoming = Episode::observation("owner", "disposable tie admission");
    storage
        .store_episode_with_capacity("owner", &incoming, 2)
        .await
        .unwrap();
    assert!(
        storage
            .get_episode("owner", older.id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        storage
            .get_episode("owner", newer.id)
            .await
            .unwrap()
            .is_some()
    );
    // Equal scores and times use UUID order, independent of insertion order.
    storage.delete_all_episodes("owner").await.unwrap();
    older.event_time = EventTime::from_millis(20);
    let (low, high) = if older.id.as_uuid() < newer.id.as_uuid() {
        (older, newer)
    } else {
        (newer, older)
    };
    storage
        .store_episode_with_capacity("owner", &high, 2)
        .await
        .unwrap();
    storage
        .store_episode_with_capacity("owner", &low, 2)
        .await
        .unwrap();
    storage
        .store_episode_with_capacity("owner", &incoming, 2)
        .await
        .unwrap();
    assert!(
        storage
            .get_episode("owner", low.id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        storage
            .get_episode("owner", high.id)
            .await
            .unwrap()
            .is_some()
    );
    println!("{{\"deterministic_eviction_order\":\"passed\"}}");
}

async fn concurrent_managers(path: &std::path::Path) {
    let storage = Arc::new(RocksDbMemoryStorage::open(config(path)).unwrap());
    let foreign = Episode::observation("foreign", "disposable foreign survivor");
    storage.store_episode("foreign", &foreign).await.unwrap();
    let gate = Arc::new(std::sync::Barrier::new(9));
    let mut workers = Vec::new();
    for _ in 0..8 {
        let (backend, gate) = (storage.clone(), gate.clone());
        workers.push(std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let manager = PersistentAgentMemory::with_storage(
                MemoryConfig::new("owner").max_episodes(4),
                backend,
            );
            gate.wait();
            runtime.block_on(async {
                for _ in 0..8 {
                    manager
                        .store_episode(Episode::observation(
                            "owner",
                            "disposable concurrent admission",
                        ))
                        .await
                        .unwrap();
                    assert!(manager.episode_count().await.unwrap() <= 4);
                }
            });
        }));
    }
    gate.wait();
    for worker in workers {
        worker.join().unwrap();
    }
    assert_eq!(storage.episode_count("owner").await.unwrap(), 4);
    assert_eq!(storage.get_all_episodes("owner").await.unwrap().len(), 4);
    assert!(
        storage
            .get_episode("foreign", foreign.id)
            .await
            .unwrap()
            .is_some()
    );
    drop(storage);
    let reopened = RocksDbMemoryStorage::open(config(path)).unwrap();
    assert_eq!(reopened.episode_count("owner").await.unwrap(), 4);
    println!("{{\"concurrent_managers_64_admissions_capacity_4\":\"passed\"}}");
}

async fn real_write_failure(path: &std::path::Path) {
    let storage = RocksDbMemoryStorage::open(config(path)).unwrap();
    let existing = Episode::observation("owner", "disposable failed-batch survivor");
    storage.store_episode("owner", &existing).await.unwrap();
    drop(storage);
    let reader = Arc::new(RocksDbMemoryStorage::open_read_only(path).unwrap());
    let manager = PersistentAgentMemory::with_storage(
        MemoryConfig::new("owner").max_episodes(1),
        reader.clone(),
    );
    let incoming = Episode::observation("owner", "disposable rejected batch");
    assert!(matches!(
        manager.store_episode(incoming.clone()).await,
        Err(qilbee_core::Error::Storage(_))
    ));
    assert!(
        reader
            .get_episode("owner", existing.id)
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        reader
            .get_episode("owner", incoming.id)
            .await
            .unwrap()
            .is_none()
    );
    drop((manager, reader));
    let reopened = RocksDbMemoryStorage::open(config(path)).unwrap();
    assert_eq!(reopened.get_all_episodes("owner").await.unwrap().len(), 1);
    assert!(
        reopened
            .get_episode("owner", existing.id)
            .await
            .unwrap()
            .is_some()
    );
    println!("{{\"real_read_only_batch_failure_preserves_victim\":\"passed\"}}");
}

fn replace_index(path: &std::path::Path, key: &[u8], replacement: Option<&[u8]>) -> Vec<u8> {
    let options = rocksdb::Options::default();
    let families = rocksdb::DB::list_cf(&options, path).unwrap();
    let database = rocksdb::DB::open_cf(&options, path, families).unwrap();
    let index = database.cf_handle("memory_episode_index").unwrap();
    let original = database.get_cf(index, key).unwrap().unwrap();
    if let Some(bytes) = replacement {
        let mut writes = rocksdb::WriteOptions::default();
        writes.set_sync(true);
        database.put_cf_opt(index, key, bytes, &writes).unwrap();
    }
    original
}

async fn real_corrupt_index_rejection(path: &std::path::Path) {
    let existing = Episode::observation("owner", "disposable controlled corruption record");
    let incoming = Episode::observation("owner", "disposable corruption refusal");
    let storage = RocksDbMemoryStorage::open(config(path)).unwrap();
    storage.store_episode("owner", &existing).await.unwrap();
    drop(storage);
    let mut key = vec![0x02];
    key.extend_from_slice(existing.id.as_uuid().as_bytes());
    let original = replace_index(path, &key, None);
    let mut wrong_timestamp = original.clone();
    let end = wrong_timestamp.len();
    wrong_timestamp[end - 8..]
        .copy_from_slice(&(existing.event_time.as_millis() + 1).to_be_bytes());
    replace_index(path, &key, Some(&wrong_timestamp));
    let storage = RocksDbMemoryStorage::open(config(path)).unwrap();
    // A missing existing-record location and an inconsistent victim index must both refuse mutation.
    assert!(matches!(
        storage
            .store_episode_with_capacity("owner", &existing, 1)
            .await,
        Err(qilbee_core::Error::DataCorruption(_))
    ));
    assert!(matches!(
        storage
            .store_episode_with_capacity("owner", &incoming, 1)
            .await,
        Err(qilbee_core::Error::DataCorruption(_))
    ));
    assert_eq!(storage.get_all_episodes("owner").await.unwrap().len(), 1);
    assert!(
        storage
            .get_episode("owner", incoming.id)
            .await
            .unwrap()
            .is_none()
    );
    drop(storage);
    assert_eq!(replace_index(path, &key, Some(&original)), wrong_timestamp);
    let storage = RocksDbMemoryStorage::open(config(path)).unwrap();
    assert!(
        storage
            .get_episode("owner", existing.id)
            .await
            .unwrap()
            .is_some()
    );
    let mut malformed = existing.clone();
    malformed.relevance.score = 1.5;
    storage.store_episode("owner", &malformed).await.unwrap();
    assert!(matches!(
        storage
            .store_episode_with_capacity("owner", &incoming, 1)
            .await,
        Err(qilbee_core::Error::ValidationError(_))
    ));
    assert_eq!(storage.get_all_episodes("owner").await.unwrap().len(), 1);
    println!("{{\"real_corrupt_index_and_score_rejection\":\"passed\"}}");
}

async fn killed_writer_recovery(path: &std::path::Path) {
    let storage = RocksDbMemoryStorage::open(config(path)).unwrap();
    let existing = Episode::observation("owner", "disposable crash victim");
    storage.store_episode("owner", &existing).await.unwrap();
    drop(storage);
    let incoming_id = EpisodeId::new();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .arg("--crash-writer")
        .arg(path)
        .arg(incoming_id.to_string())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut acknowledgement = String::new();
    std::io::BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut acknowledgement)
        .unwrap();
    assert_eq!(acknowledgement.trim(), "admission-acknowledged");
    child.kill().unwrap();
    child.wait().unwrap();
    let reopened = RocksDbMemoryStorage::open(config(path)).unwrap();
    assert!(
        reopened
            .get_episode("owner", existing.id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        reopened
            .get_episode("owner", incoming_id)
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(reopened.episode_count("owner").await.unwrap(), 1);
    assert_eq!(reopened.get_all_episodes("owner").await.unwrap().len(), 1);
    println!("{{\"acknowledged_admission_eviction_process_kill_recovery\":\"passed\"}}");
}

#[tokio::main]
async fn main() {
    let arguments: Vec<_> = std::env::args().collect();
    if arguments.get(1).map(String::as_str) == Some("--crash-writer") {
        let storage = Arc::new(
            RocksDbMemoryStorage::open(config(std::path::Path::new(&arguments[2]))).unwrap(),
        );
        let manager = PersistentAgentMemory::with_storage(
            MemoryConfig::new("owner").max_episodes(1),
            storage,
        );
        let mut incoming = Episode::observation("owner", "disposable crash admission");
        incoming.id = EpisodeId::from_uuid(uuid::Uuid::parse_str(&arguments[3]).unwrap());
        manager.store_episode(incoming).await.unwrap();
        println!("admission-acknowledged");
        std::io::stdout().flush().unwrap();
        std::thread::sleep(Duration::from_secs(30));
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    rejected_write_preserves_existing(&directory.path().join("rejection")).await;
    replacement_and_capacity(&directory.path().join("capacity")).await;
    deterministic_eviction(&directory.path().join("ties")).await;
    concurrent_managers(&directory.path().join("concurrency")).await;
    real_write_failure(&directory.path().join("failure")).await;
    real_corrupt_index_rejection(&directory.path().join("corruption")).await;
    killed_writer_recovery(&directory.path().join("recovery")).await;
}
