use super::*;

pub const MAX_GRAPH_AFFINITY_BYTES: usize = 8 * 1024 * 1024;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphRankingVersion {
    TypedPathBalancedV1,
    TypedPathEntityV1,
    TypedPathTemporalV1,
    TypedPathEvidenceV1,
    TypedPathBasePreservingV1,
    TypedPathBestChannelV1,
    TypedPathStrengthV1,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphRelationWeight {
    pub kind: MemoryRelationKind,
    pub weight: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphRankingProfile {
    pub version: GraphRankingVersion,
    pub method: String,
    pub experimental: bool,
    pub traversal_version: String,
    pub base_candidate_limit: usize,
    pub anchor_limit: usize,
    pub rank_constant: usize,
    pub base_weight: f64,
    pub graph_weight: f64,
    pub hop_decay: f64,
    pub cosine_affinity_floor: f64,
    pub cosine_affinity_weight: f64,
    pub missing_embedding_affinity: f64,
    pub relation_weights: Vec<GraphRelationWeight>,
    pub maximum_score: f64,
}
impl GraphRankingVersion {
    pub const ALL: [Self; 7] = [
        Self::TypedPathBalancedV1,
        Self::TypedPathEntityV1,
        Self::TypedPathTemporalV1,
        Self::TypedPathEvidenceV1,
        Self::TypedPathBasePreservingV1,
        Self::TypedPathBestChannelV1,
        Self::TypedPathStrengthV1,
    ];
    pub fn profile(self) -> GraphRankingProfile {
        use MemoryRelationKind::*;
        let weights = match self {
            Self::TypedPathBalancedV1
            | Self::TypedPathBasePreservingV1
            | Self::TypedPathBestChannelV1
            | Self::TypedPathStrengthV1 => [1.0, 1.0, 1.0, 1.0, 1.0, 1.0],
            Self::TypedPathEntityV1 => [0.5, 1.0, 0.0, 0.0, 0.0, 0.0],
            Self::TypedPathTemporalV1 => [0.25, 0.5, 1.0, 0.0, 0.0, 0.0],
            Self::TypedPathEvidenceV1 => [0.25, 0.5, 0.25, 0.75, 1.0, 1.0],
        };
        let (base_weight, graph_weight) = match self {
            Self::TypedPathStrengthV1 => (0.5, 0.5),
            Self::TypedPathBasePreservingV1 => (0.75, 0.25),
            Self::TypedPathBestChannelV1 => (1.0, 1.0),
            _ => (0.25, 0.75),
        };
        GraphRankingProfile {
            version: self,
            method: if self == Self::TypedPathStrengthV1 {
                "strongest_typed_path_strength"
            } else if self == Self::TypedPathBestChannelV1 {
                "strongest_typed_path_max"
            } else {
                "strongest_typed_path_rrf"
            }
            .into(),
            experimental: true,
            traversal_version: TYPED_GRAPH_TRAVERSAL_VERSION.into(),
            base_candidate_limit: 100,
            anchor_limit: 4,
            rank_constant: 2,
            base_weight,
            graph_weight,
            hop_decay: 0.5,
            cosine_affinity_floor: 0.5,
            cosine_affinity_weight: 0.5,
            missing_embedding_affinity: 0.5,
            relation_weights: [
                SemanticRelated,
                SameEntity,
                TemporalBefore,
                CausalClaim,
                Supports,
                Contradicts,
            ]
            .into_iter()
            .zip(weights)
            .map(|(kind, weight)| GraphRelationWeight { kind, weight })
            .collect(),
            maximum_score: if self == Self::TypedPathBestChannelV1 {
                1.0 / 3.0
            } else {
                base_weight / 3.0 + graph_weight / 3.0
            },
        }
    }
}
fn min_score() -> f64 {
    -1.0
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum GraphSeedQuery {
    Lexical {
        text: String,
    },
    Semantic {
        space: EmbeddingSpace,
        vector: Vec<f32>,
        #[serde(default = "min_score")]
        min_score: f64,
    },
    Hybrid {
        text: String,
        space: EmbeddingSpace,
        vector: Vec<f32>,
        ranking_version: HybridRankingVersion,
        #[serde(default = "min_score")]
        min_score: f64,
    },
}
impl GraphSeedQuery {
    pub fn embedding_space(&self) -> Option<&EmbeddingSpace> {
        match self {
            Self::Semantic { space, .. } | Self::Hybrid { space, .. } => Some(space),
            _ => None,
        }
    }
    pub(super) fn vector(&self) -> Option<&[f32]> {
        match self {
            Self::Semantic { vector, .. } | Self::Hybrid { vector, .. } => Some(vector),
            _ => None,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphExpansionLimits {
    pub direction: TypedGraphDirection,
    pub max_depth: usize,
    pub node_limit: usize,
    pub edge_limit: usize,
    pub scan_limit: usize,
    pub embedding_bytes_limit: usize,
}
impl Default for GraphExpansionLimits {
    fn default() -> Self {
        Self {
            direction: TypedGraphDirection::Both,
            max_depth: 2,
            node_limit: 128,
            edge_limit: 256,
            scan_limit: 1024,
            embedding_bytes_limit: MAX_GRAPH_AFFINITY_BYTES,
        }
    }
}
impl GraphExpansionLimits {
    pub(super) fn query(
        &self,
        roots: Vec<Uuid>,
        profile: &GraphRankingProfile,
    ) -> TypedMemoryGraphQuery {
        TypedMemoryGraphQuery {
            root_record_ids: roots,
            direction: self.direction,
            relation_kinds: profile
                .relation_weights
                .iter()
                .filter(|w| w.weight > 0.0)
                .map(|w| w.kind)
                .collect(),
            max_depth: self.max_depth,
            node_limit: self.node_limit,
            edge_limit: self.edge_limit,
            scan_limit: self.scan_limit,
        }
    }
    pub(super) fn validate(&self, profile: &GraphRankingProfile) -> Result<()> {
        // Validate the traversal's operational bounds before selecting real roots or reading storage.
        self.query(vec![Uuid::nil()], profile).validate()?;
        if !(1..=MAX_GRAPH_AFFINITY_BYTES).contains(&self.embedding_bytes_limit) {
            return Err(Error::ValidationError(
                "Graph embedding byte limit must be 1 to 8388608".into(),
            ));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphRetrievalQuery {
    pub ranking_version: GraphRankingVersion,
    pub seed: GraphSeedQuery,
    pub limit: usize,
    #[serde(default = "super::super::lexical::default_scan_limit")]
    pub scan_limit: usize,
    #[serde(default = "super::super::lexical::default_scan_bytes_limit")]
    pub scan_bytes_limit: usize,
    #[serde(default)]
    pub expansion: GraphExpansionLimits,
    pub episode_type: Option<EpisodeType>,
    pub tag: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum GraphSeedScore {
    Lexical {
        score: f64,
    },
    Semantic {
        score: f64,
        embedding: EmbeddingReceipt,
    },
    Hybrid {
        score: f64,
        lexical: Option<RankContribution>,
        semantic: Option<RankContribution>,
        embedding: Option<EmbeddingReceipt>,
    },
}
impl GraphSeedScore {
    pub(super) fn affinity(&self) -> Option<GraphAffinity> {
        match self {
            Self::Semantic { score, embedding } => Some(GraphAffinity {
                cosine: *score,
                embedding: embedding.clone(),
            }),
            Self::Hybrid {
                semantic: Some(s),
                embedding: Some(e),
                ..
            } => Some(GraphAffinity {
                cosine: s.score,
                embedding: e.clone(),
            }),
            _ => None,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphSeedMetadata {
    pub mode: String,
    pub ranking_version: String,
    pub hybrid_profile: Option<HybridRankingProfile>,
    pub embedding_space: Option<EmbeddingSpace>,
    pub embedding_coverage: Option<EmbeddingCoverage>,
    pub selected_anchors: Vec<MemorySourceRef>,
    pub candidate_selection_version: String,
    pub scanned_records: usize,
    pub scanned_bytes: usize,
    pub candidate_index_bytes: usize,
    pub corpus_records: Option<usize>,
    pub lexical_matches: Option<usize>,
    pub semantic_matches: Option<usize>,
    pub base_candidates: usize,
    pub base_candidates_truncated: bool,
    pub channel_candidates_truncated: bool,
    pub scan_complete: bool,
    pub dependency_work: DependencyWork,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphBaseContribution {
    pub rank: usize,
    pub retrieval: GraphSeedScore,
    pub contribution: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphAffinity {
    pub cosine: f64,
    pub embedding: EmbeddingReceipt,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphStepDirection {
    Outgoing,
    Incoming,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphPathStep {
    pub relation_id: Uuid,
    pub relation_revision: u64,
    pub direction: GraphStepDirection,
    pub from: MemorySourceRef,
    pub to: MemorySourceRef,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphPathContribution {
    pub rank: usize,
    pub strength: f64,
    pub contribution: f64,
    pub anchor: MemorySourceRef,
    pub anchor_rank: usize,
    pub steps: Vec<GraphPathStep>,
    pub missing_affinity: Vec<MemorySourceRef>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphAffinityWork {
    pub requested: bool,
    pub reused_scores: usize,
    pub lookups: usize,
    pub current_bindings: usize,
    pub missing_bindings: usize,
    pub stale_bindings: usize,
    pub bytes_read: usize,
    pub byte_limit: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphRetrievalHit {
    pub record: MemoryRecord,
    pub score: f64,
    pub base: Option<GraphBaseContribution>,
    pub graph: Option<GraphPathContribution>,
    pub affinity: Option<GraphAffinity>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphRetrievalCoverage {
    pub complete: bool,
    pub source_complete: bool,
    pub candidates_complete: bool,
    pub graph_complete: bool,
    pub embeddings_complete: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphRetrievalPage {
    pub ranking: GraphRankingProfile,
    pub evaluated_at_millis: i64,
    pub seed: GraphSeedMetadata,
    pub expansion: GraphExpansionLimits,
    pub graph_roots: Vec<MemoryGraphRoot>,
    pub graph_coverage: TypedGraphCoverage,
    pub affinity_work: GraphAffinityWork,
    pub graph_candidates: usize,
    pub candidates_ranked: usize,
    pub coverage: GraphRetrievalCoverage,
    pub hits: Vec<GraphRetrievalHit>,
    pub relations: Vec<MemoryRelation>,
}
