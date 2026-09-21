//! Bounded typed adjacency traversal over canonical memories in one authorized scope.
use super::*;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
mod types;
pub use types::*;

impl RocksDbMemoryStorage {
    /// Callers authorize the exact namespace before traversing any adjacency.
    pub fn read_memory_typed_graph(
        &self,
        namespace: &str,
        query: &TypedMemoryGraphQuery,
    ) -> Result<TypedMemoryGraph> {
        Self::validate_agent(namespace)?;
        query.validate()?;
        self.memory_snapshot().typed_graph(namespace, query)
    }
}

#[derive(Default)]
struct Traversal {
    records: BTreeMap<Uuid, Option<MemoryRecord>>,
    relations: BTreeMap<Uuid, (MemoryRelation, Vec<u8>)>,
    included: BTreeSet<Uuid>,
    edge_ids: BTreeSet<Uuid>,
    nodes: Vec<MemoryGraphNode>,
    edges: Vec<MemoryRelation>,
    queue: VecDeque<(Uuid, usize)>,
    deferred: Vec<(Uuid, TypedGraphStopReason)>,
    deferred_ids: BTreeSet<Uuid>,
    coverage: TypedGraphCoverage,
}
impl Traversal {
    fn include(&mut self, id: Uuid, depth: usize) {
        if self.included.insert(id) {
            self.nodes.push(MemoryGraphNode {
                record: self.records[&id].as_ref().unwrap().clone(),
                depth,
            });
            self.queue.push_back((id, depth));
        }
    }
    fn add_edge(&mut self, relation: &MemoryRelation) {
        if self.edge_ids.insert(relation.relation_id) {
            self.edges.push(relation.clone());
        }
    }
    fn defer(&mut self, id: Uuid, reason: TypedGraphStopReason) {
        if self.deferred_ids.insert(id) {
            self.deferred.push((id, reason));
        }
    }
    fn known_invalid(&self, reference: &MemorySourceRef) -> bool {
        self.records
            .get(&reference.record_id)
            .is_some_and(|r| r.as_ref().is_none_or(|r| r.revision != reference.revision))
    }
}

fn adjacency_prefix(kind: u8, namespace: &str, reference: &MemorySourceRef) -> Vec<u8> {
    let mut prefix = record_key(kind, namespace, reference.record_id);
    prefix.extend(reference.revision.to_be_bytes());
    prefix
}
fn prefix_end(prefix: &[u8]) -> Vec<u8> {
    let mut end = prefix.to_vec();
    while let Some(last) = end.pop() {
        if last != u8::MAX {
            end.push(last + 1);
            return end;
        }
    }
    unreachable!("Adjacency keys always begin with a non-maximal type byte")
}
impl MemorySnapshot<'_> {
    fn typed_graph_record(
        &self,
        namespace: &str,
        id: Uuid,
        query: &TypedMemoryGraphQuery,
        state: &mut Traversal,
        episode_type: Option<&EpisodeType>,
        tag: Option<&str>,
    ) -> Result<bool> {
        if state.records.contains_key(&id) {
            return Ok(true);
        }
        if state.coverage.records_examined == query.node_limit {
            return Ok(false);
        }
        let bytes = self
            .db
            .get_cf(
                self.storage.cf(super::super::super::cf::EPISODES)?,
                record_key(0x10, namespace, id),
            )
            .map_err(storage_error)?;
        state.coverage.record_bytes = state
            .coverage
            .record_bytes
            .saturating_add(bytes.as_ref().map_or(0, Vec::len));
        if state.coverage.record_bytes > MAX_MEMORY_READ_BYTES {
            return Err(Error::ValidationError(
                "Typed graph canonical byte budget exceeded".into(),
            ));
        }
        let index = self
            .db
            .get_cf(
                self.storage.cf(super::super::super::cf::EPISODE_INDEX)?,
                record_key(0x11, namespace, id),
            )
            .map_err(storage_error)?;
        let record = match decode_record_pair(id, bytes, index)? {
            Some(record)
                if self.eligible(namespace, &record)?
                    && record.payload.as_ref().is_some_and(|p| {
                        episode_type.is_none_or(|kind| kind == &p.episode_type)
                            && tag.is_none_or(|tag| p.tags.iter().any(|v| v == tag))
                    }) =>
            {
                Some(record)
            }
            _ => None,
        };
        state.coverage.records_examined += 1;
        state.records.insert(id, record);
        Ok(true)
    }

    pub(in crate::storage::platform) fn typed_graph(
        &self,
        namespace: &str,
        query: &TypedMemoryGraphQuery,
    ) -> Result<TypedMemoryGraph> {
        self.typed_graph_filtered(namespace, query, None, None)
    }
    pub(in crate::storage::platform) fn typed_graph_filtered(
        &self,
        namespace: &str,
        query: &TypedMemoryGraphQuery,
        episode_type: Option<&EpisodeType>,
        tag: Option<&str>,
    ) -> Result<TypedMemoryGraph> {
        let mut state = Traversal::default();
        let mut roots = Vec::new();
        let mut stops = BTreeSet::new();
        // Reserve no hidden budget: roots are actually read in request order before neighbors.
        for &id in &query.root_record_ids {
            let status =
                if !self.typed_graph_record(namespace, id, query, &mut state, episode_type, tag)? {
                    stops.insert(TypedGraphStopReason::NodeLimit);
                    MemoryGraphRootStatus::NotExamined
                } else if state.records[&id].is_some() {
                    state.include(id, 0);
                    MemoryGraphRootStatus::Included
                } else {
                    MemoryGraphRootStatus::Unavailable
                };
            roots.push(MemoryGraphRoot {
                record_id: id,
                status,
            });
        }
        let cf = self.storage.cf(super::super::super::cf::AGENT_META)?;
        'traversal: while let Some((id, depth)) = state.queue.pop_front() {
            let anchor = MemorySourceRef {
                record_id: id,
                revision: state.records[&id].as_ref().unwrap().revision,
            };
            for &direction in query.direction.indexes() {
                let prefix = adjacency_prefix(direction, namespace, &anchor);
                let expected = self.adjacency_head(&prefix)?;
                let mut observed_count = 0u64;
                let mut observed_digest = [0u8; 32];
                let mut options = rocksdb::ReadOptions::default();
                options.set_iterate_lower_bound(prefix.clone());
                options.set_iterate_upper_bound(prefix_end(&prefix));
                let mut iter = self.db.raw_iterator_cf_opt(cf, options);
                iter.seek(&prefix);
                loop {
                    iter.status().map_err(storage_error)?;
                    let Some(key) = iter.key() else {
                        if expected.count != observed_count
                            || expected.entries_digest != observed_digest
                        {
                            return Err(inconsistent());
                        }
                        break;
                    };
                    if !key.starts_with(&prefix) || key.len() != prefix.len() + 16 {
                        return Err(inconsistent());
                    }
                    // Inspect only this scoped key as lookahead; do not load its value beyond the budget.
                    if state.coverage.adjacency_entries_examined == query.scan_limit {
                        stops.insert(TypedGraphStopReason::ScanLimit);
                        break 'traversal;
                    }
                    let relation_id =
                        Uuid::from_slice(&key[prefix.len()..]).map_err(|_| inconsistent())?;
                    let value = iter.value().ok_or_else(inconsistent)?;
                    observed_count = observed_count.checked_add(1).ok_or_else(inconsistent)?;
                    if observed_count > expected.count {
                        return Err(inconsistent());
                    }
                    adjacency::accumulate(&mut observed_digest, key, value);
                    state.coverage.adjacency_entries_examined += 1;
                    if let std::collections::btree_map::Entry::Vacant(entry) =
                        state.relations.entry(relation_id)
                    {
                        let (relation, bytes, integrity) = self
                            .relation_with_bytes(namespace, relation_id)?
                            .ok_or_else(inconsistent)?;
                        state.coverage.relation_bytes =
                            state.coverage.relation_bytes.saturating_add(bytes);
                        if state.coverage.relation_bytes > MAX_TYPED_GRAPH_RELATION_BYTES {
                            return Err(Error::ValidationError(
                                "Typed graph relation byte budget exceeded".into(),
                            ));
                        }
                        state.coverage.relations_examined += 1;
                        entry.insert((relation, integrity));
                    }
                    let (relation, integrity) = &state.relations[&relation_id];
                    let (selected, neighbor) = if direction == OUTGOING {
                        (&relation.input.source, &relation.input.target)
                    } else {
                        (&relation.input.target, &relation.input.source)
                    };
                    if value != integrity || selected != &anchor || !relation.indexed() {
                        return Err(inconsistent());
                    }
                    let relation = relation.clone();
                    let neighbor = neighbor.clone();
                    iter.next();
                    if state.edge_ids.contains(&relation_id)
                        || !query.relation_kinds.contains(&relation.input.kind)
                        || local_relation_reason(&relation, self.now)
                            != RelationEligibilityReason::Eligible
                        || state.known_invalid(&neighbor)
                    {
                        continue;
                    }
                    if !state.included.contains(&neighbor.record_id) && depth == query.max_depth {
                        state.defer(relation_id, TypedGraphStopReason::DepthLimit);
                        continue;
                    }
                    if !self.typed_graph_record(
                        namespace,
                        neighbor.record_id,
                        query,
                        &mut state,
                        episode_type,
                        tag,
                    )? {
                        state.defer(relation_id, TypedGraphStopReason::NodeLimit);
                        continue;
                    }
                    if state.known_invalid(&neighbor) {
                        continue;
                    }
                    if state.edges.len() == query.edge_limit {
                        state.defer(relation_id, TypedGraphStopReason::EdgeLimit);
                        continue;
                    }
                    state.include(neighbor.record_id, depth + 1);
                    state.add_edge(&relation);
                }
            }
        }
        // An endpoint omitted at a boundary can be discovered through another path.
        // Resolve those edges after discovery, without additional reads or invented reverse edges.
        for (id, reason) in state.deferred.clone() {
            let relation = state.relations[&id].0.clone();
            if state.edge_ids.contains(&id)
                || state.known_invalid(&relation.input.source)
                || state.known_invalid(&relation.input.target)
            {
                continue;
            }
            if state.included.contains(&relation.input.source.record_id)
                && state.included.contains(&relation.input.target.record_id)
            {
                if state.edges.len() < query.edge_limit {
                    state.add_edge(&relation);
                } else {
                    stops.insert(TypedGraphStopReason::EdgeLimit);
                }
            } else {
                stops.insert(reason);
            }
        }
        state.coverage.complete = stops.is_empty();
        state.coverage.stop_reasons = stops.into_iter().collect();
        state.coverage.dependency_work = self.dependency_work();
        Ok(TypedMemoryGraph {
            traversal_version: TYPED_GRAPH_TRAVERSAL_VERSION.into(),
            evaluated_at_millis: self.now,
            direction: query.direction,
            relation_kinds: query.relation_kinds.clone(),
            max_depth: query.max_depth,
            node_limit: query.node_limit,
            edge_limit: query.edge_limit,
            scan_limit: query.scan_limit,
            roots,
            nodes: state.nodes,
            edges: state.edges,
            coverage: state.coverage,
        })
    }
}

#[cfg(test)]
mod tests;
