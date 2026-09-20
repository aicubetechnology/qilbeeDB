//! BM25 over a bounded, authorized corpus from a single storage snapshot.
use super::snapshot::MemorySnapshot;
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LexicalQuery {
    pub text: String,
    pub limit: usize,
    #[serde(default = "default_scan_limit")]
    pub scan_limit: usize,
    #[serde(default = "default_scan_bytes_limit")]
    pub scan_bytes_limit: usize,
    pub after: Option<Uuid>,
    pub episode_type: Option<EpisodeType>,
    pub tag: Option<String>,
}
pub(super) fn default_scan_limit() -> usize {
    10_000
}
pub(super) fn default_scan_bytes_limit() -> usize {
    8_388_608
}
impl LexicalQuery {
    pub(super) fn validate(&self) -> Result<()> {
        let terms: std::collections::BTreeSet<_> = self
            .text
            .split(|c: char| !c.is_alphanumeric())
            .filter(|s| !s.is_empty())
            .map(str::to_lowercase)
            .collect();
        if self.text.len() > 4096
            || terms.is_empty()
            || terms.len() > 64
            || !(1..=100).contains(&self.limit)
            || !(1..=10_000).contains(&self.scan_limit)
            || !(1..=67_108_864).contains(&self.scan_bytes_limit)
        {
            return Err(Error::ValidationError(
                "Invalid lexical text, result limit or scan budget".into(),
            ));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LexicalHit {
    pub record: MemoryRecord,
    pub score: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LexicalPage {
    pub hits: Vec<LexicalHit>,
    pub next_after: Option<Uuid>,
    pub scanned_records: usize,
    pub scanned_bytes: usize,
    pub corpus_records: usize,
    pub matched_records: usize,
    /// Statistics and ranking cover the whole filtered scope only when true.
    pub exhaustive: bool,
}
pub(super) struct ScannedCorpus {
    pub records: Vec<MemoryRecord>,
    pub embeddings: std::collections::BTreeMap<Uuid, super::semantic::StoredEmbedding>,
    pub page: LexicalPage,
}
impl RocksDbMemoryStorage {
    /// BM25 needs no embedding and includes existing durable records without migration.
    pub fn search_memory_lexical(
        &self,
        namespace: &str,
        query: &LexicalQuery,
    ) -> Result<LexicalPage> {
        let ScannedCorpus {
            records, mut page, ..
        } = self.memory_snapshot().scan_corpus(namespace, query)?;
        let ranks = rank_records(&records, &query.text);
        page.matched_records = ranks.len();
        let mut records: std::collections::BTreeMap<_, _> =
            records.into_iter().map(|r| (r.record_id, r)).collect();
        page.hits = ranks
            .into_iter()
            .take(query.limit)
            .map(|(id, score)| LexicalHit {
                record: records.remove(&id).expect("ranked record exists"),
                score,
            })
            .collect();
        Ok(page)
    }
}
pub(super) fn rank_records(records: &[MemoryRecord], text: &str) -> Vec<(Uuid, f64)> {
    crate::retrieval::rank_contents(
        records.iter().map(|r| {
            (
                r.record_id,
                &r.payload.as_ref().expect("corpus is visible").content,
            )
        }),
        text,
        usize::MAX,
    )
}
impl MemorySnapshot<'_> {
    pub(super) fn scan_corpus(
        &self,
        namespace: &str,
        query: &LexicalQuery,
    ) -> Result<ScannedCorpus> {
        self.scan_corpus_with_embeddings(namespace, query, None)
    }
    pub(super) fn scan_corpus_with_embeddings(
        &self,
        namespace: &str,
        query: &LexicalQuery,
        space: Option<&EmbeddingSpace>,
    ) -> Result<ScannedCorpus> {
        RocksDbMemoryStorage::validate_agent(namespace)?;
        query.validate()?;
        let prefix = record_prefix(0x10, namespace);
        let start = query
            .after
            .map(|id| record_key(0x10, namespace, id))
            .unwrap_or_else(|| prefix.clone());
        let mut page = LexicalPage {
            hits: vec![],
            next_after: None,
            scanned_records: 0,
            scanned_bytes: 0,
            corpus_records: 0,
            matched_records: 0,
            exhaustive: query.after.is_none(),
        };
        let mut records = vec![];
        let mut embeddings = std::collections::BTreeMap::new();
        let mut last = None;
        for item in self.db.iterator_cf(
            self.storage.cf(super::super::cf::EPISODES)?,
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
            if page.scanned_records == query.scan_limit
                || page.scanned_bytes + bytes.len() > query.scan_bytes_limit
            {
                if last.is_none() {
                    return Err(Error::ValidationError(
                        "Scan byte budget cannot fit the next record".into(),
                    ));
                }
                page.next_after = last;
                page.exhaustive = false;
                break;
            }
            let record = self.record(namespace, id)?.ok_or_else(inconsistent)?;
            let eligible = visible(&record, self.now)
                && record.payload.as_ref().is_some_and(|payload| {
                    query
                        .episode_type
                        .as_ref()
                        .is_none_or(|kind| kind == &payload.episode_type)
                        && query
                            .tag
                            .as_ref()
                            .is_none_or(|tag| payload.tags.contains(tag))
                });
            let embedding = if eligible {
                space
                    .map(|space| self.embedding(namespace, space, id))
                    .transpose()?
                    .flatten()
            } else {
                None
            };
            let row_bytes = bytes.len() + embedding.as_ref().map_or(0, |(_, size)| *size);
            if page.scanned_bytes + row_bytes > query.scan_bytes_limit {
                if last.is_none() {
                    return Err(Error::ValidationError(
                        "Scan byte budget cannot fit the next record and embedding".into(),
                    ));
                }
                page.next_after = last;
                page.exhaustive = false;
                break;
            }
            page.scanned_records += 1;
            page.scanned_bytes += row_bytes;
            last = Some(id);
            if !eligible {
                continue;
            }
            if let Some((embedding, _)) = embedding {
                if embedding.receipt.record_revision > record.revision {
                    return Err(inconsistent());
                }
                if embedding.receipt.record_revision == record.revision {
                    embeddings.insert(id, embedding);
                }
            }
            records.push(record);
        }
        page.corpus_records = records.len();
        Ok(ScannedCorpus {
            records,
            embeddings,
            page,
        })
    }
}
