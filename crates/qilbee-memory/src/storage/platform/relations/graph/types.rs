use super::*;
pub const TYPED_GRAPH_TRAVERSAL_VERSION: &str = "typed_relations_v1";
pub const MAX_TYPED_GRAPH_EDGES: usize = 1024;
pub const MAX_TYPED_GRAPH_SCAN: usize = 4096;
pub const MAX_TYPED_GRAPH_RELATION_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TypedGraphDirection {
    Outgoing,
    Incoming,
    #[default]
    Both,
}
impl TypedGraphDirection {
    pub(super) fn indexes(&self) -> &'static [u8] {
        match self {
            Self::Outgoing => &[OUTGOING],
            Self::Incoming => &[INCOMING],
            Self::Both => &[OUTGOING, INCOMING],
        }
    }
}
fn default_kinds() -> Vec<MemoryRelationKind> {
    vec![
        MemoryRelationKind::SemanticRelated,
        MemoryRelationKind::SameEntity,
        MemoryRelationKind::TemporalBefore,
        MemoryRelationKind::CausalClaim,
        MemoryRelationKind::Supports,
        MemoryRelationKind::Contradicts,
    ]
}
fn default_depth() -> usize {
    2
}
fn default_nodes() -> usize {
    128
}
fn default_edges() -> usize {
    256
}
fn default_scan() -> usize {
    1024
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TypedMemoryGraphQuery {
    pub root_record_ids: Vec<Uuid>,
    #[serde(default)]
    pub direction: TypedGraphDirection,
    #[serde(default = "default_kinds")]
    pub relation_kinds: Vec<MemoryRelationKind>,
    #[serde(default = "default_depth")]
    pub max_depth: usize,
    #[serde(default = "default_nodes")]
    pub node_limit: usize,
    #[serde(default = "default_edges")]
    pub edge_limit: usize,
    #[serde(default = "default_scan")]
    pub scan_limit: usize,
}
impl TypedMemoryGraphQuery {
    pub(in crate::storage::platform) fn validate(&self) -> Result<()> {
        if !(1..=MAX_MEMORY_GRAPH_ROOTS).contains(&self.root_record_ids.len())
            || self.root_record_ids.iter().collect::<BTreeSet<_>>().len()
                != self.root_record_ids.len()
            || self.relation_kinds.is_empty()
            || self.relation_kinds.iter().collect::<BTreeSet<_>>().len()
                != self.relation_kinds.len()
            || self.max_depth > MAX_DERIVATION_DEPTH
            || !(1..=MAX_MEMORY_GRAPH_NODES).contains(&self.node_limit)
            || !(1..=MAX_TYPED_GRAPH_EDGES).contains(&self.edge_limit)
            || !(1..=MAX_TYPED_GRAPH_SCAN).contains(&self.scan_limit)
        {
            return Err(Error::ValidationError("Typed graph requires 1-16 distinct roots, nonempty unique relation kinds, depth 0-8, nodes 1-256, edges 1-1024 and scan limit 1-4096".into()));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TypedGraphStopReason {
    DepthLimit,
    NodeLimit,
    EdgeLimit,
    ScanLimit,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TypedGraphCoverage {
    pub complete: bool,
    pub stop_reasons: Vec<TypedGraphStopReason>,
    pub records_examined: usize,
    pub record_bytes: usize,
    pub adjacency_entries_examined: usize,
    pub relations_examined: usize,
    pub relation_bytes: usize,
    pub dependency_work: DependencyWork,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TypedMemoryGraph {
    pub traversal_version: String,
    pub evaluated_at_millis: i64,
    pub direction: TypedGraphDirection,
    pub relation_kinds: Vec<MemoryRelationKind>,
    pub max_depth: usize,
    pub node_limit: usize,
    pub edge_limit: usize,
    pub scan_limit: usize,
    pub roots: Vec<MemoryGraphRoot>,
    pub nodes: Vec<MemoryGraphNode>,
    pub edges: Vec<MemoryRelation>,
    pub coverage: TypedGraphCoverage,
}
