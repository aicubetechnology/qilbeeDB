//! Outgoing evidence ancestry from canonical revisioned memories in one snapshot.
use super::snapshot::MemorySnapshot;
use super::*;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub const MAX_MEMORY_GRAPH_ROOTS: usize = 16;
pub const MAX_MEMORY_GRAPH_NODES: usize = 256;
pub const MEMORY_GRAPH_TRAVERSAL_VERSION: &str = "derived_ancestry_v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryGraphQuery {
    pub root_record_ids: Vec<Uuid>,
    #[serde(default = "default_depth")]
    pub max_depth: usize,
    #[serde(default = "default_nodes")]
    pub node_limit: usize,
}
fn default_depth() -> usize {
    4
}
fn default_nodes() -> usize {
    128
}
impl MemoryGraphQuery {
    pub(super) fn validate(&self) -> Result<()> {
        if !(1..=MAX_MEMORY_GRAPH_ROOTS).contains(&self.root_record_ids.len())
            || self.root_record_ids.iter().collect::<BTreeSet<_>>().len()
                != self.root_record_ids.len()
            || self.max_depth > MAX_DERIVATION_DEPTH
            || !(1..=MAX_MEMORY_GRAPH_NODES).contains(&self.node_limit)
        {
            return Err(Error::ValidationError(
                "Evidence graph requires 1-16 distinct roots, depth 0-8 and node limit 1-256"
                    .into(),
            ));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryGraphRootStatus {
    Included,
    Unavailable,
    NotExamined,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryGraphRoot {
    pub record_id: Uuid,
    pub status: MemoryGraphRootStatus,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryGraphNode {
    pub record: MemoryRecord,
    pub depth: usize,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryGraphRelation {
    DerivedFrom,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryGraphEdge {
    pub source: MemorySourceRef,
    pub target: MemorySourceRef,
    pub relation: MemoryGraphRelation,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryGraphStopReason {
    DepthLimit,
    NodeLimit,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryGraphCoverage {
    pub complete: bool,
    pub stop_reasons: Vec<MemoryGraphStopReason>,
    /// Completed canonical record lookups, including missing/ineligible records.
    pub records_examined: usize,
    /// Serialized canonical record bytes, excluding indexes and dependency reads.
    pub record_bytes: usize,
    pub dependency_work: DependencyWork,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryEvidenceGraph {
    pub traversal_version: String,
    pub evaluated_at_millis: i64,
    pub max_depth: usize,
    pub node_limit: usize,
    pub roots: Vec<MemoryGraphRoot>,
    pub nodes: Vec<MemoryGraphNode>,
    pub edges: Vec<MemoryGraphEdge>,
    pub coverage: MemoryGraphCoverage,
}
impl RocksDbMemoryStorage {
    /// The caller authorizes the exact namespace before this read. Eligibility
    /// checks always inspect complete dependencies, regardless of display depth.
    pub fn read_memory_graph(
        &self,
        namespace: &str,
        query: &MemoryGraphQuery,
    ) -> Result<MemoryEvidenceGraph> {
        Self::validate_agent(namespace)?;
        query.validate()?;
        self.memory_snapshot().evidence_graph(namespace, query)
    }
}
impl MemorySnapshot<'_> {
    pub(super) fn evidence_graph(
        &self,
        namespace: &str,
        query: &MemoryGraphQuery,
    ) -> Result<MemoryEvidenceGraph> {
        let mut roots: Vec<_> = query
            .root_record_ids
            .iter()
            .map(|&record_id| MemoryGraphRoot {
                record_id,
                status: MemoryGraphRootStatus::NotExamined,
            })
            .collect();
        let mut queue: VecDeque<_> = query.root_record_ids.iter().map(|&id| (id, 0)).collect();
        let mut discovered: BTreeSet<_> = query.root_record_ids.iter().copied().collect();
        let mut nodes = Vec::new();
        let mut records_examined = 0;
        let mut record_bytes = 0usize;
        while let Some((id, depth)) = queue.pop_front() {
            if records_examined == query.node_limit {
                break;
            }
            let bytes = self
                .db
                .get_cf(
                    self.storage.cf(super::super::cf::EPISODES)?,
                    record_key(0x10, namespace, id),
                )
                .map_err(storage_error)?;
            record_bytes = record_bytes.saturating_add(bytes.as_ref().map_or(0, Vec::len));
            // Fetching the crossing value is necessary to learn its size; this
            // decoded-byte budget is not a cap on RSS or JSON response bytes.
            if record_bytes > MAX_MEMORY_READ_BYTES {
                return Err(Error::ValidationError(
                    "Evidence graph byte budget exhausted; reduce roots or node limit".into(),
                ));
            }
            let index = self
                .db
                .get_cf(
                    self.storage.cf(super::super::cf::EPISODE_INDEX)?,
                    record_key(0x11, namespace, id),
                )
                .map_err(storage_error)?;
            let record = match decode_record_pair(id, bytes, index)? {
                Some(record) if self.eligible(namespace, &record)? => Some(record),
                _ => None,
            };
            records_examined += 1;
            if let Some(root) = roots.iter_mut().find(|root| root.record_id == id) {
                root.status = if record.is_some() {
                    MemoryGraphRootStatus::Included
                } else {
                    MemoryGraphRootStatus::Unavailable
                };
            }
            let Some(record) = record else {
                continue;
            };
            if depth < query.max_depth {
                for source in ordered_sources(&record) {
                    if discovered.insert(source.record_id) {
                        queue.push_back((source.record_id, depth + 1));
                    }
                }
            }
            nodes.push(MemoryGraphNode { record, depth });
        }
        let included: BTreeMap<_, _> = nodes
            .iter()
            .map(|node| (node.record.record_id, node.record.revision))
            .collect();
        let mut edges = Vec::new();
        let mut stops = BTreeSet::new();
        if roots
            .iter()
            .any(|root| root.status == MemoryGraphRootStatus::NotExamined)
        {
            stops.insert(MemoryGraphStopReason::NodeLimit);
        }
        for node in &nodes {
            for target in ordered_sources(&node.record) {
                if included.get(&target.record_id) == Some(&target.revision) {
                    edges.push(MemoryGraphEdge {
                        source: MemorySourceRef {
                            record_id: node.record.record_id,
                            revision: node.record.revision,
                        },
                        target,
                        relation: MemoryGraphRelation::DerivedFrom,
                    });
                } else {
                    stops.insert(if node.depth == query.max_depth {
                        MemoryGraphStopReason::DepthLimit
                    } else {
                        MemoryGraphStopReason::NodeLimit
                    });
                }
            }
        }
        Ok(MemoryEvidenceGraph {
            traversal_version: MEMORY_GRAPH_TRAVERSAL_VERSION.into(),
            evaluated_at_millis: self.now,
            max_depth: query.max_depth,
            node_limit: query.node_limit,
            roots,
            nodes,
            edges,
            coverage: MemoryGraphCoverage {
                complete: stops.is_empty(),
                stop_reasons: stops.into_iter().collect(),
                records_examined,
                record_bytes,
                dependency_work: self.dependency_work(),
            },
        })
    }
}
fn ordered_sources(record: &MemoryRecord) -> Vec<MemorySourceRef> {
    let mut sources = record
        .derivation
        .as_ref()
        .map(|d| d.sources.clone())
        .unwrap_or_default();
    sources.sort_by_key(|source| source.record_id);
    sources
}

#[cfg(test)]
mod tests;
