//! Versioned memory commands and receipts on the existing RocksDB backend.

use super::RocksDbMemoryStorage;
use crate::{EpisodeContent, EpisodeType};
use qilbee_core::{Error, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordInput {
    pub episode_type: EpisodeType,
    pub content: EpisodeContent,
    pub event_time_millis: i64,
    pub valid_until_millis: Option<i64>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub metadata: std::collections::BTreeMap<String, serde_json::Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum MemoryOperation {
    Create {
        record: RecordInput,
    },
    Derive {
        record: RecordInput,
        derivation: MemoryDerivation,
    },
    Update {
        record_id: Uuid,
        expected_revision: u64,
        record: RecordInput,
    },
    Delete {
        record_id: Uuid,
        expected_revision: u64,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryCommand {
    pub contract_version: u32,
    pub idempotency_key: String,
    pub operation: MemoryOperation,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordAuthor {
    pub credential_id: Uuid,
    pub subject_id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryRecord {
    pub schema_version: u32,
    pub record_id: Uuid,
    pub revision: u64,
    pub created_at_millis: i64,
    pub modified_at_millis: i64,
    pub author: RecordAuthor,
    pub payload: Option<RecordInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<MemoryReview>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derivation: Option<MemoryDerivation>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandReceipt {
    pub contract_version: u32,
    pub idempotency_key: String,
    pub record_id: Uuid,
    pub revision: u64,
    pub action: String,
    pub committed_at_millis: i64,
    pub author: RecordAuthor,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryQuery {
    pub limit: usize,
    #[serde(default = "default_query_scan_limit")]
    pub scan_limit: usize,
    pub after: Option<Uuid>,
    pub text_contains: Option<String>,
    pub episode_type: Option<EpisodeType>,
    pub tag: Option<String>,
}
fn default_query_scan_limit() -> usize {
    10_000
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryPage {
    pub records: Vec<MemoryRecord>,
    pub next_after: Option<Uuid>,
    pub scanned_records: usize,
    #[serde(default)]
    pub dependency_work: DependencyWork,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredReceipt {
    schema_version: u32,
    request_digest: [u8; 32],
    receipt: CommandReceipt,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecordIndex {
    schema_version: u32,
    revision: u64,
    record_digest: [u8; 32],
}

impl RocksDbMemoryStorage {
    /// Commit record, index and receipt together. Callers provide an authorized
    /// namespace and trusted author; this low-level library does not authenticate.
    /// This operation is blocking and always uses synchronous WAL writes.
    pub fn apply_memory_command(
        &self,
        namespace: &str,
        author: &RecordAuthor,
        command: &MemoryCommand,
    ) -> Result<CommandReceipt> {
        self.apply_memory_command_with_observation(namespace, author, command, None)
    }

    /// Atomically register the external company-agent association with the first
    /// successful memory command, its canonical record, journal and receipt.
    pub fn apply_observed_memory_command(
        &self,
        observation: &AgentObservation,
        command: &MemoryCommand,
    ) -> Result<CommandReceipt> {
        observation.validate()?;
        self.apply_memory_command_with_observation(&observation.namespace, &observation.author, command, Some(observation))
    }

    fn apply_memory_command_with_observation(
        &self,
        namespace: &str,
        author: &RecordAuthor,
        command: &MemoryCommand,
        observation: Option<&AgentObservation>,
    ) -> Result<CommandReceipt> {
        Self::validate_agent(namespace)?;
        if command.contract_version != 1
            || command.idempotency_key.trim().is_empty()
            || command.idempotency_key.len() > 256
            || command.idempotency_key.chars().any(char::is_control)
            || author.subject_id.trim().is_empty()
            || author.subject_id.len() > 256
        {
            return Err(Error::ValidationError(
                "Invalid memory command identity or contract version".into(),
            ));
        }
        let mut receipt_key = vec![0x12];
        receipt_key.extend(encode(&(
            namespace,
            &author.subject_id,
            &command.idempotency_key,
        ))?);
        let request_digest = digest(&encode(command)?);
        let _guard = self
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Memory mutation lock poisoned".into()))?;
        let receipts = self.cf(super::cf::AGENT_META)?;
        if let Some(bytes) = self
            .db
            .get_cf(receipts, &receipt_key)
            .map_err(storage_error)?
        {
            let stored: StoredReceipt = decode(&bytes)?;
            if stored.schema_version != 1 || stored.receipt.contract_version != 1 {
                return Err(inconsistent());
            }
            if stored.request_digest != request_digest {
                return Err(Error::ConstraintViolation(
                    "Idempotency key was already used for a different command".into(),
                ));
            }
            if let Some(observation) = observation {
                // An acknowledged command from before registration support may
                // be the first successful observation after upgrade.
                let mut batch = rocksdb::WriteBatch::default();
                self.append_agent_observation(observation, AgentRegistrationTrigger::MemoryCommand { record_id: stored.receipt.record_id, revision: stored.receipt.revision, action: stored.receipt.action.clone() }, chrono::Utc::now().timestamp_millis(), &mut batch)?;
                if batch.len() > 0 {
                    let mut options = rocksdb::WriteOptions::default(); options.disable_wal(false); options.set_sync(true);
                    self.db.write_opt(batch, &options).map_err(storage_error)?;
                }
            }
            return Ok(stored.receipt);
        }
        let now = chrono::Utc::now().timestamp_millis();
        let (record, action) = match &command.operation {
            MemoryOperation::Create { record } | MemoryOperation::Derive { record, .. } => {
                validate_input(record)?;
                let derivation =
                    if let MemoryOperation::Derive { derivation, .. } = &command.operation {
                        self.memory_snapshot()
                            .validate_derivation_sources(namespace, derivation)?;
                        Some(derivation.clone())
                    } else {
                        None
                    };
                let id = Uuid::new_v4();
                if self.platform_record_locked(namespace, id)?.is_some() {
                    return Err(Error::TransactionConflict(
                        "Record identifier collision".into(),
                    ));
                }
                (
                    MemoryRecord {
                        schema_version: 1,
                        record_id: id,
                        revision: 1,
                        created_at_millis: now,
                        modified_at_millis: now,
                        author: author.clone(),
                        payload: Some(record.clone()),
                        review: None,
                        derivation,
                    },
                    if matches!(command.operation, MemoryOperation::Derive { .. }) {
                        "derived"
                    } else {
                        "created"
                    },
                )
            }
            MemoryOperation::Update {
                record_id,
                expected_revision,
                record,
            } => {
                validate_input(record)?;
                let mut current =
                    self.platform_record_for_write(namespace, *record_id, *expected_revision)?;
                if current.derivation.is_some() {
                    return Err(Error::ConstraintViolation(
                        "Derived content is immutable; create a new derivation".into(),
                    ));
                }
                current.payload = Some(record.clone());
                current.review = None;
                current.author = author.clone();
                current.modified_at_millis = now;
                (current, "updated")
            }
            MemoryOperation::Delete {
                record_id,
                expected_revision,
            } => {
                let mut current =
                    self.platform_record_for_write(namespace, *record_id, *expected_revision)?;
                current.payload = None;
                current.review = None;
                current.author = author.clone();
                current.modified_at_millis = now;
                (current, "deleted")
            }
        };
        let receipt = CommandReceipt {
            contract_version: 1,
            idempotency_key: command.idempotency_key.clone(),
            record_id: record.record_id,
            revision: record.revision,
            action: action.into(),
            committed_at_millis: now,
            author: author.clone(),
        };
        let encoded = encode(&record)?;
        let index = RecordIndex {
            schema_version: 1,
            revision: record.revision,
            record_digest: digest(&encoded),
        };
        let mut batch = rocksdb::WriteBatch::default();
        if let Some(observation) = observation {
            self.append_agent_observation(observation, AgentRegistrationTrigger::MemoryCommand { record_id: receipt.record_id, revision: receipt.revision, action: receipt.action.clone() }, now, &mut batch)?;
        }
        self.update_candidates(namespace, &record, &mut batch)?;
        self.append_empty_relation_heads(namespace, &record, &mut batch)?;
        batch.put_cf(
            self.cf(super::cf::EPISODES)?,
            record_key(0x10, namespace, record.record_id),
            encoded,
        );
        batch.put_cf(
            self.cf(super::cf::EPISODE_INDEX)?,
            record_key(0x11, namespace, record.record_id),
            encode(&index)?,
        );
        batch.put_cf(
            receipts,
            receipt_key,
            encode(&StoredReceipt {
                schema_version: 1,
                request_digest,
                receipt: receipt.clone(),
            })?,
        );
        let kind = match &command.operation {
            MemoryOperation::Create { .. } => MemoryChangeKind::Created,
            MemoryOperation::Derive { .. } => MemoryChangeKind::Derived,
            MemoryOperation::Update { .. } => MemoryChangeKind::Updated,
            MemoryOperation::Delete { .. } => MemoryChangeKind::Deleted,
        };
        self.append_memory_change(
            namespace,
            &mut batch,
            kind,
            record.record_id,
            record.revision,
            author,
            now,
        )?;
        let mut options = rocksdb::WriteOptions::default();
        options.disable_wal(false);
        options.set_sync(true);
        self.db.write_opt(batch, &options).map_err(storage_error)?;
        Ok(receipt)
    }

    /// Return only current, non-deleted, unexpired records, checking index integrity.
    pub fn read_memory_record(&self, namespace: &str, id: Uuid) -> Result<Option<MemoryRecord>> {
        Self::validate_agent(namespace)?;
        let _guard = self
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Memory mutation lock poisoned".into()))?;
        let snapshot = self.memory_snapshot();
        match snapshot.record(namespace, id)? {
            Some(record) if snapshot.eligible(namespace, &record)? => Ok(Some(record)),
            _ => Ok(None),
        }
    }

    /// Ordered UUID scan with explicit continuation and honest substring filtering.
    /// This is not a semantic search, relevance score, or durable change cursor.
    pub fn query_memory_records(&self, namespace: &str, query: &MemoryQuery) -> Result<QueryPage> {
        Self::validate_agent(namespace)?;
        if !(1..=1000).contains(&query.limit) || !(1..=10_000).contains(&query.scan_limit) {
            return Err(Error::ValidationError(
                "Query limit must be in 1..=1000 and scan limit in 1..=10000".into(),
            ));
        }
        let _guard = self
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Memory mutation lock poisoned".into()))?;
        let prefix = record_prefix(0x10, namespace);
        let start = query
            .after
            .map(|id| record_key(0x10, namespace, id))
            .unwrap_or_else(|| prefix.clone());
        let mut page = QueryPage {
            records: vec![],
            next_after: None,
            scanned_records: 0,
            dependency_work: DependencyWork::default(),
        };
        let snapshot = self.memory_snapshot();
        let needle = query.text_contains.as_ref().map(|text| text.to_lowercase());
        for item in self.db.iterator_cf(
            self.cf(super::cf::EPISODES)?,
            rocksdb::IteratorMode::From(&start, rocksdb::Direction::Forward),
        ) {
            let (key, _) = item.map_err(storage_error)?;
            if !key.starts_with(&prefix) {
                break;
            }
            let id = Uuid::from_slice(&key[prefix.len()..]).map_err(|_| inconsistent())?;
            if query.after.is_some_and(|after| id <= after) {
                continue;
            }
            let record = self
                .platform_record_locked(namespace, id)?
                .ok_or_else(inconsistent)?;
            page.scanned_records += 1;
            if snapshot.eligible(namespace, &record)? {
                let input = record
                    .payload
                    .as_ref()
                    .expect("visible records have content");
                let matches = query
                    .episode_type
                    .as_ref()
                    .is_none_or(|kind| kind == &input.episode_type)
                    && query
                        .tag
                        .as_ref()
                        .is_none_or(|tag| input.tags.contains(tag))
                    && needle.as_ref().is_none_or(|text| {
                        [
                            &input.content.primary,
                            input.content.secondary.as_deref().unwrap_or(""),
                            input.content.context.as_deref().unwrap_or(""),
                        ]
                        .iter()
                        .any(|field| field.to_lowercase().contains(text))
                    });
                if matches {
                    page.records.push(record);
                }
            }
            // Bound work per request without silently losing the remainder.
            if page.records.len() == query.limit || page.scanned_records == query.scan_limit {
                page.next_after = Some(id);
                break;
            }
        }
        page.dependency_work = snapshot.dependency_work();
        Ok(page)
    }

    fn platform_record_for_write(
        &self,
        namespace: &str,
        id: Uuid,
        revision: u64,
    ) -> Result<MemoryRecord> {
        let mut record = self
            .platform_record_locked(namespace, id)?
            .ok_or_else(|| Error::KeyNotFound("Memory record".into()))?;
        if record.revision != revision {
            return Err(Error::TransactionConflict(
                "Memory record revision changed".into(),
            ));
        }
        if record.payload.is_none() {
            return Err(Error::KeyNotFound("Memory record".into()));
        }
        record.revision = revision
            .checked_add(1)
            .ok_or_else(|| Error::TransactionConflict("Memory record revision exhausted".into()))?;
        Ok(record)
    }
    fn platform_record_locked(&self, namespace: &str, id: Uuid) -> Result<Option<MemoryRecord>> {
        let record = self
            .db
            .get_cf(
                self.cf(super::cf::EPISODES)?,
                record_key(0x10, namespace, id),
            )
            .map_err(storage_error)?;
        let index = self
            .db
            .get_cf(
                self.cf(super::cf::EPISODE_INDEX)?,
                record_key(0x11, namespace, id),
            )
            .map_err(storage_error)?;
        decode_record_pair(id, record, index)
    }
}

fn decode_record_pair(
    id: Uuid,
    record: Option<Vec<u8>>,
    index: Option<Vec<u8>>,
) -> Result<Option<MemoryRecord>> {
    let (bytes, index) = match (record, index) {
        (None, None) => return Ok(None),
        (Some(record), Some(index)) => (record, index),
        _ => return Err(inconsistent()),
    };
    let record: MemoryRecord = decode(&bytes)?;
    let index: RecordIndex = decode(&index)?;
    if record.schema_version != 1
        || index.schema_version != 1
        || record.record_id != id
        || record.revision == 0
        || index.revision != record.revision
        || index.record_digest != digest(&bytes)
    {
        return Err(inconsistent());
    }
    Ok(Some(record))
}

fn record_prefix(kind: u8, namespace: &str) -> Vec<u8> {
    let mut key = vec![kind];
    key.extend_from_slice(&(namespace.len() as u16).to_be_bytes());
    key.extend_from_slice(namespace.as_bytes());
    key
}
fn record_key(kind: u8, namespace: &str, id: Uuid) -> Vec<u8> {
    let mut key = record_prefix(kind, namespace);
    key.extend_from_slice(id.as_bytes());
    key
}
fn visible(record: &MemoryRecord, now: i64) -> bool {
    record
        .review
        .as_ref()
        .is_none_or(|review| review.disposition != MemoryReviewDisposition::Rejected)
        && record
            .payload
            .as_ref()
            .is_some_and(|input| input.valid_until_millis.is_none_or(|expiry| now < expiry))
}
fn validate_input(input: &RecordInput) -> Result<()> {
    if chrono::DateTime::from_timestamp_millis(input.event_time_millis).is_none()
        || input
            .valid_until_millis
            .is_some_and(|value| chrono::DateTime::from_timestamp_millis(value).is_none())
        || input
            .content
            .embedding
            .as_ref()
            .is_some_and(|vector| vector.iter().any(|v| !v.is_finite()))
    {
        return Err(Error::ValidationError(
            "Unsupported event/validity time or embedding value".into(),
        ));
    }
    Ok(())
}
fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    serde_json::to_vec(value).map_err(|e| Error::Serialization(e.to_string()))
}
fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    serde_json::from_slice(bytes).map_err(|_| inconsistent())
}
fn digest(bytes: &[u8]) -> [u8; 32] {
    use sha2::Digest;
    sha2::Sha256::digest(bytes).into()
}
fn storage_error(error: rocksdb::Error) -> Error {
    Error::Storage(error.to_string())
}
fn inconsistent() -> Error {
    Error::DataCorruption("Unsupported or inconsistent memory record, index or receipt".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryStorageConfig;
    use std::sync::{Arc, Barrier};
    use tempfile::TempDir;
    fn open(dir: &std::path::Path) -> RocksDbMemoryStorage {
        RocksDbMemoryStorage::open(MemoryStorageConfig {
            path: dir.to_str().unwrap().into(),
            enable_wal: true,
            sync_writes: true,
            ..Default::default()
        })
        .unwrap()
    }
    fn author() -> RecordAuthor {
        RecordAuthor {
            credential_id: Uuid::new_v4(),
            subject_id: "subject".into(),
        }
    }
    fn input(text: &str) -> RecordInput {
        RecordInput {
            episode_type: EpisodeType::Observation,
            content: EpisodeContent::new(text)
                .with_data(serde_json::json!({"value":[1,true,"text"]})),
            event_time_millis: 1_700_000_000_000,
            valid_until_millis: None,
            tags: vec!["tested".into()],
            metadata: [("source".into(), serde_json::json!({"request": 7}))].into(),
        }
    }
    fn create(key: &str, text: &str) -> MemoryCommand {
        MemoryCommand {
            contract_version: 1,
            idempotency_key: key.into(),
            operation: MemoryOperation::Create {
                record: input(text),
            },
        }
    }
    fn query() -> MemoryQuery {
        MemoryQuery {
            scan_limit: 10_000,
            limit: 100,
            after: None,
            text_contains: None,
            episode_type: None,
            tag: None,
        }
    }
    #[test]
    fn platform_memory_receipts_and_records_survive_reopen() {
        let dir = TempDir::new().unwrap();
        let actor = author();
        let command = create("request-1", "durable");
        let receipt = open(dir.path())
            .apply_memory_command("scope", &actor, &command)
            .unwrap();
        let db = open(dir.path());
        assert_eq!(
            db.apply_memory_command("scope", &actor, &command).unwrap(),
            receipt
        );
        let record = db
            .read_memory_record("scope", receipt.record_id)
            .unwrap()
            .unwrap();
        assert_eq!(record.revision, 1);
        assert_eq!(
            record.payload.as_ref().unwrap().metadata["source"]["request"],
            7
        );
        assert_eq!(record.payload.unwrap().content.data.unwrap()["value"][0], 1);
        assert!(
            db.read_memory_record("other", receipt.record_id)
                .unwrap()
                .is_none()
        );
        assert!(matches!(
            db.apply_memory_command("scope", &actor, &create("request-1", "changed")),
            Err(Error::ConstraintViolation(_))
        ));
        assert_eq!(
            db.query_memory_records("scope", &query())
                .unwrap()
                .records
                .len(),
            1
        );
    }
    #[test]
    fn platform_memory_update_delete_and_retry_do_not_resurrect_content() {
        let dir = TempDir::new().unwrap();
        let actor = author();
        let db = open(dir.path());
        let command = create("create", "before");
        let receipt = db.apply_memory_command("scope", &actor, &command).unwrap();
        let update = MemoryCommand {
            contract_version: 1,
            idempotency_key: "update".into(),
            operation: MemoryOperation::Update {
                record_id: receipt.record_id,
                expected_revision: 1,
                record: input("after"),
            },
        };
        assert_eq!(
            db.apply_memory_command("scope", &actor, &update)
                .unwrap()
                .revision,
            2
        );
        assert_eq!(
            db.read_memory_record("scope", receipt.record_id)
                .unwrap()
                .unwrap()
                .payload
                .unwrap()
                .content
                .primary,
            "after"
        );
        let stale = MemoryCommand {
            idempotency_key: "stale".into(),
            ..update.clone()
        };
        assert!(matches!(
            db.apply_memory_command("scope", &actor, &stale),
            Err(Error::TransactionConflict(_))
        ));
        let delete = MemoryCommand {
            contract_version: 1,
            idempotency_key: "delete".into(),
            operation: MemoryOperation::Delete {
                record_id: receipt.record_id,
                expected_revision: 2,
            },
        };
        let deleted = db.apply_memory_command("scope", &actor, &delete).unwrap();
        assert_eq!(deleted.revision, 3);
        assert_eq!(
            db.apply_memory_command("scope", &actor, &command).unwrap(),
            receipt
        );
        assert_eq!(
            db.apply_memory_command("scope", &actor, &delete).unwrap(),
            deleted
        );
        assert!(
            db.read_memory_record("scope", receipt.record_id)
                .unwrap()
                .is_none()
        );
        assert!(
            db.query_memory_records("scope", &query())
                .unwrap()
                .records
                .is_empty()
        );
    }
    #[test]
    fn platform_memory_concurrent_retries_create_one_logical_record() {
        let dir = TempDir::new().unwrap();
        let db = Arc::new(open(dir.path()));
        let actor = author();
        let gate = Arc::new(Barrier::new(12));
        let handles: Vec<_> = (0..12)
            .map(|_| {
                let db = db.clone();
                let gate = gate.clone();
                let actor = actor.clone();
                std::thread::spawn(move || {
                    gate.wait();
                    db.apply_memory_command("scope", &actor, &create("same-key", "same-data"))
                        .unwrap()
                })
            })
            .collect();
        let receipts: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert!(receipts.iter().all(|r| r == &receipts[0]));
        assert_eq!(
            db.query_memory_records("scope", &query())
                .unwrap()
                .records
                .len(),
            1
        );
    }
    #[test]
    fn platform_memory_queries_enforce_scope_validity_filters_and_pagination() {
        let dir = TempDir::new().unwrap();
        let db = open(dir.path());
        let actor = author();
        for i in 0..4 {
            db.apply_memory_command("scope", &actor, &create(&format!("r{i}"), "Needle"))
                .unwrap();
        }
        let mut expired = input("Needle");
        expired.valid_until_millis = Some(1);
        let expired_receipt = db
            .apply_memory_command(
                "scope",
                &actor,
                &MemoryCommand {
                    contract_version: 1,
                    idempotency_key: "expired".into(),
                    operation: MemoryOperation::Create { record: expired },
                },
            )
            .unwrap();
        assert!(
            db.read_memory_record("scope", expired_receipt.record_id)
                .unwrap()
                .is_none()
        );
        let mut q = MemoryQuery {
            limit: 2,
            text_contains: Some("needle".into()),
            tag: Some("tested".into()),
            ..query()
        };
        let mut ids = std::collections::HashSet::new();
        loop {
            let page = db.query_memory_records("scope", &q).unwrap();
            for item in page.records {
                assert!(ids.insert(item.record_id));
            }
            if page.next_after.is_none() {
                break;
            }
            q.after = page.next_after;
        }
        assert_eq!(ids.len(), 4);
        assert!(
            db.query_memory_records("other", &query())
                .unwrap()
                .records
                .is_empty()
        );
        assert!(
            db.query_memory_records(
                "scope",
                &MemoryQuery {
                    limit: 0,
                    ..query()
                }
            )
            .is_err()
        );
    }
    #[test]
    fn platform_memory_record_index_version_mismatch_is_explicit() {
        let dir = TempDir::new().unwrap();
        let db = open(dir.path());
        let actor = author();
        let receipt = db
            .apply_memory_command("scope", &actor, &create("create", "value"))
            .unwrap();
        let index_cf = db.cf(super::super::cf::EPISODE_INDEX).unwrap();
        let key = record_key(0x11, "scope", receipt.record_id);
        let bytes = db.db.get_cf(index_cf, &key).unwrap().unwrap();
        let mut index: RecordIndex = decode(&bytes).unwrap();
        index.revision += 1;
        db.db
            .put_cf(index_cf, &key, encode(&index).unwrap())
            .unwrap();
        assert!(matches!(
            db.read_memory_record("scope", receipt.record_id),
            Err(Error::DataCorruption(_))
        ));
        assert!(matches!(
            db.query_memory_records("scope", &query()),
            Err(Error::DataCorruption(_))
        ));
    }
    #[test]
    fn platform_memory_concurrent_updates_reject_lost_writes() {
        let dir = TempDir::new().unwrap();
        let db = Arc::new(open(dir.path()));
        let actor = author();
        let first = db
            .apply_memory_command("scope", &actor, &create("create", "before"))
            .unwrap();
        let gate = Arc::new(Barrier::new(8));
        let handles: Vec<_> = (0..8)
            .map(|number| {
                let db = db.clone();
                let actor = actor.clone();
                let gate = gate.clone();
                std::thread::spawn(move || {
                    gate.wait();
                    db.apply_memory_command(
                        "scope",
                        &actor,
                        &MemoryCommand {
                            contract_version: 1,
                            idempotency_key: format!("update-{number}"),
                            operation: MemoryOperation::Update {
                                record_id: first.record_id,
                                expected_revision: 1,
                                record: input(&number.to_string()),
                            },
                        },
                    )
                })
            })
            .collect();
        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
        assert!(
            results
                .iter()
                .filter(|r| r.is_err())
                .all(|r| matches!(r, Err(Error::TransactionConflict(_))))
        );
        assert_eq!(
            db.read_memory_record("scope", first.record_id)
                .unwrap()
                .unwrap()
                .revision,
            2
        );
    }
}

mod semantic;
mod batch_read;
pub use batch_read::*;
mod evidence_graph;
pub use evidence_graph::*;
mod relations;
pub use relations::*;
#[cfg(test)]
mod semantic_tests;
pub use semantic::*;

mod lexical;
#[cfg(test)]
mod lexical_tests;
mod snapshot;
mod candidates;
#[cfg(test)]
mod candidate_tests;
pub use candidates::CANDIDATE_SELECTION_VERSION;
mod company;
pub use company::*;
mod agents;
pub use agents::*;
pub use lexical::*;
mod hybrid;
#[cfg(test)]
mod hybrid_tests;
pub use hybrid::*;

mod changes;
pub use changes::*;
#[cfg(test)]
mod changes_tests;

mod review;
pub use review::*;
#[cfg(test)]
mod review_tests;

mod derivation;
pub use derivation::*;
#[cfg(test)]
mod derivation_tests;

mod eligibility;
pub use eligibility::*;
#[cfg(test)]
mod eligibility_tests;

mod consumer_catalog;
pub use consumer_catalog::*;

mod checkpoints;
pub use checkpoints::*;
#[cfg(test)]
mod checkpoint_tests;

mod verified_changes;
pub use verified_changes::*;
mod journal_audit;
pub use journal_audit::*;
#[cfg(test)]
mod journal_audit_tests;
#[cfg(test)]
mod verified_changes_tests;

mod verified_checkpoints;
pub use verified_checkpoints::*;
#[cfg(test)]
mod verified_checkpoint_tests;
mod graph_retrieval;
pub use graph_retrieval::*;
