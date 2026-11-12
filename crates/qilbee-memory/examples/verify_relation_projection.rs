//! Actual stopped-RocksDB reproduction and acceptance for relation projections.
use qilbee_memory::{
    MemoryStorageConfig, RocksDbMemoryStorage,
    storage::platform::{
        MemoryCommand, MemoryRelationCommand, MemoryReviewCommand, RecordAuthor,
        TypedMemoryGraphQuery,
    },
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::Path;

const NAMESPACE: &str = "projection-coverage";

fn raw(path: &Path) -> rocksdb::DB {
    let options = rocksdb::Options::default();
    let families = rocksdb::DB::list_cf(&options, path).unwrap();
    rocksdb::DB::open_cf(&options, path, families).unwrap()
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
fn replace(path: &Path, key: &[u8], value: Option<&[u8]>) {
    let database = raw(path);
    let family = database.cf_handle("memory_agent_meta").unwrap();
    let mut options = rocksdb::WriteOptions::default();
    options.set_sync(true);
    match value {
        Some(value) => database.put_cf_opt(family, key, value, &options).unwrap(),
        None => database.delete_cf_opt(family, key, &options).unwrap(),
    }
}
fn read(path: &Path, key: &[u8]) -> Option<Vec<u8>> {
    let database = raw(path);
    database
        .get_cf(database.cf_handle("memory_agent_meta").unwrap(), key)
        .unwrap()
}
fn prefix(kind: u8, namespace: &str) -> Vec<u8> {
    let mut key = vec![kind];
    key.extend_from_slice(&(namespace.len() as u16).to_be_bytes());
    key.extend_from_slice(namespace.as_bytes());
    key
}
fn record_key(kind: u8, namespace: &str, id: uuid::Uuid) -> Vec<u8> {
    let mut key = prefix(kind, namespace);
    key.extend_from_slice(id.as_bytes());
    key
}
fn endpoint(kind: u8, namespace: &str, id: uuid::Uuid, revision: u64) -> Vec<u8> {
    let mut key = record_key(kind, namespace, id);
    key.extend_from_slice(&revision.to_be_bytes());
    key
}
fn head(kind: u8, namespace: &str, id: uuid::Uuid, revision: u64) -> Vec<u8> {
    let mut key = endpoint(0x56, namespace, id, revision);
    key.push(kind);
    key
}
fn adjacency(
    kind: u8,
    namespace: &str,
    id: uuid::Uuid,
    revision: u64,
    relation: uuid::Uuid,
) -> Vec<u8> {
    let mut key = endpoint(kind, namespace, id, revision);
    key.extend_from_slice(relation.as_bytes());
    key
}
fn empty_head(key: &[u8]) -> Vec<u8> {
    let mut prefix = key[..key.len() - 1].to_vec();
    prefix[0] = key[key.len() - 1];
    serde_json::to_vec(
        &json!({"schema_version":1,"prefix_digest":Sha256::digest(prefix).to_vec(),
        "count":0,"entries_digest":vec![0;32]}),
    )
    .unwrap()
}
fn verify(path: &Path, healthy: bool) -> u64 {
    let before = inventory(path);
    let store = RocksDbMemoryStorage::open_read_only(path).unwrap();
    let result = store.verify_memory_projections();
    let count = if healthy {
        result.unwrap().relation_head_entries
    } else {
        assert!(
            matches!(result, Err(qilbee_core::Error::DataCorruption(_))),
            "Offline audit accepted an inconsistent relation projection"
        );
        0
    };
    drop(store);
    assert_eq!(inventory(path), before, "Audit mutated logical records");
    count
}
fn fault(path: &Path, edits: &[(Vec<u8>, Option<Vec<u8>>)]) {
    let before = inventory(path);
    let old: Vec<_> = edits
        .iter()
        .map(|(key, _)| (key.clone(), read(path, key)))
        .collect();
    for (key, value) in edits {
        replace(path, key, value.as_deref());
    }
    verify(path, false);
    for (key, value) in old.into_iter().rev() {
        replace(path, &key, value.as_deref());
    }
    verify(path, true);
    assert_eq!(
        inventory(path),
        before,
        "Owned fault recovery changed retained bytes"
    );
}
fn open(path: &Path) -> RocksDbMemoryStorage {
    RocksDbMemoryStorage::open(MemoryStorageConfig {
        path: path.to_string_lossy().into_owned(),
        sync_writes: true,
        ..Default::default()
    })
    .unwrap()
}
fn author() -> RecordAuthor {
    RecordAuthor {
        credential_id: uuid::Uuid::new_v4(),
        subject_id: "projection-owner".into(),
    }
}
fn create(
    store: &RocksDbMemoryStorage,
    author: &RecordAuthor,
    namespace: &str,
    name: &str,
) -> uuid::Uuid {
    let command: MemoryCommand =
        serde_json::from_value(json!({"contract_version":1,"idempotency_key":name,
        "operation":{"type":"create","record":{"episode_type":"Observation","event_time_millis":0,
        "content":{"primary":"Disposable local relation verification fixture"}}}}))
        .unwrap();
    store
        .apply_memory_command(namespace, author, &command)
        .unwrap()
        .record_id
}
fn relation(
    store: &RocksDbMemoryStorage,
    author: &RecordAuthor,
    a: uuid::Uuid,
    b: uuid::Uuid,
    name: &str,
    expired: bool,
) -> uuid::Uuid {
    let command: MemoryRelationCommand = serde_json::from_value(json!({"contract_version":1,"idempotency_key":name,
        "operation":{"type":"assert","relation":{"source":{"record_id":a,"revision":1},
        "target":{"record_id":b,"revision":1},"kind":"supports","provenance":{"origin":"tool_observation",
        "method":"local-fixture","method_revision":"1","evidence_ref":"local:disposable-projection-fixture","model":null},
        "valid_from_millis":null,"valid_until_millis":if expired {Some(1)} else {None}}}})).unwrap();
    store
        .apply_memory_relation_command(NAMESPACE, author, &command)
        .unwrap()
        .relation_id
}
struct Fixture {
    author: RecordAuthor,
    a: uuid::Uuid,
    b: uuid::Uuid,
    c: uuid::Uuid,
    relation: uuid::Uuid,
}
fn fixture(path: &Path) -> Fixture {
    let store = open(path);
    let author = author();
    let a = create(&store, &author, NAMESPACE, "a");
    let b = create(&store, &author, NAMESPACE, "b");
    let c = create(&store, &author, NAMESPACE, "c");
    create(&store, &author, "second-scope", "other");
    let relation = relation(&store, &author, a, b, "assert", false);
    let query: TypedMemoryGraphQuery =
        serde_json::from_value(json!({"root_record_ids":[a]})).unwrap();
    let graph = store.read_memory_typed_graph(NAMESPACE, &query).unwrap();
    assert!(graph.coverage.complete);
    assert_eq!(graph.edges.len(), 1);
    drop(store);
    verify(path, true);
    Fixture {
        author,
        a,
        b,
        c,
        relation,
    }
}
fn corruptions(path: &Path) {
    let f = fixture(path);
    let h = head(0x54, NAMESPACE, f.a, 1);
    let h2 = head(0x55, NAMESPACE, f.b, 1);
    let tip = prefix(0x57, NAMESPACE);
    let relation = record_key(0x50, NAMESPACE, f.relation);
    let integrity = record_key(0x51, NAMESPACE, f.relation);
    let history = endpoint(0x52, NAMESPACE, f.relation, 1);
    let outgoing = adjacency(0x54, NAMESPACE, f.a, 1, f.relation);
    let incoming = adjacency(0x55, NAMESPACE, f.b, 1, f.relation);
    for key in [
        &h, &h2, &tip, &relation, &integrity, &history, &outgoing, &incoming,
    ] {
        fault(path, &[(key.clone(), None)]);
    }
    let original_head: Value = serde_json::from_slice(&read(path, &h).unwrap()).unwrap();
    for field in [
        "schema_version",
        "prefix_digest",
        "count",
        "entries_digest",
        "unknown",
    ] {
        let mut changed = original_head.clone();
        match field {
            "schema_version" => changed[field] = json!(2),
            "prefix_digest" | "entries_digest" => {
                changed[field][0] = json!((changed[field][0].as_u64().unwrap() + 1) % 256)
            }
            "count" => changed[field] = json!(u64::MAX),
            _ => changed[field] = json!(true),
        }
        fault(
            path,
            &[(h.clone(), Some(serde_json::to_vec(&changed).unwrap()))],
        );
    }
    fault(path, &[(h.clone(), Some(vec![b' '; 1025]))]);
    let mut trailing_head = h.clone();
    trailing_head.push(0);
    fault(path, &[(trailing_head, Some(read(path, &h).unwrap()))]);
    let mut short = h.clone();
    short.pop();
    fault(path, &[(short, Some(read(path, &h).unwrap()))]);
    let mut invalid_utf8 = h.clone();
    invalid_utf8[3] = 255;
    fault(path, &[(invalid_utf8, Some(read(path, &h).unwrap()))]);
    let mut bad_kind = h.clone();
    *bad_kind.last_mut().unwrap() = 0x10;
    fault(path, &[(bad_kind, Some(read(path, &h).unwrap()))]);
    for key in [
        head(0x54, NAMESPACE, f.a, 0),
        head(0x54, NAMESPACE, f.a, 2),
        head(0x54, NAMESPACE, uuid::Uuid::new_v4(), 1),
        head(0x54, "orphan-scope", f.a, 1),
        head(0x54, "", f.a, 1),
    ] {
        let value = empty_head(&key);
        fault(path, &[(key, Some(value))]);
    }
    fault(path, &[(tip.clone(), Some(vec![0; 32]))]);
    fault(
        path,
        &[(
            prefix(0x57, "orphan-scope"),
            Some(Sha256::digest([]).to_vec()),
        )],
    );
    let mut trailing_tip = tip.clone();
    trailing_tip.push(0);
    fault(path, &[(trailing_tip, Some(read(path, &tip).unwrap()))]);
    let mut changed = read(path, &outgoing).unwrap();
    changed[0] ^= 1;
    fault(path, &[(outgoing.clone(), Some(changed))]);
    fault(
        path,
        &[
            (outgoing.clone(), None),
            (incoming.clone(), None),
            (h.clone(), Some(empty_head(&h))),
            (h2.clone(), Some(empty_head(&h2))),
        ],
    );
    let orphan = adjacency(0x54, NAMESPACE, f.a, 1, uuid::Uuid::new_v4());
    let value = read(path, &integrity).unwrap();
    let mut forged = original_head.clone();
    forged["count"] = json!(2);
    let leaf = Sha256::digest([orphan.as_slice(), value.as_slice()].concat());
    for (index, byte) in leaf.iter().enumerate() {
        forged["entries_digest"][index] =
            json!(original_head["entries_digest"][index].as_u64().unwrap() as u8 ^ *byte);
    }
    fault(
        path,
        &[
            (orphan, Some(value.clone())),
            (h.clone(), Some(serde_json::to_vec(&forged).unwrap())),
        ],
    );
    fault(
        path,
        &[(
            adjacency(0x55, "orphan-scope", f.b, 1, f.relation),
            Some(value.clone()),
        )],
    );
    let mut trailing = outgoing.clone();
    trailing.push(0);
    fault(path, &[(trailing, Some(value.clone()))]);
    fault(
        path,
        &[(
            record_key(0x51, NAMESPACE, uuid::Uuid::new_v4()),
            Some(value),
        )],
    );
    for key in [&relation, &integrity, &history] {
        fault(path, &[(key.clone(), Some(vec![0xff]))]);
        let mut extra = key.clone();
        extra.push(0);
        // Current relation/integrity projections are audited; full historic
        // ledger enumeration remains outside this projection contract.
        if key[0] != 0x52 {
            fault(path, &[(extra, Some(read(path, key).unwrap()))]);
        }
    }
    let mut wrong: Value = serde_json::from_slice(&read(path, &relation).unwrap()).unwrap();
    wrong["relation_id"] = json!(uuid::Uuid::new_v4());
    fault(
        path,
        &[(relation, Some(serde_json::to_vec(&wrong).unwrap()))],
    );
    println!("{{\"relation_heads_tips_rows_integrity_faults_and_recovery\":\"passed\"}}");
}
fn lifecycle(path: &Path) {
    let f = fixture(path);
    let store = open(path);
    let parallel = relation(&store, &f.author, f.a, f.b, "parallel-expired", true);
    let mut serial = 0;
    let mut operation = |id: uuid::Uuid, revision: u64, kind: &str, disposition: Option<&str>| {
        serial += 1;
        let mut action = json!({"type":kind,"relation_id":id,"expected_revision":revision,"evidence_ref":"local:disposable-lifecycle"});
        if let Some(disposition) = disposition {
            action["disposition"] = json!(disposition);
        }
        let command: MemoryRelationCommand=serde_json::from_value(json!({"contract_version":1,"idempotency_key":format!("life-{serial}"),"operation":action})).unwrap();
        store
            .apply_memory_relation_command(NAMESPACE, &f.author, &command)
            .unwrap();
        store.verify_memory_projections().unwrap();
    };
    operation(f.relation, 1, "review", Some("rejected"));
    operation(parallel, 1, "retire", None);
    operation(parallel, 2, "restore", None);
    operation(f.relation, 2, "review", Some("approved"));
    drop(operation);
    for (name, id, deleted) in [
        ("move-source", f.a, false),
        ("delete-target", f.b, true),
        ("move-unused", f.c, false),
    ] {
        let mut action = json!({"type":if deleted {"delete"} else {"update"},"record_id":id,"expected_revision":1});
        if !deleted {
            action["record"] = json!({"episode_type":"Observation","event_time_millis":0,"content":{"primary":"Disposable updated fixture"}});
        }
        let command: MemoryCommand = serde_json::from_value(
            json!({"contract_version":1,"idempotency_key":name,"operation":action}),
        )
        .unwrap();
        store
            .apply_memory_command(NAMESPACE, &f.author, &command)
            .unwrap();
        store.verify_memory_projections().unwrap();
    }
    let query: TypedMemoryGraphQuery =
        serde_json::from_value(json!({"root_record_ids":[f.a]})).unwrap();
    let graph = store.read_memory_typed_graph(NAMESPACE, &query).unwrap();
    assert!(graph.coverage.complete);
    assert!(graph.edges.is_empty());
    drop(store);
    let before = verify(path, true);
    let old_key = head(0x54, NAMESPACE, f.c, 1);
    let old = read(path, &old_key).unwrap();
    replace(path, &prefix(0x57, NAMESPACE), None);
    // The real startup rebuild intentionally drops unreferenced historical empty heads.
    drop(open(path));
    let rebuilt = verify(path, true);
    assert!(rebuilt < before);
    assert!(read(path, &old_key).is_none());
    replace(path, &old_key, Some(&old));
    verify(path, true);
    println!("{{\"real_relation_lifecycle_historical_heads_and_startup_rebuild\":\"passed\"}}");
}
fn copy_directory(source: &Path, target: &Path) {
    std::fs::create_dir_all(target).unwrap();
    for row in std::fs::read_dir(source).unwrap() {
        let row = row.unwrap();
        let target = target.join(row.file_name());
        if row.file_type().unwrap().is_dir() {
            copy_directory(&row.path(), &target);
        } else {
            assert!(row.file_type().unwrap().is_file());
            std::fs::copy(row.path(), target).unwrap();
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
    let source = root.join("operator-source");
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
    let f = fixture(&memory);
    let before = inventory(&memory);
    let healthy = operator(binary, &source);
    assert!(
        healthy.status.success(),
        "Actual healthy operator failed: {}",
        String::from_utf8_lossy(&healthy.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&healthy.stdout).unwrap()["status"],
        "verified"
    );
    let key = head(0x54, NAMESPACE, f.a, 1);
    let value = read(&memory, &key).unwrap();
    replace(&memory, &key, None);
    let corrupt = inventory(&memory);
    let failed = operator(binary, &source);
    assert!(!failed.status.success());
    assert!(failed.stdout.is_empty());
    assert_eq!(inventory(&memory), corrupt);
    replace(&memory, &key, Some(&value));
    assert_eq!(inventory(&memory), before);
    assert!(operator(binary, &source).status.success());
    let copy = root.join("operator-copy");
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
        "Actual source/copy comparison failed: {}",
        String::from_utf8_lossy(&compared.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&compared.stdout).unwrap()["status"],
        "verified_equal"
    );
    println!("{{\"actual_operator_relation_failure_recovery_and_copy\":\"passed\"}}");
}
fn baseline(path: &Path) {
    let store = RocksDbMemoryStorage::open(MemoryStorageConfig {
        path: path.to_string_lossy().into_owned(),
        sync_writes: true,
        ..Default::default()
    })
    .unwrap();
    let author = RecordAuthor {
        credential_id: uuid::Uuid::new_v4(),
        subject_id: "projection-owner".into(),
    };
    let command: MemoryCommand = serde_json::from_value(serde_json::json!({
        "contract_version":1,"idempotency_key":"relation-projection-fixture",
        "operation":{"type":"create","record":{"episode_type":"Observation",
            "event_time_millis":0,"content":{"primary":"Disposable local relation projection fixture"}}}
    })).unwrap();
    let receipt = store
        .apply_memory_command("projection-coverage", &author, &command)
        .unwrap();
    let query: TypedMemoryGraphQuery = serde_json::from_value(serde_json::json!({
        "root_record_ids":[receipt.record_id],"max_depth":2
    }))
    .unwrap();
    assert!(
        store
            .read_memory_typed_graph("projection-coverage", &query)
            .unwrap()
            .coverage
            .complete
    );
    assert_eq!(
        store
            .verify_memory_projections()
            .unwrap()
            .relation_head_entries,
        2
    );
    drop(store);
    let before = inventory(path);
    let (key, value) = {
        let database = raw(path);
        let family = database.cf_handle("memory_agent_meta").unwrap();
        let (key, value) = database
            .iterator_cf(
                family,
                rocksdb::IteratorMode::From(&[0x56], rocksdb::Direction::Forward),
            )
            .next()
            .unwrap()
            .unwrap();
        assert_eq!(key[0], 0x56);
        (key.to_vec(), value.to_vec())
    };
    replace(path, &key, None);
    let corrupt = inventory(path);
    let store = RocksDbMemoryStorage::open_read_only(path).unwrap();
    let graph = store.read_memory_typed_graph("projection-coverage", &query);
    assert!(
        matches!(graph, Err(qilbee_core::Error::DataCorruption(_))),
        "The real typed graph did not refuse its missing required head"
    );
    let audit = store.verify_memory_projections();
    drop(store);
    assert_eq!(
        inventory(path),
        corrupt,
        "Read-only auditing changed stored records"
    );
    // Recover original owned bytes before the regression assertion can fail.
    replace(path, &key, Some(&value));
    assert_eq!(inventory(path), before);
    let recovered = RocksDbMemoryStorage::open_read_only(path).unwrap();
    recovered.verify_memory_projections().unwrap();
    assert!(
        recovered
            .read_memory_typed_graph("projection-coverage", &query)
            .unwrap()
            .coverage
            .complete
    );
    println!(
        "{}",
        serde_json::json!({"actual_typed_graph_missing_head_refused":true,
        "offline_audit_accepted_missing_head":audit.is_ok(),"original_bytes_and_graph_recovered":true,
        "read_only_inventories_unchanged":true})
    );
    assert!(
        matches!(audit, Err(qilbee_core::Error::DataCorruption(_))),
        "Offline verification accepted a missing required relation adjacency head"
    );
}
fn reviewed_memory(path: &Path) {
    let store = open(path);
    let author = author();
    let id = create(&store, &author, NAMESPACE, "reviewed-record");
    let query: TypedMemoryGraphQuery =
        serde_json::from_value(json!({"root_record_ids":[id]})).unwrap();
    assert!(
        store
            .read_memory_typed_graph(NAMESPACE, &query)
            .unwrap()
            .coverage
            .complete
    );
    let command: MemoryReviewCommand = serde_json::from_value(json!({"contract_version":1,
        "idempotency_key":"review-memory","record_id":id,"expected_revision":1,
        "disposition":"approved","evidence_ref":"local:disposable-memory-review"}))
    .unwrap();
    let receipt = store
        .review_memory_record(NAMESPACE, &author, &command)
        .unwrap();
    assert_eq!(receipt.revision, 2);
    let graph = store.read_memory_typed_graph(NAMESPACE, &query);
    println!(
        "{}",
        json!({"memory_review_acknowledged_revision":receipt.revision,
        "graph_read_after_review_succeeded":graph.is_ok()})
    );
    assert!(
        graph.unwrap().coverage.complete,
        "A committed review left its graph projection incomplete"
    );
    assert_eq!(
        store
            .review_memory_record(NAMESPACE, &author, &command)
            .unwrap(),
        receipt
    );
    drop(store);
    verify(path, true);
    let before = inventory(path);
    let read_only = RocksDbMemoryStorage::open_read_only(path).unwrap();
    let failed_review: MemoryReviewCommand = serde_json::from_value(json!({"contract_version":1,
        "idempotency_key":"read-only-review","record_id":id,"expected_revision":2,
        "disposition":"unreviewed","evidence_ref":"local:owned-read-only-failure"}))
    .unwrap();
    assert!(
        read_only
            .review_memory_record(NAMESPACE, &author, &failed_review)
            .is_err()
    );
    assert!(
        read_only
            .read_memory_typed_graph(NAMESPACE, &query)
            .unwrap()
            .coverage
            .complete
    );
    drop(read_only);
    assert_eq!(inventory(path), before);
    let reopened = open(path);
    assert_eq!(
        reopened
            .review_memory_record(NAMESPACE, &author, &failed_review)
            .unwrap()
            .revision,
        3
    );
    assert!(
        reopened
            .read_memory_typed_graph(NAMESPACE, &query)
            .unwrap()
            .coverage
            .complete
    );
    drop(reopened);
    verify(path, true);
    println!("{{\"reviewed_memory_heads_idempotency_and_reopen\":\"passed\"}}");
}
fn main() {
    let directory = tempfile::tempdir().unwrap();
    baseline(&directory.path().join("baseline"));
    reviewed_memory(&directory.path().join("memory-review"));
    corruptions(&directory.path().join("faults"));
    lifecycle(&directory.path().join("lifecycle"));
    let empty = directory.path().join("empty");
    drop(open(&empty));
    assert_eq!(verify(&empty, true), 0);
    if let Some(binary) = std::env::args_os().nth(1) {
        operator_acceptance(Path::new(&binary), directory.path());
    }
}
