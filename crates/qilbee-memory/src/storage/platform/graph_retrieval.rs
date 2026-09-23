//! Experimental retrieval through revision-bound typed paths in one scoped snapshot.
use super::snapshot::MemorySnapshot;
use super::*;
use std::collections::{BTreeMap, BTreeSet};
mod paths;
mod seeds;
mod types;
pub use types::*;

struct SeedHit {
    record: MemoryRecord,
    score: GraphSeedScore,
}
fn reference(record: &MemoryRecord) -> MemorySourceRef {
    MemorySourceRef {
        record_id: record.record_id,
        revision: record.revision,
    }
}
impl RocksDbMemoryStorage {
    /// Callers must authorize the namespace before invoking retrieval. All stages
    /// share a snapshot and clock; no embedding provider is contacted.
    pub fn search_memory_graph(
        &self,
        namespace: &str,
        query: &GraphRetrievalQuery,
    ) -> Result<GraphRetrievalPage> {
        self.memory_snapshot().search_graph(namespace, query)
    }
}
impl MemorySnapshot<'_> {
    fn search_graph(
        &self,
        namespace: &str,
        query: &GraphRetrievalQuery,
    ) -> Result<GraphRetrievalPage> {
        RocksDbMemoryStorage::validate_agent(namespace)?;
        if !(1..=100).contains(&query.limit) {
            return Err(Error::ValidationError(
                "Graph result limit must be 1 to 100".into(),
            ));
        }
        let profile = query.ranking_version.profile();
        query.expansion.validate(&profile)?;
        let (mut seed, seeds) = self.graph_seeds(namespace, query, &profile)?;
        seed.selected_anchors = seeds
            .iter()
            .take(profile.anchor_limit)
            .map(|s| reference(&s.record))
            .collect();
        let traversal_query = query.expansion.query(
            seed.selected_anchors.iter().map(|r| r.record_id).collect(),
            &profile,
        );
        let graph = if traversal_query.root_record_ids.is_empty() {
            TypedMemoryGraph {
                traversal_version: TYPED_GRAPH_TRAVERSAL_VERSION.into(),
                evaluated_at_millis: self.now,
                direction: traversal_query.direction,
                relation_kinds: traversal_query.relation_kinds.clone(),
                max_depth: traversal_query.max_depth,
                node_limit: traversal_query.node_limit,
                edge_limit: traversal_query.edge_limit,
                scan_limit: traversal_query.scan_limit,
                roots: vec![],
                nodes: vec![],
                edges: vec![],
                coverage: TypedGraphCoverage {
                    complete: true,
                    dependency_work: self.dependency_work(),
                    ..Default::default()
                },
            }
        } else {
            self.typed_graph_filtered(
                namespace,
                &traversal_query,
                query.episode_type.as_ref(),
                query.tag.as_deref(),
            )?
        };
        let (affinities, affinity_work) =
            self.graph_affinities(namespace, query, &seeds, &graph.nodes)?;
        let paths = paths::rank_paths(
            &profile,
            &seed.selected_anchors,
            &graph,
            &affinities,
            affinity_work.requested,
        );
        let graph_candidates = paths.len();
        let mut hits = BTreeMap::new();
        for (index, seed_hit) in seeds.into_iter().enumerate() {
            let rank = index + 1;
            let contribution = profile.base_weight / (profile.rank_constant + rank) as f64;
            let id = seed_hit.record.record_id;
            hits.insert(
                id,
                GraphRetrievalHit {
                    affinity: seed_hit.score.affinity(),
                    record: seed_hit.record,
                    score: contribution,
                    base: Some(GraphBaseContribution {
                        rank,
                        retrieval: seed_hit.score,
                        contribution,
                    }),
                    graph: None,
                },
            );
        }
        let records: BTreeMap<_, _> = graph
            .nodes
            .into_iter()
            .map(|n| (n.record.record_id, n.record))
            .collect();
        for (id, path) in paths {
            let hit = hits.entry(id).or_insert_with(|| GraphRetrievalHit {
                record: records[&id].clone(),
                score: 0.0,
                base: None,
                graph: None,
                affinity: None,
            });
            hit.affinity = affinities.get(&id).cloned();
            hit.score += path.contribution;
            hit.graph = Some(path);
        }
        let candidates_ranked = hits.len();
        let mut hits: Vec<_> = hits.into_values().collect();
        hits.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then_with(|| a.record.record_id.cmp(&b.record.record_id))
        });
        hits.truncate(query.limit);
        let proof_ids: BTreeSet<_> = hits
            .iter()
            .flat_map(|h| {
                h.graph
                    .iter()
                    .flat_map(|p| p.steps.iter().map(|s| s.relation_id))
            })
            .collect();
        let relations = graph
            .edges
            .into_iter()
            .filter(|r| proof_ids.contains(&r.relation_id))
            .collect();
        let source_complete = seed.scan_complete;
        let candidates_complete =
            !seed.base_candidates_truncated && !seed.channel_candidates_truncated;
        let graph_complete = graph.coverage.complete;
        let embeddings_complete = !matches!(
            seed.embedding_coverage,
            Some(EmbeddingCoverage::Missing | EmbeddingCoverage::Partial)
        ) && affinity_work.missing_bindings == 0
            && affinity_work.stale_bindings == 0;
        Ok(GraphRetrievalPage {
            ranking: profile,
            evaluated_at_millis: self.now,
            seed,
            expansion: query.expansion.clone(),
            graph_roots: graph.roots,
            graph_coverage: graph.coverage,
            affinity_work,
            graph_candidates,
            candidates_ranked,
            coverage: GraphRetrievalCoverage {
                complete: source_complete
                    && candidates_complete
                    && graph_complete
                    && embeddings_complete,
                source_complete,
                candidates_complete,
                graph_complete,
                embeddings_complete,
            },
            hits,
            relations,
        })
    }

    fn graph_affinities(
        &self,
        namespace: &str,
        query: &GraphRetrievalQuery,
        seeds: &[SeedHit],
        nodes: &[MemoryGraphNode],
    ) -> Result<(BTreeMap<Uuid, GraphAffinity>, GraphAffinityWork)> {
        let mut work = GraphAffinityWork {
            requested: query.seed.embedding_space().is_some(),
            byte_limit: query.expansion.embedding_bytes_limit,
            ..Default::default()
        };
        let mut affinities = BTreeMap::new();
        let Some(space) = query.seed.embedding_space() else {
            return Ok((affinities, work));
        };
        let vector = query.seed.vector().expect("vector mode has a query vector");
        let query_norm = super::semantic::norm(vector, space.dimensions)?;
        let known: BTreeMap<_, _> = seeds
            .iter()
            .filter_map(|s| s.score.affinity().map(|a| (s.record.record_id, a)))
            .collect();
        for node in nodes {
            let id = node.record.record_id;
            if let Some(affinity) = known.get(&id) {
                if affinity.embedding.record_revision != node.record.revision {
                    return Err(inconsistent());
                }
                work.reused_scores += 1;
                affinities.insert(id, affinity.clone());
                continue;
            }
            work.lookups += 1;
            let Some((binding, bytes)) = self.embedding(namespace, space, id)? else {
                work.missing_bindings += 1;
                continue;
            };
            work.bytes_read = work.bytes_read.saturating_add(bytes);
            if work.bytes_read > work.byte_limit {
                return Err(Error::ValidationError(
                    "Graph embedding byte budget exceeded".into(),
                ));
            }
            if binding.receipt.record_revision > node.record.revision {
                return Err(inconsistent());
            }
            if binding.receipt.record_revision < node.record.revision {
                work.stale_bindings += 1;
                continue;
            }
            work.current_bindings += 1;
            let denominator = query_norm * binding.validated_norm;
            let dot: f64 = vector
                .iter()
                .zip(&binding.vector)
                .map(|(&a, &b)| f64::from(a) * f64::from(b))
                .sum();
            affinities.insert(
                id,
                GraphAffinity {
                    cosine: (dot / denominator).clamp(-1.0, 1.0),
                    embedding: binding.receipt,
                },
            );
        }
        Ok((affinities, work))
    }
}

#[cfg(test)]
mod tests;
