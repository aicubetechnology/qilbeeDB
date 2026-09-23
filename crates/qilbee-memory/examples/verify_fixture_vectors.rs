//! Offline read-only audit of captured fixture vectors in an operator-owned laboratory.
use qilbee_memory::storage::platform::{
    CompanyMemoryAddress, EmbeddingReceipt, MemoryResourceScope, MemoryVisibility,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::Path,
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Deserialize)]
struct State {
    fixture_sha256: String,
    tenant_id: String,
    subject_id: String,
    scope: MemoryResourceScope,
    documents: BTreeMap<String, Entry>,
}
#[derive(Deserialize)]
struct Entry {
    record_id: uuid::Uuid,
    revision: u64,
    embedding: EmbeddingReceipt,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Stored {
    schema_version: u32,
    namespace: String,
    receipt: EmbeddingReceipt,
    vector: Vec<f32>,
}
fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn vector_hash(vector: &[f32]) -> Result<String> {
    if vector.is_empty()
        || vector.iter().any(|v| !v.is_finite())
        || !vector.iter().any(|v| *v != 0.)
    {
        return Err("Invalid stored vector".into());
    }
    Ok(hash(
        &vector
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>(),
    ))
}
fn run(database: &Path, state_path: &Path, manifest_path: &Path, output: &Path) -> Result<()> {
    if !database.is_dir() || output.exists() {
        return Err("Existing database and fresh output path required".into());
    }
    let state: State = serde_json::from_slice(&fs::read(state_path)?)?;
    let manifest: Value = serde_json::from_slice(&fs::read(manifest_path)?)?;
    let subject = match state.scope.visibility {
        MemoryVisibility::Private => Some(state.subject_id.as_str()),
        MemoryVisibility::Shared => None,
    };
    let namespace =
        CompanyMemoryAddress::new(&state.tenant_id, &state.scope, subject)?.namespace()?;
    let entries = manifest["entries"]
        .as_array()
        .ok_or("Missing manifest entries")?;
    let expected: BTreeMap<_, _> = entries
        .iter()
        .filter(|e| e["role"] == "documents")
        .map(|e| Ok((e["source_id"].as_str().ok_or("Missing source ID")?, e)))
        .collect::<Result<_>>()?;
    if expected.len() != state.documents.len()
        || expected.len() != entries.iter().filter(|e| e["role"] == "documents").count()
    {
        return Err("Document inventory differs or contains duplicate IDs".into());
    }
    let record_ids: BTreeSet<_> = state
        .documents
        .values()
        .map(|entry| entry.record_id)
        .collect();
    if record_ids.len() != state.documents.len() {
        return Err("Multiple fixture documents reference the same stored record".into());
    }
    let options = rocksdb::Options::default();
    let families = rocksdb::DB::list_cf(&options, database)?;
    let db = rocksdb::DB::open_cf_for_read_only(&options, database, families, false)?;
    let cf = db
        .cf_handle("memory_episode_index")
        .ok_or("Missing embedding column family")?;
    let mut verified = Vec::new();
    for (id, entry) in &state.documents {
        let expected = expected.get(id.as_str()).ok_or("Unexpected document ID")?;
        let space = &entry.embedding.space;
        if space.provider != manifest["provider"].as_str().ok_or("Missing provider")?
            || space.model != manifest["model"].as_str().ok_or("Missing model")?
            || space.dimensions as u64
                != manifest["dimensions"]
                    .as_u64()
                    .ok_or("Missing dimensions")?
            || entry.record_id != entry.embedding.record_id
            || entry.revision == 0
            || entry.revision != entry.embedding.record_revision
        {
            return Err("Manifest, record or embedding identity differs".into());
        }
        let mut key = vec![0x13];
        key.extend_from_slice(&u16::try_from(namespace.len())?.to_be_bytes());
        key.extend_from_slice(namespace.as_bytes());
        key.extend_from_slice(&Sha256::digest(serde_json::to_vec(space)?));
        key.extend_from_slice(entry.record_id.as_bytes());
        let raw = db.get_cf(cf, key)?.ok_or("Stored embedding missing")?;
        let stored: Stored = serde_json::from_slice(&raw)?;
        if stored.schema_version != 1
            || stored.namespace != namespace
            || stored.receipt != entry.embedding
            || stored.receipt.contract_version != 1
            || stored.vector.len() != space.dimensions
            || hash(&serde_json::to_vec(&stored.vector)?) != stored.receipt.vector_digest
        {
            return Err("Stored embedding integrity differs".into());
        }
        let actual = vector_hash(&stored.vector)?;
        if actual
            != expected["float32_le_sha256"]
                .as_str()
                .ok_or("Missing expected vector hash")?
        {
            return Err("Stored float32 values differ from captured vector".into());
        }
        verified.push(json!({"source_id":id,"record_id":entry.record_id,
            "record_revision":entry.revision,"float32_le_sha256":actual}));
    }
    if verified.is_empty() {
        return Err("Empty fixture cannot establish readback".into());
    }
    let result = json!({"status":"verified_offline_database_readback",
        "fixture_sha256":state.fixture_sha256,"state_file_sha256":hash(&fs::read(state_path)?),
        "manifest_file_sha256":hash(&fs::read(manifest_path)?),"documents":verified,
        "query_vectors_persisted":false,"retrieval_executed":false});
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    serde_json::to_writer_pretty(&mut file, &result)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    println!("Verified {} stored document vectors", state.documents.len());
    Ok(())
}
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 4 {
        return Err(
            "Usage: verify_fixture_vectors CLOSED_MEMORY_DB STATE MANIFEST NEW_RECEIPT".into(),
        );
    }
    run(
        Path::new(&args[0]),
        Path::new(&args[1]),
        Path::new(&args[2]),
        Path::new(&args[3]),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_reopened_storage_and_rejects_changed_expectations() {
        use qilbee_memory::storage::platform::{
            EmbeddingCommand, EmbeddingSpace, MemoryCommand, MemoryOperation, RecordAuthor,
            RecordInput,
        };
        use qilbee_memory::{
            EpisodeContent, EpisodeType, MemoryStorageConfig, RocksDbMemoryStorage,
        };
        let dir = tempfile::tempdir().unwrap();
        let database = dir.path().join("memory");
        let scope = MemoryResourceScope {
            project_id: "project".into(),
            mission_id: None,
            agent_id: "agent".into(),
            visibility: MemoryVisibility::Private,
        };
        let namespace = CompanyMemoryAddress::new("company", &scope, Some("subject"))
            .unwrap()
            .namespace()
            .unwrap();
        let store =
            RocksDbMemoryStorage::open(MemoryStorageConfig::for_testing(&database)).unwrap();
        let author = RecordAuthor {
            credential_id: uuid::Uuid::new_v4(),
            subject_id: "subject".into(),
        };
        let record = store
            .apply_memory_command(
                &namespace,
                &author,
                &MemoryCommand {
                    contract_version: 1,
                    idempotency_key: "create".into(),
                    operation: MemoryOperation::Create {
                        record: RecordInput {
                            episode_type: EpisodeType::Observation,
                            content: EpisodeContent::new("Synthetic evidence"),
                            event_time_millis: 1700000000000,
                            valid_until_millis: None,
                            tags: vec![],
                            metadata: Default::default(),
                        },
                    },
                },
            )
            .unwrap();
        let space = EmbeddingSpace {
            provider: "fixture".into(),
            model: "fixture".into(),
            revision: "fixture-v1".into(),
            dimensions: 2,
        };
        let receipt = store
            .apply_memory_embedding(
                &namespace,
                &author,
                &EmbeddingCommand {
                    contract_version: 1,
                    idempotency_key: "embedding".into(),
                    record_id: record.record_id,
                    record_revision: record.revision,
                    space,
                    vector: vec![0.5, 0.25],
                },
            )
            .unwrap();
        drop(store);
        let state_path = dir.path().join("state.json");
        let state = json!({"fixture_sha256":"fixture", "tenant_id":"company", "subject_id":"subject",
            "scope":scope,"documents":{"doc":{"record_id":record.record_id,
                "revision":record.revision,"embedding":receipt}}});
        fs::write(&state_path, serde_json::to_vec(&state).unwrap()).unwrap();
        let manifest_path = dir.path().join("manifest.json");
        let mut manifest = json!({"provider":"fixture","model":"fixture","dimensions":2,
            "entries":[{"role":"documents","source_id":"doc","float32_le_sha256":vector_hash(&[0.5,0.25]).unwrap()}]});
        fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        let output = dir.path().join("receipt.json");
        run(&database, &state_path, &manifest_path, &output).unwrap();
        assert!(run(&database, &state_path, &manifest_path, &output).is_err());
        manifest["entries"][0]["float32_le_sha256"] = json!("wrong");
        fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        let failed_output = dir.path().join("failure.json");
        assert!(run(&database, &state_path, &manifest_path, &failed_output).is_err());
        assert!(!failed_output.exists());
        let mut wrong_scope = state;
        wrong_scope["subject_id"] = json!("another-subject");
        fs::write(&state_path, serde_json::to_vec(&wrong_scope).unwrap()).unwrap();
        assert!(run(&database, &state_path, &manifest_path, &failed_output).is_err());
    }
    #[test]
    fn hashes_float32_bits_and_rejects_invalid_values() {
        assert_eq!(
            vector_hash(&[0.5, 0.25]).unwrap(),
            hash(&[0, 0, 0, 63, 0, 0, 128, 62])
        );
        for values in [vec![], vec![0.0, -0.0], vec![f32::NAN], vec![f32::INFINITY]] {
            assert!(vector_hash(&values).is_err());
        }
        assert_ne!(
            vector_hash(&[1., 0.]).unwrap(),
            vector_hash(&[1., -0.]).unwrap()
        );
    }
}
