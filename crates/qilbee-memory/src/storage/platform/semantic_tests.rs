use super::*;
use crate::{MemoryStorageConfig, RocksDbMemoryStorage};
use tempfile::TempDir;

fn open(path: &std::path::Path) -> RocksDbMemoryStorage {
    RocksDbMemoryStorage::open(MemoryStorageConfig::for_testing(path)).unwrap()
}
fn actor() -> RecordAuthor {
    RecordAuthor {
        credential_id: Uuid::new_v4(),
        subject_id: "writer".into(),
    }
}
fn input(text: &str) -> RecordInput {
    RecordInput {
        episode_type: EpisodeType::Observation,
        content: EpisodeContent::new(text),
        event_time_millis: 1700000000000,
        valid_until_millis: None,
        tags: vec!["knowledge".into()],
        metadata: Default::default(),
    }
}
fn create(store: &RocksDbMemoryStorage, scope: &str, id: &str) -> CommandReceipt {
    store
        .apply_memory_command(
            scope,
            &actor(),
            &MemoryCommand {
                contract_version: 1,
                idempotency_key: id.into(),
                operation: MemoryOperation::Create { record: input(id) },
            },
        )
        .unwrap()
}
fn space() -> EmbeddingSpace {
    EmbeddingSpace {
        provider: "fixture".into(),
        model: "fixture-model".into(),
        revision: "v1".into(),
        dimensions: 3,
    }
}
fn attach(id: Uuid, key: &str, vector: Vec<f32>) -> EmbeddingCommand {
    EmbeddingCommand {
        contract_version: 1,
        idempotency_key: key.into(),
        record_id: id,
        record_revision: 1,
        space: space(),
        vector,
    }
}
fn search() -> SemanticQuery {
    SemanticQuery {
        space: space(),
        vector: vec![1.0, 0.0, 0.0],
        limit: 10,
        min_score: -1.0,
        after: None,
        scan_limit: 100,
        episode_type: None,
        tag: None,
    }
}
#[test]
fn semantic_rank_is_cosine_not_text_and_survives_reopen() {
    let dir = TempDir::new().unwrap();
    let store = open(dir.path());
    let author = actor();
    let relevant = create(&store, "scope", "relevant without shared query words");
    let unrelated = create(&store, "scope", "literal query words but unrelated vector");
    let a = attach(relevant.record_id, "embedding-a", vec![2.0, 0.0, 0.0]);
    let receipt = store.apply_memory_embedding("scope", &author, &a).unwrap();
    store
        .apply_memory_embedding(
            "scope",
            &author,
            &attach(unrelated.record_id, "embedding-b", vec![0.0, 1.0, 0.0]),
        )
        .unwrap();
    let page = store.search_memory_semantic("scope", &search()).unwrap();
    assert_eq!(page.hits[0].record.record_id, relevant.record_id);
    assert_eq!(page.hits[0].score, 1.0);
    assert_eq!(page.hits[1].score, 0.0);
    assert!(page.exhaustive);
    drop(store);
    let store = open(dir.path());
    assert_eq!(
        store.apply_memory_embedding("scope", &author, &a).unwrap(),
        receipt
    );
    assert_eq!(
        store
            .search_memory_semantic("scope", &search())
            .unwrap()
            .hits[0]
            .record
            .record_id,
        relevant.record_id
    );
    assert!(
        store
            .search_memory_semantic("foreign", &search())
            .unwrap()
            .hits
            .is_empty()
    );
    let mut wrong_model = search();
    wrong_model.space.revision = "v2".into();
    assert!(
        store
            .search_memory_semantic("scope", &wrong_model)
            .unwrap()
            .hits
            .is_empty()
    );
}
#[test]
fn semantic_embeddings_bind_exact_revisions_and_do_not_resurrect_deleted_memory() {
    let dir = TempDir::new().unwrap();
    let store = open(dir.path());
    let author = actor();
    let created = create(&store, "scope", "content");
    let original = attach(created.record_id, "embed", vec![1.0, 0.0, 0.0]);
    let receipt = store
        .apply_memory_embedding("scope", &author, &original)
        .unwrap();
    store
        .apply_memory_command(
            "scope",
            &author,
            &MemoryCommand {
                contract_version: 1,
                idempotency_key: "update".into(),
                operation: MemoryOperation::Update {
                    record_id: created.record_id,
                    expected_revision: 1,
                    record: input("changed content"),
                },
            },
        )
        .unwrap();
    assert!(
        store
            .search_memory_semantic("scope", &search())
            .unwrap()
            .hits
            .is_empty()
    );
    let mut stale = original.clone();
    stale.idempotency_key = "stale".into();
    assert!(
        store
            .apply_memory_embedding("scope", &author, &stale)
            .is_err()
    );
    let mut current = original.clone();
    current.idempotency_key = "embed-new".into();
    current.record_revision = 2;
    store
        .apply_memory_embedding("scope", &author, &current)
        .unwrap();
    assert_eq!(
        store
            .search_memory_semantic("scope", &search())
            .unwrap()
            .hits
            .len(),
        1
    );
    store
        .apply_memory_command(
            "scope",
            &author,
            &MemoryCommand {
                contract_version: 1,
                idempotency_key: "delete".into(),
                operation: MemoryOperation::Delete {
                    record_id: created.record_id,
                    expected_revision: 2,
                },
            },
        )
        .unwrap();
    assert!(
        store
            .search_memory_semantic("scope", &search())
            .unwrap()
            .hits
            .is_empty()
    );
    assert_eq!(
        store
            .apply_memory_embedding("scope", &author, &original)
            .unwrap(),
        receipt
    );
    assert!(
        store
            .search_memory_semantic("scope", &search())
            .unwrap()
            .hits
            .is_empty()
    );
}
#[test]
fn semantic_validation_rejects_zero_nonfinite_wrong_dimension_and_changed_vectors() {
    let dir = TempDir::new().unwrap();
    let store = open(dir.path());
    let author = actor();
    let record = create(&store, "scope", "content");
    for vector in [
        vec![],
        vec![1.0],
        vec![0.0; 3],
        vec![f32::NAN, 0.0, 0.0],
        vec![f32::INFINITY, 0.0, 0.0],
    ] {
        assert!(
            store
                .apply_memory_embedding("scope", &author, &attach(record.record_id, "bad", vector))
                .is_err()
        );
    }
    let original = attach(record.record_id, "first", vec![1.0, 0.0, 0.0]);
    store
        .apply_memory_embedding("scope", &author, &original)
        .unwrap();
    let mut changed = original.clone();
    changed.vector = vec![0.0, 1.0, 0.0];
    assert!(
        store
            .apply_memory_embedding("scope", &author, &changed)
            .is_err()
    );
    changed.idempotency_key = "different".into();
    assert!(
        store
            .apply_memory_embedding("scope", &author, &changed)
            .is_err()
    );
    let mut invalid = search();
    invalid.min_score = f64::NAN;
    assert!(store.search_memory_semantic("scope", &invalid).is_err());
    let mut invalid = search();
    invalid.vector = vec![0.0; 3];
    assert!(store.search_memory_semantic("scope", &invalid).is_err());
}
#[test]
fn semantic_pages_disclose_partial_ranking_and_preserve_filters() {
    let dir = TempDir::new().unwrap();
    let store = open(dir.path());
    let mut ids = vec![];
    for i in 0..5 {
        let record = create(&store, "scope", &format!("record-{i}"));
        store
            .apply_memory_embedding(
                "scope",
                &actor(),
                &attach(record.record_id, &format!("embed-{i}"), vec![1.0, 0.0, 0.0]),
            )
            .unwrap();
        ids.push(record.record_id);
    }
    ids.sort();
    let mut query = search();
    query.limit = 2;
    query.scan_limit = 2;
    let first = store.search_memory_semantic("scope", &query).unwrap();
    assert!(!first.exhaustive);
    assert_eq!(first.scanned_embeddings, 2);
    assert_eq!(
        first
            .hits
            .iter()
            .map(|h| h.record.record_id)
            .collect::<Vec<_>>(),
        ids[..2]
    );
    query.after = first.next_after;
    let second = store.search_memory_semantic("scope", &query).unwrap();
    assert_eq!(second.hits[0].record.record_id, ids[2]);
    query.after = second.next_after;
    let last = store.search_memory_semantic("scope", &query).unwrap();
    assert!(last.next_after.is_none());
    assert!(!last.exhaustive);
    assert_eq!(last.hits.len(), 1);
    let mut filtered = search();
    filtered.tag = Some("missing".into());
    assert!(
        store
            .search_memory_semantic("scope", &filtered)
            .unwrap()
            .hits
            .is_empty()
    );
    filtered.tag = None;
    filtered.episode_type = Some(EpisodeType::Decision);
    assert!(
        store
            .search_memory_semantic("scope", &filtered)
            .unwrap()
            .hits
            .is_empty()
    );
}
#[test]
fn semantic_expiry_is_checked_at_query_time_and_tiny_vectors_remain_valid() {
    let dir = TempDir::new().unwrap();
    let store = open(dir.path());
    let author = actor();
    let mut payload = input("expires");
    payload.valid_until_millis = Some(chrono::Utc::now().timestamp_millis() + 500);
    let receipt = store
        .apply_memory_command(
            "scope",
            &author,
            &MemoryCommand {
                contract_version: 1,
                idempotency_key: "expiring".into(),
                operation: MemoryOperation::Create { record: payload },
            },
        )
        .unwrap();
    store
        .apply_memory_embedding(
            "scope",
            &author,
            &attach(receipt.record_id, "tiny", vec![f32::MIN_POSITIVE, 0.0, 0.0]),
        )
        .unwrap();
    let mut query = search();
    query.vector = vec![f32::MAX, 0.0, 0.0];
    assert_eq!(
        store.search_memory_semantic("scope", &query).unwrap().hits[0].score,
        1.0
    );
    std::thread::sleep(std::time::Duration::from_millis(550));
    assert!(
        store
            .search_memory_semantic("scope", &query)
            .unwrap()
            .hits
            .is_empty()
    );
}

#[test]
fn semantic_competing_embeddings_commit_one_immutable_vector() {
    let dir = TempDir::new().unwrap();
    let store = std::sync::Arc::new(open(dir.path()));
    let record = create(&store, "scope", "content");
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let author = actor();
    let threads: Vec<_> = (0..2)
        .map(|i| {
            let store = store.clone();
            let barrier = barrier.clone();
            let author = author.clone();
            std::thread::spawn(move || {
                let vector = if i == 0 {
                    vec![1.0, 0.0, 0.0]
                } else {
                    vec![-1.0, 0.0, 0.0]
                };
                barrier.wait();
                store.apply_memory_embedding(
                    "scope",
                    &author,
                    &attach(record.record_id, &format!("competing-{i}"), vector),
                )
            })
        })
        .collect();
    let results: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    let page = store.search_memory_semantic("scope", &search()).unwrap();
    assert_eq!(page.hits.len(), 1);
    assert!(page.hits[0].score.abs() == 1.0);
    let mut query = search();
    query.vector = vec![-page.hits[0].score as f32, 0.0, 0.0];
    query.min_score = 0.0;
    assert!(
        store
            .search_memory_semantic("scope", &query)
            .unwrap()
            .hits
            .is_empty()
    );
}

#[test]
fn semantic_search_does_not_wait_for_the_memory_writer_lock() {
    let dir = TempDir::new().unwrap();
    let store = std::sync::Arc::new(open(dir.path()));
    let record = create(&store, "scope", "snapshot");
    store
        .apply_memory_embedding(
            "scope",
            &actor(),
            &attach(record.record_id, "vector", vec![1.0, 0.0, 0.0]),
        )
        .unwrap();
    let guard = store.mutation_lock.lock().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let reader = store.clone();
    let thread = std::thread::spawn(move || {
        tx.send(reader.search_memory_semantic("scope", &search()))
            .unwrap()
    });
    let result = rx.recv_timeout(std::time::Duration::from_secs(1));
    drop(guard);
    thread.join().unwrap();
    assert_eq!(
        result
            .expect("search must not acquire the writer lock")
            .unwrap()
            .hits
            .len(),
        1
    );
}

#[test]
fn semantic_snapshot_keeps_records_indexes_and_embeddings_at_one_revision() {
    let dir = TempDir::new().unwrap();
    let store = open(dir.path());
    let author = actor();
    let source = create(&store, "scope", "original");
    store
        .apply_memory_embedding(
            "scope",
            &author,
            &attach(source.record_id, "first", vec![1.0, 0.0, 0.0]),
        )
        .unwrap();
    let snapshot = store.memory_snapshot();
    store
        .apply_memory_command(
            "scope",
            &author,
            &MemoryCommand {
                contract_version: 1,
                idempotency_key: "update".into(),
                operation: MemoryOperation::Update {
                    record_id: source.record_id,
                    expected_revision: 1,
                    record: input("replacement"),
                },
            },
        )
        .unwrap();
    let mut replacement = attach(source.record_id, "second", vec![0.0, 1.0, 0.0]);
    replacement.record_revision = 2;
    store
        .apply_memory_embedding("scope", &author, &replacement)
        .unwrap();
    let old = snapshot.search_semantic("scope", &search()).unwrap();
    let new = store.search_memory_semantic("scope", &search()).unwrap();
    assert_eq!(old.hits[0].record.revision, 1);
    assert_eq!(old.hits[0].embedding.record_revision, 1);
    assert_eq!(old.hits[0].score, 1.0);
    assert_eq!(new.hits[0].record.revision, 2);
    assert_eq!(new.hits[0].embedding.record_revision, 2);
    assert_eq!(new.hits[0].score, 0.0);
}
