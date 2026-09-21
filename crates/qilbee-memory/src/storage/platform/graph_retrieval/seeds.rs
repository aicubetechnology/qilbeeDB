use super::*;

impl MemorySnapshot<'_> {
    pub(super) fn graph_seeds(
        &self,
        namespace: &str,
        query: &GraphRetrievalQuery,
        profile: &GraphRankingProfile,
    ) -> Result<(GraphSeedMetadata, Vec<SeedHit>)> {
        match &query.seed {
            GraphSeedQuery::Lexical { text } => {
                let page = self.search_lexical(
                    namespace,
                    &LexicalQuery {
                        text: text.clone(),
                        limit: profile.base_candidate_limit,
                        scan_limit: query.scan_limit,
                        scan_bytes_limit: query.scan_bytes_limit,
                        after: None,
                        episode_type: query.episode_type.clone(),
                        tag: query.tag.clone(),
                    },
                )?;
                Ok((
                    GraphSeedMetadata {
                        mode: "lexical".into(),
                        ranking_version: "bm25_v1".into(),
                        hybrid_profile: None,
                        embedding_space: None,
                        embedding_coverage: None,
                        selected_anchors: vec![],
                        candidate_selection_version: page.candidate_selection_version,
                        scanned_records: page.scanned_records,
                        scanned_bytes: page.scanned_bytes,
                        candidate_index_bytes: page.candidate_index_bytes,
                        corpus_records: Some(page.corpus_records),
                        lexical_matches: Some(page.matched_records),
                        semantic_matches: None,
                        base_candidates: page.hits.len(),
                        base_candidates_truncated: page.matched_records > page.hits.len(),
                        channel_candidates_truncated: false,
                        scan_complete: page.exhaustive,
                        dependency_work: page.dependency_work,
                    },
                    page.hits
                        .into_iter()
                        .map(|h| SeedHit {
                            record: h.record,
                            score: GraphSeedScore::Lexical { score: h.score },
                        })
                        .collect(),
                ))
            }
            GraphSeedQuery::Semantic {
                space,
                vector,
                min_score,
            } => {
                let (page, eligible, current) = self.search_semantic_with_byte_limit(
                    namespace,
                    &SemanticQuery {
                        space: space.clone(),
                        vector: vector.clone(),
                        limit: profile.base_candidate_limit,
                        min_score: *min_score,
                        scan_limit: query.scan_limit,
                        after: None,
                        episode_type: query.episode_type.clone(),
                        tag: query.tag.clone(),
                    },
                    Some(query.scan_bytes_limit),
                )?;
                let coverage = if eligible == 0 {
                    EmbeddingCoverage::EmptyCorpus
                } else if current == 0 {
                    EmbeddingCoverage::Missing
                } else if current == eligible {
                    EmbeddingCoverage::Complete
                } else {
                    EmbeddingCoverage::Partial
                };
                Ok((
                    GraphSeedMetadata {
                        mode: "semantic".into(),
                        ranking_version: "cosine_exact_v1".into(),
                        hybrid_profile: None,
                        embedding_space: Some(space.clone()),
                        embedding_coverage: Some(coverage),
                        selected_anchors: vec![],
                        candidate_selection_version: page.candidate_selection_version,
                        scanned_records: page.scanned_records,
                        scanned_bytes: page.scanned_bytes,
                        candidate_index_bytes: page.candidate_index_bytes,
                        corpus_records: Some(eligible),
                        lexical_matches: None,
                        semantic_matches: Some(page.matched_records),
                        base_candidates: page.hits.len(),
                        base_candidates_truncated: page.matched_records > page.hits.len(),
                        channel_candidates_truncated: false,
                        scan_complete: page.exhaustive,
                        dependency_work: page.dependency_work,
                    },
                    page.hits
                        .into_iter()
                        .map(|h| SeedHit {
                            record: h.record,
                            score: GraphSeedScore::Semantic {
                                score: h.score,
                                embedding: h.embedding,
                            },
                        })
                        .collect(),
                ))
            }
            GraphSeedQuery::Hybrid {
                text,
                space,
                vector,
                ranking_version,
                min_score,
            } => {
                let (page, fused) = self.search_hybrid_counted(
                    namespace,
                    &HybridQuery {
                        text: text.clone(),
                        space: space.clone(),
                        vector: vector.clone(),
                        ranking_version: *ranking_version,
                        limit: profile.base_candidate_limit,
                        min_score: *min_score,
                        scan_limit: query.scan_limit,
                        scan_bytes_limit: query.scan_bytes_limit,
                        after: None,
                        episode_type: query.episode_type.clone(),
                        tag: query.tag.clone(),
                    },
                )?;
                let version = match page.ranking.version {
                    HybridRankingVersion::WeightedRrfV1 => "weighted_rrf_v1",
                    HybridRankingVersion::WeightedRrfV2 => "weighted_rrf_v2",
                }
                .into();
                Ok((
                    GraphSeedMetadata {
                        mode: "hybrid".into(),
                        ranking_version: version,
                        hybrid_profile: Some(page.ranking),
                        embedding_space: Some(space.clone()),
                        embedding_coverage: Some(page.embedding_coverage),
                        selected_anchors: vec![],
                        candidate_selection_version: page.candidate_selection_version,
                        scanned_records: page.scanned_records,
                        scanned_bytes: page.scanned_bytes,
                        candidate_index_bytes: page.candidate_index_bytes,
                        corpus_records: Some(page.corpus_records),
                        lexical_matches: Some(page.lexical_matches),
                        semantic_matches: Some(page.semantic_matches),
                        base_candidates: page.hits.len(),
                        base_candidates_truncated: fused > page.hits.len(),
                        channel_candidates_truncated: page.candidates_truncated,
                        scan_complete: page.exhaustive,
                        dependency_work: page.dependency_work,
                    },
                    page.hits
                        .into_iter()
                        .map(|h| SeedHit {
                            record: h.record,
                            score: GraphSeedScore::Hybrid {
                                score: h.score,
                                lexical: h.lexical,
                                semantic: h.semantic,
                                embedding: h.embedding,
                            },
                        })
                        .collect(),
                ))
            }
        }
    }
}
