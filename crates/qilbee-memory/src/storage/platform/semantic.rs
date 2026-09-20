//! Durable, revision-bound embeddings and bounded exact cosine retrieval.
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddingSpace {
    pub provider: String,
    pub model: String,
    pub revision: String,
    pub dimensions: usize,
}
impl EmbeddingSpace {
    fn validate(&self) -> Result<()> {
        for value in [&self.provider, &self.model, &self.revision] {
            if value.trim().is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
                return Err(Error::ValidationError(
                    "Invalid embedding space identity".into(),
                ));
            }
        }
        if !(1..=4096).contains(&self.dimensions) {
            return Err(Error::ValidationError(
                "Embedding dimensions must be in 1..=4096".into(),
            ));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddingCommand {
    pub contract_version: u32,
    pub idempotency_key: String,
    pub record_id: Uuid,
    pub record_revision: u64,
    pub space: EmbeddingSpace,
    pub vector: Vec<f32>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddingReceipt {
    pub contract_version: u32,
    pub idempotency_key: String,
    pub record_id: Uuid,
    pub record_revision: u64,
    pub space: EmbeddingSpace,
    pub vector_digest: String,
    pub author: RecordAuthor,
    pub committed_at_millis: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticQuery {
    pub space: EmbeddingSpace,
    pub vector: Vec<f32>,
    pub limit: usize,
    #[serde(default = "minimum_score")]
    pub min_score: f64,
    pub after: Option<Uuid>,
    #[serde(default = "default_scan_limit")]
    pub scan_limit: usize,
    pub episode_type: Option<EpisodeType>,
    pub tag: Option<String>,
}
fn minimum_score() -> f64 {
    -1.0
}
fn default_scan_limit() -> usize {
    10_000
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticHit {
    pub record: MemoryRecord,
    pub score: f64,
    pub embedding: EmbeddingReceipt,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticPage {
    pub hits: Vec<SemanticHit>,
    pub next_after: Option<Uuid>,
    pub scanned_embeddings: usize,
    pub matched_records: usize,
    /// True only when this request ranked the whole model space without a cursor.
    pub exhaustive: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredEmbedding {
    schema_version: u32,
    namespace: String,
    receipt: EmbeddingReceipt,
    vector: Vec<f32>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredEmbeddingReceipt {
    schema_version: u32,
    request_digest: [u8; 32],
    receipt: EmbeddingReceipt,
}
fn embedding_prefix(namespace: &str, space: &EmbeddingSpace) -> Result<Vec<u8>> {
    let mut key = record_prefix(0x13, namespace);
    key.extend_from_slice(&digest(&encode(space)?));
    Ok(key)
}
fn embedding_key(namespace: &str, space: &EmbeddingSpace, id: Uuid) -> Result<Vec<u8>> {
    let mut key = embedding_prefix(namespace, space)?;
    key.extend_from_slice(id.as_bytes());
    Ok(key)
}
fn vector_digest(vector: &[f32]) -> Result<String> {
    Ok(digest(&encode(&vector)?)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}
fn norm(vector: &[f32], dimensions: usize) -> Result<f64> {
    if vector.len() != dimensions || vector.iter().any(|value| !value.is_finite()) {
        return Err(Error::ValidationError(
            "Embedding must match dimensions and contain finite values".into(),
        ));
    }
    let norm = vector
        .iter()
        .map(|&value| f64::from(value).powi(2))
        .sum::<f64>()
        .sqrt();
    if norm == 0.0 {
        return Err(Error::ValidationError(
            "Zero embeddings cannot define cosine similarity".into(),
        ));
    }
    Ok(norm)
}
fn decode_embedding(
    bytes: &[u8],
    namespace: &str,
    space: &EmbeddingSpace,
    id: Uuid,
) -> Result<StoredEmbedding> {
    let stored: StoredEmbedding = decode(bytes)?;
    if stored.schema_version != 1
        || stored.namespace != namespace
        || stored.receipt.contract_version != 1
        || stored.receipt.record_id != id
        || stored.receipt.record_revision == 0
        || stored.receipt.space != *space
        || stored.receipt.vector_digest != vector_digest(&stored.vector)?
        || norm(&stored.vector, space.dimensions).is_err()
    {
        return Err(inconsistent());
    }
    Ok(stored)
}
impl RocksDbMemoryStorage {
    /// Attach externally generated vectors to the exact current memory revision.
    /// Old retries return their original receipt without resurrecting a binding.
    pub fn apply_memory_embedding(
        &self,
        namespace: &str,
        author: &RecordAuthor,
        command: &EmbeddingCommand,
    ) -> Result<EmbeddingReceipt> {
        Self::validate_agent(namespace)?;
        command.space.validate()?;
        norm(&command.vector, command.space.dimensions)?;
        if command.contract_version != 1
            || command.record_revision == 0
            || command.idempotency_key.trim().is_empty()
            || command.idempotency_key.len() > 256
            || command.idempotency_key.chars().any(char::is_control)
            || author.subject_id.trim().is_empty()
            || author.subject_id.len() > 256
        {
            return Err(Error::ValidationError(
                "Invalid embedding command identity or contract version".into(),
            ));
        }
        let _guard = self
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Memory mutation lock poisoned".into()))?;
        let receipts = self.cf(super::super::cf::AGENT_META)?;
        let mut receipt_key = vec![0x14];
        receipt_key.extend(encode(&(
            namespace,
            &author.subject_id,
            &command.idempotency_key,
        ))?);
        let request_digest = digest(&encode(command)?);
        if let Some(bytes) = self
            .db
            .get_cf(receipts, &receipt_key)
            .map_err(storage_error)?
        {
            let stored: StoredEmbeddingReceipt = decode(&bytes)?;
            if stored.schema_version != 1
                || stored.receipt.contract_version != 1
                || stored.receipt.idempotency_key != command.idempotency_key
                || stored.receipt.author.subject_id != author.subject_id
            {
                return Err(inconsistent());
            }
            if stored.request_digest != request_digest {
                return Err(Error::ConstraintViolation(
                    "Embedding idempotency key was already used".into(),
                ));
            }
            if stored.receipt.record_id != command.record_id
                || stored.receipt.record_revision != command.record_revision
                || stored.receipt.space != command.space
                || stored.receipt.vector_digest != vector_digest(&command.vector)?
            {
                return Err(inconsistent());
            }
            return Ok(stored.receipt);
        }
        let record = self
            .platform_record_locked(namespace, command.record_id)?
            .filter(|record| visible(record, chrono::Utc::now().timestamp_millis()))
            .ok_or_else(|| Error::KeyNotFound("Current memory record".into()))?;
        if record.revision != command.record_revision {
            return Err(Error::TransactionConflict(
                "Embedding source revision changed".into(),
            ));
        }
        let key = embedding_key(namespace, &command.space, command.record_id)?;
        let embeddings = self.cf(super::super::cf::EPISODE_INDEX)?;
        let existing = self
            .db
            .get_cf(embeddings, &key)
            .map_err(storage_error)?
            .map(|bytes| decode_embedding(&bytes, namespace, &command.space, command.record_id))
            .transpose()?;
        if let Some(existing) = &existing {
            if existing.receipt.record_revision > command.record_revision {
                return Err(inconsistent());
            }
            if existing.receipt.record_revision == command.record_revision
                && existing.receipt.vector_digest != vector_digest(&command.vector)?
            {
                return Err(Error::ConstraintViolation(
                    "Embedding is immutable within a record revision and model space".into(),
                ));
            }
        }
        let receipt = EmbeddingReceipt {
            contract_version: 1,
            idempotency_key: command.idempotency_key.clone(),
            record_id: command.record_id,
            record_revision: command.record_revision,
            space: command.space.clone(),
            vector_digest: vector_digest(&command.vector)?,
            author: author.clone(),
            committed_at_millis: chrono::Utc::now().timestamp_millis(),
        };
        let mut batch = rocksdb::WriteBatch::default();
        if existing
            .as_ref()
            .is_none_or(|existing| existing.receipt.record_revision != command.record_revision)
        {
            batch.put_cf(
                embeddings,
                key,
                encode(&StoredEmbedding {
                    schema_version: 1,
                    namespace: namespace.into(),
                    receipt: receipt.clone(),
                    vector: command.vector.clone(),
                })?,
            );
        }
        batch.put_cf(
            receipts,
            receipt_key,
            encode(&StoredEmbeddingReceipt {
                schema_version: 1,
                request_digest,
                receipt: receipt.clone(),
            })?,
        );
        let mut options = rocksdb::WriteOptions::default();
        options.disable_wal(false);
        options.set_sync(true);
        self.db.write_opt(batch, &options).map_err(storage_error)?;
        Ok(receipt)
    }
    /// Exact cosine top-k within an explicitly bounded scan page. The caller
    /// must not interpret a partial page as the global top-k of a large namespace.
    pub fn search_memory_semantic(
        &self,
        namespace: &str,
        query: &SemanticQuery,
    ) -> Result<SemanticPage> {
        self.memory_snapshot().search_semantic(namespace, query)
    }
}
impl super::snapshot::MemorySnapshot<'_> {
    pub(super) fn search_semantic(
        &self,
        namespace: &str,
        query: &SemanticQuery,
    ) -> Result<SemanticPage> {
        RocksDbMemoryStorage::validate_agent(namespace)?;
        query.space.validate()?;
        let query_norm = norm(&query.vector, query.space.dimensions)?;
        if !(1..=100).contains(&query.limit)
            || !(1..=10_000).contains(&query.scan_limit)
            || !query.min_score.is_finite()
            || !(-1.0..=1.0).contains(&query.min_score)
        {
            return Err(Error::ValidationError(
                "Invalid semantic limit, scan budget or score threshold".into(),
            ));
        }
        let prefix = embedding_prefix(namespace, &query.space)?;
        let start = query
            .after
            .map(|id| embedding_key(namespace, &query.space, id))
            .transpose()?
            .unwrap_or_else(|| prefix.clone());
        let mut page = SemanticPage {
            hits: vec![],
            next_after: None,
            scanned_embeddings: 0,
            matched_records: 0,
            exhaustive: query.after.is_none(),
        };
        let mut last_scanned = None;
        let now = self.now;
        for item in self.db.iterator_cf(
            self.storage.cf(super::super::cf::EPISODE_INDEX)?,
            rocksdb::IteratorMode::From(&start, rocksdb::Direction::Forward),
        ) {
            let (key, bytes) = item.map_err(storage_error)?;
            if !key.starts_with(&prefix) {
                break;
            }
            let id = Uuid::from_slice(&key[prefix.len()..]).map_err(|_| inconsistent())?;
            if query.after.is_some_and(|after| id <= after) {
                continue;
            }
            if page.scanned_embeddings == query.scan_limit {
                page.next_after = last_scanned;
                page.exhaustive = false;
                break;
            }
            let embedding = decode_embedding(&bytes, namespace, &query.space, id)?;
            page.scanned_embeddings += 1;
            last_scanned = Some(id);
            let record = self.record(namespace, id)?.ok_or_else(inconsistent)?;
            if record.revision < embedding.receipt.record_revision {
                return Err(inconsistent());
            }
            if record.revision != embedding.receipt.record_revision || !visible(&record, now) {
                continue;
            }
            let payload = record.payload.as_ref().expect("visible memory has content");
            if query
                .episode_type
                .as_ref()
                .is_some_and(|kind| kind != &payload.episode_type)
                || query
                    .tag
                    .as_ref()
                    .is_some_and(|tag| !payload.tags.contains(tag))
            {
                continue;
            }
            let denominator = query_norm * norm(&embedding.vector, query.space.dimensions)?;
            let dot: f64 = query
                .vector
                .iter()
                .zip(&embedding.vector)
                .map(|(&a, &b)| f64::from(a) * f64::from(b))
                .sum();
            let score = (dot / denominator).clamp(-1.0, 1.0);
            if score < query.min_score {
                continue;
            }
            page.matched_records += 1;
            page.hits.push(SemanticHit {
                record,
                score,
                embedding: embedding.receipt,
            });
            page.hits.sort_by(|a, b| {
                b.score
                    .total_cmp(&a.score)
                    .then_with(|| a.record.record_id.cmp(&b.record.record_id))
            });
            page.hits.truncate(query.limit);
        }
        Ok(page)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn semantic_corrupt_vectors_fail_explicitly() {
        let vector = vec![1.0];
        let space = EmbeddingSpace {
            provider: "p".into(),
            model: "m".into(),
            revision: "v".into(),
            dimensions: 1,
        };
        let id = Uuid::new_v4();
        let receipt = EmbeddingReceipt {
            contract_version: 1,
            idempotency_key: "key".into(),
            record_id: id,
            record_revision: 1,
            space: space.clone(),
            vector_digest: vector_digest(&vector).unwrap(),
            author: RecordAuthor {
                credential_id: Uuid::new_v4(),
                subject_id: "a".into(),
            },
            committed_at_millis: 0,
        };
        let mut stored = StoredEmbedding {
            schema_version: 1,
            namespace: "scope".into(),
            receipt,
            vector,
        };
        stored.vector[0] = 2.0;
        assert!(matches!(
            decode_embedding(&encode(&stored).unwrap(), "scope", &space, id),
            Err(Error::DataCorruption(_))
        ));
    }
}
