//! Weighted reciprocal rank fusion of scoped lexical and external-vector ranks.
use super::snapshot::MemorySnapshot;
use super::*;
use std::collections::BTreeMap;
const RANK_CONSTANT: usize = 60;
fn default_candidates() -> usize {
    100
}
fn default_weight() -> f64 {
    0.5
}
fn default_min_score() -> f64 {
    -1.0
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HybridQuery {
    pub text: String,
    pub space: EmbeddingSpace,
    pub vector: Vec<f32>,
    pub limit: usize,
    #[serde(default = "default_candidates")]
    pub candidate_limit: usize,
    #[serde(default = "default_weight")]
    pub semantic_weight: f64,
    #[serde(default = "default_min_score")]
    pub min_score: f64,
    #[serde(default = "super::lexical::default_scan_limit")]
    pub scan_limit: usize,
    #[serde(default = "super::lexical::default_scan_bytes_limit")]
    pub scan_bytes_limit: usize,
    pub after: Option<Uuid>,
    pub episode_type: Option<EpisodeType>,
    pub tag: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RankContribution {
    /// One-based rank in this channel's candidate list.
    pub rank: usize,
    /// Raw BM25 or cosine score; not a calibrated probability.
    pub score: f64,
    pub contribution: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HybridHit {
    pub record: MemoryRecord,
    pub score: f64,
    pub lexical: Option<RankContribution>,
    pub semantic: Option<RankContribution>,
    /// Present exactly when this hit has a semantic contribution.
    pub embedding: Option<EmbeddingReceipt>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HybridPage {
    pub hits: Vec<HybridHit>,
    pub next_after: Option<Uuid>,
    pub scanned_records: usize,
    pub scanned_bytes: usize,
    pub corpus_records: usize,
    pub embedded_records: usize,
    pub lexical_matches: usize,
    pub semantic_matches: usize,
    pub lexical_candidates: usize,
    pub semantic_candidates: usize,
    pub candidates_truncated: bool,
    pub rank_constant: usize,
    /// Describes source scan coverage, independently of candidate truncation.
    pub exhaustive: bool,
}
impl RocksDbMemoryStorage {
    pub fn search_memory_hybrid(&self, namespace: &str, query: &HybridQuery) -> Result<HybridPage> {
        self.memory_snapshot().search_hybrid(namespace, query)
    }
}
impl MemorySnapshot<'_> {
    pub(super) fn search_hybrid(&self, namespace: &str, query: &HybridQuery) -> Result<HybridPage> {
        query.space.validate()?;
        let query_norm = super::semantic::norm(&query.vector, query.space.dimensions)?;
        if !(query.limit..=1000).contains(&query.candidate_limit)
            || !query.semantic_weight.is_finite()
            || !(0.0..=1.0).contains(&query.semantic_weight)
            || !query.min_score.is_finite()
            || !(-1.0..=1.0).contains(&query.min_score)
        {
            return Err(Error::ValidationError(
                "Invalid hybrid candidate limit, weight or cosine threshold".into(),
            ));
        }
        let lexical_query = LexicalQuery {
            text: query.text.clone(),
            limit: query.limit,
            scan_limit: query.scan_limit,
            scan_bytes_limit: query.scan_bytes_limit,
            after: query.after,
            episode_type: query.episode_type.clone(),
            tag: query.tag.clone(),
        };
        let super::lexical::ScannedCorpus {
            records,
            mut embeddings,
            page,
        } = self.scan_corpus_with_embeddings(
            namespace,
            &lexical_query,
            (query.semantic_weight > 0.0).then_some(&query.space),
        )?;
        let embedded_records = embeddings.len();
        let mut lexical = if query.semantic_weight < 1.0 {
            super::lexical::rank_records(&records, &query.text)
        } else {
            vec![]
        };
        let mut semantic = Vec::with_capacity(embeddings.len());
        for (&id, embedding) in &embeddings {
            let denominator =
                query_norm * super::semantic::norm(&embedding.vector, query.space.dimensions)?;
            let dot: f64 = query
                .vector
                .iter()
                .zip(&embedding.vector)
                .map(|(&a, &b)| f64::from(a) * f64::from(b))
                .sum();
            let score = (dot / denominator).clamp(-1.0, 1.0);
            if score >= query.min_score {
                semantic.push((id, score));
            }
        }
        semantic.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let lexical_matches = lexical.len();
        let semantic_matches = semantic.len();
        lexical.truncate(query.candidate_limit);
        semantic.truncate(query.candidate_limit);
        let lexical_candidates = lexical.len();
        let semantic_candidates = semantic.len();
        let mut fused =
            BTreeMap::<Uuid, (Option<RankContribution>, Option<RankContribution>)>::new();
        for (is_semantic, candidates, weight) in [
            (false, lexical, 1.0 - query.semantic_weight),
            (true, semantic, query.semantic_weight),
        ] {
            for (index, (id, score)) in candidates.into_iter().enumerate() {
                let rank = index + 1;
                let contribution = RankContribution {
                    rank,
                    score,
                    contribution: weight / (RANK_CONSTANT + rank) as f64,
                };
                let channels = fused.entry(id).or_default();
                if is_semantic {
                    channels.1 = Some(contribution);
                } else {
                    channels.0 = Some(contribution);
                }
            }
        }
        let mut records: BTreeMap<_, _> = records.into_iter().map(|r| (r.record_id, r)).collect();
        let mut hits: Vec<_> = fused
            .into_iter()
            .map(|(id, (lexical, semantic))| HybridHit {
                score: lexical.as_ref().map_or(0.0, |c| c.contribution)
                    + semantic.as_ref().map_or(0.0, |c| c.contribution),
                embedding: semantic.as_ref().map(|_| {
                    embeddings
                        .remove(&id)
                        .expect("semantic candidate has a binding")
                        .receipt
                }),
                record: records.remove(&id).expect("candidate belongs to corpus"),
                lexical,
                semantic,
            })
            .collect();
        hits.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then_with(|| a.record.record_id.cmp(&b.record.record_id))
        });
        hits.truncate(query.limit);
        Ok(HybridPage {
            hits,
            next_after: page.next_after,
            scanned_records: page.scanned_records,
            scanned_bytes: page.scanned_bytes,
            corpus_records: page.corpus_records,
            embedded_records,
            lexical_matches,
            semantic_matches,
            lexical_candidates,
            semantic_candidates,
            candidates_truncated: lexical_matches > lexical_candidates
                || semantic_matches > semantic_candidates,
            rank_constant: RANK_CONSTANT,
            exhaustive: page.exhaustive,
        })
    }
}
