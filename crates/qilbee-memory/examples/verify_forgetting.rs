//! Real RocksDB acceptance for native forgetting; uses disposable local records.
use qilbee_core::temporal::EventTime;
use qilbee_memory::episode::EpisodeId;
use qilbee_memory::{
    Episode, EpisodeContent, EpisodeType, MemoryConfig, MemoryStorage, MemoryStorageConfig,
    PersistentAgentMemory, RocksDbMemoryStorage, relevance_accounting::RelevanceChange,
};
use std::{
    io::{BufRead, Write},
    process::{Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};

fn config(path: &std::path::Path) -> MemoryStorageConfig {
    MemoryStorageConfig {
        path: path.to_string_lossy().into_owned(),
        sync_writes: true,
        ..MemoryStorageConfig::default()
    }
}

/// The first observed deletion proves the manager has already selected its batch.
/// Raise the last candidate through the real storage while the batch is deleting.
async fn stale_scan_race(path: &std::path::Path) {
    let storage = Arc::new(RocksDbMemoryStorage::open(config(path)).unwrap());
    let mut ids = Vec::new();
    for i in 0..2048 {
        let mut episode = Episode::with_event_time(
            "retention-owner",
            EpisodeType::Observation,
            EpisodeContent::new("disposable retention acceptance record"),
            EventTime::from_millis(i),
        );
        episode.relevance.score = 0.05;
        ids.push(episode.id);
        storage
            .store_episode("retention-owner", &episode)
            .await
            .unwrap();
    }
    let manager =
        PersistentAgentMemory::with_storage(MemoryConfig::new("retention-owner"), storage.clone());
    let first = ids[0];
    let last = *ids.last().unwrap();
    let observer = storage.clone();
    let worker = std::thread::spawn(move || {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async move {
            let start = Instant::now();
            while observer
                .get_episode("retention-owner", first)
                .await
                .unwrap()
                .is_some()
            {
                assert!(
                    start.elapsed() < Duration::from_secs(30),
                    "forget scan never started"
                );
                std::thread::yield_now();
            }
            observer
                .apply_relevance_change(
                    "retention-owner",
                    last,
                    RelevanceChange::AccessNow { boost: 0.2 },
                )
                .await
                .unwrap()
                .expect("race inconclusive: final candidate was deleted before the access");
        });
    });
    let deleted = manager.forget().await.unwrap();
    worker.join().unwrap();
    let survivor = storage.get_episode("retention-owner", last).await.unwrap();
    assert!(
        survivor.is_some(),
        "forget deleted an episode after its above-threshold access committed"
    );
    assert_eq!(
        deleted,
        ids.len() - 1,
        "forget must count only actual deletions"
    );
    assert_eq!(storage.episode_count("retention-owner").await.unwrap(), 1);
    drop(manager);
    drop(storage);
    let reopened = RocksDbMemoryStorage::open(config(path)).unwrap();
    assert!(
        reopened
            .get_episode("retention-owner", first)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        reopened
            .get_episode("retention-owner", last)
            .await
            .unwrap()
            .unwrap()
            .relevance
            .access_count,
        1
    );
    println!("{{\"stale_scan_race\":\"passed\",\"deleted\":{deleted},\"preserved\":1}}");
}

async fn storage_contract(path: &std::path::Path) {
    let storage = Arc::new(RocksDbMemoryStorage::open(config(path)).unwrap());
    let mut episode = Episode::observation("retention-owner", "disposable boundary record");
    episode.relevance.score = 0.1;
    storage
        .store_episode("retention-owner", &episode)
        .await
        .unwrap();
    assert!(
        !storage
            .forget_episode_if_below("retention-owner", episode.id, 0.1)
            .await
            .unwrap()
    );
    assert!(
        !storage
            .forget_episode_if_below("foreign-owner", episode.id, 1.0)
            .await
            .unwrap()
    );
    assert!(
        !storage
            .forget_episode_if_below("retention-owner", EpisodeId::new(), 1.0)
            .await
            .unwrap()
    );
    assert!(
        storage
            .forget_episode_if_below("", episode.id, 1.0)
            .await
            .is_err()
    );
    for threshold in [f64::NAN, f64::INFINITY, -0.1, 1.1] {
        assert!(
            storage
                .forget_episode_if_below("retention-owner", episode.id, threshold)
                .await
                .is_err()
        );
        let manager = PersistentAgentMemory::with_storage(
            MemoryConfig::new("retention-owner").min_relevance(threshold),
            storage.clone(),
        );
        assert!(manager.forget().await.is_err());
    }
    assert!(
        storage
            .get_episode("retention-owner", episode.id)
            .await
            .unwrap()
            .is_some()
    );
    episode.relevance.score = 0.0;
    storage
        .update_episode("retention-owner", &episode)
        .await
        .unwrap();
    assert!(
        !storage
            .forget_episode_if_below("retention-owner", episode.id, 0.0)
            .await
            .unwrap()
    );
    episode.relevance.score = 1.5;
    storage
        .update_episode("retention-owner", &episode)
        .await
        .unwrap();
    assert!(
        storage
            .forget_episode_if_below("retention-owner", episode.id, 1.0)
            .await
            .is_err()
    );
    episode.relevance.score = 0.05;
    storage
        .update_episode("retention-owner", &episode)
        .await
        .unwrap();
    let disabled = PersistentAgentMemory::with_storage(
        MemoryConfig::new("retention-owner").no_auto_forget(),
        storage.clone(),
    );
    assert_eq!(disabled.forget().await.unwrap(), 0);
    // Race two actual conditional writes: exactly one commits the deletion.
    let mut workers = Vec::new();
    let gate = Arc::new(std::sync::Barrier::new(3));
    for _ in 0..2 {
        let (backend, gate, id) = (storage.clone(), gate.clone(), episode.id);
        workers.push(std::thread::spawn(move || {
            let runtime = tokio::runtime::Runtime::new().unwrap();
            gate.wait();
            runtime
                .block_on(backend.forget_episode_if_below("retention-owner", id, 0.1))
                .unwrap()
        }));
    }
    gate.wait();
    let deleted = workers
        .into_iter()
        .filter_map(|worker| worker.join().unwrap().then_some(()))
        .count();
    assert_eq!(deleted, 1);
    assert!(
        storage
            .apply_relevance_change(
                "retention-owner",
                episode.id,
                RelevanceChange::AccessNow { boost: 0.2 }
            )
            .await
            .unwrap()
            .is_none()
    );
    let mut invalidated = Episode::observation("retention-owner", "disposable invalidated record");
    invalidated.relevance.score = 0.01;
    storage
        .store_episode("retention-owner", &invalidated)
        .await
        .unwrap();
    storage
        .invalidate_current_episode("retention-owner", invalidated.id)
        .await
        .unwrap();
    assert!(
        !storage
            .forget_episode_if_below("retention-owner", invalidated.id, 1.0)
            .await
            .unwrap()
    );
    assert!(
        storage
            .get_episode("retention-owner", invalidated.id)
            .await
            .unwrap()
            .is_some()
    );
    // A timestamp move must still resolve and delete exactly the current index location.
    let mut moved = Episode::observation("retention-owner", "disposable moved record");
    moved.relevance.score = 0.01;
    storage
        .store_episode("retention-owner", &moved)
        .await
        .unwrap();
    moved.event_time = EventTime::from_millis(123);
    storage
        .update_episode("retention-owner", &moved)
        .await
        .unwrap();
    assert!(
        storage
            .forget_episode_if_below("retention-owner", moved.id, 0.1)
            .await
            .unwrap()
    );
    assert!(
        storage
            .get_all_episodes("retention-owner")
            .await
            .unwrap()
            .is_empty()
    );
    drop(disabled);
    drop(storage);
    let reopened = RocksDbMemoryStorage::open(config(path)).unwrap();
    assert!(
        reopened
            .get_episode("retention-owner", episode.id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        reopened
            .get_episode("retention-owner", moved.id)
            .await
            .unwrap()
            .is_none()
    );
    println!("{{\"scope_boundaries_concurrent_counts_reopen\":\"passed\"}}");
}

async fn read_only_failure(path: &std::path::Path) {
    let mut episode = Episode::observation("retention-owner", "disposable read-only record");
    episode.relevance.score = 0.01;
    let storage = RocksDbMemoryStorage::open(config(path)).unwrap();
    storage
        .store_episode("retention-owner", &episode)
        .await
        .unwrap();
    drop(storage);
    let reader = RocksDbMemoryStorage::open_read_only(path).unwrap();
    assert!(matches!(
        reader
            .forget_episode_if_below("retention-owner", episode.id, 0.1)
            .await,
        Err(qilbee_core::Error::Storage(_))
    ));
    assert!(
        reader
            .get_episode("retention-owner", episode.id)
            .await
            .unwrap()
            .is_some()
    );
    drop(reader);
    let reopened = RocksDbMemoryStorage::open(config(path)).unwrap();
    assert!(
        reopened
            .get_episode("retention-owner", episode.id)
            .await
            .unwrap()
            .is_some()
    );
    println!("{{\"real_read_only_write_rejection\":\"passed\"}}");
}

async fn killed_writer_recovery(path: &std::path::Path) {
    let storage = RocksDbMemoryStorage::open(config(path)).unwrap();
    let mut episode = Episode::observation("retention-owner", "disposable crash record");
    episode.relevance.score = 0.01;
    let survivor = Episode::observation("retention-owner", "disposable crash survivor");
    storage
        .store_episode("retention-owner", &episode)
        .await
        .unwrap();
    storage
        .store_episode("retention-owner", &survivor)
        .await
        .unwrap();
    drop(storage);
    let mut child = Command::new(std::env::current_exe().unwrap())
        .arg("--crash-writer")
        .arg(path)
        .arg(episode.id.to_string())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    std::io::BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert_eq!(line.trim(), "deletion-acknowledged");
    child.kill().unwrap();
    child.wait().unwrap();
    let recovered = RocksDbMemoryStorage::open(config(path)).unwrap();
    assert!(
        recovered
            .get_episode("retention-owner", episode.id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        !recovered
            .forget_episode_if_below("retention-owner", episode.id, 0.1)
            .await
            .unwrap()
    );
    assert_eq!(
        recovered
            .get_all_episodes("retention-owner")
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(
        recovered
            .get_episode("retention-owner", survivor.id)
            .await
            .unwrap()
            .is_some()
    );
    println!("{{\"acknowledged_delete_process_kill_recovery\":\"passed\"}}");
}

#[tokio::main]
async fn main() {
    let arguments: Vec<_> = std::env::args().collect();
    if arguments.get(1).map(String::as_str) == Some("--crash-writer") {
        let storage =
            RocksDbMemoryStorage::open(config(std::path::Path::new(&arguments[2]))).unwrap();
        let id = EpisodeId::from_uuid(uuid::Uuid::parse_str(&arguments[3]).unwrap());
        assert!(
            storage
                .forget_episode_if_below("retention-owner", id, 0.1)
                .await
                .unwrap()
        );
        println!("deletion-acknowledged");
        std::io::stdout().flush().unwrap();
        std::thread::sleep(Duration::from_secs(30));
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    stale_scan_race(&directory.path().join("race")).await;
    storage_contract(&directory.path().join("contract")).await;
    read_only_failure(&directory.path().join("read-only")).await;
    killed_writer_recovery(&directory.path().join("recovery")).await;
}
