use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationChangeStream {
    TypedMemoryRelations,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelationChangeCursor {
    pub version: u32,
    pub stream: RelationChangeStream,
    pub journal_id: Uuid,
    pub sequence: u64,
    pub prefix_digest: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelationChange {
    pub schema_version: u32,
    pub kind: RelationAction,
    pub relation_id: Uuid,
    pub relation_revision: u64,
    pub source: MemorySourceRef,
    pub target: MemorySourceRef,
    pub relation_kind: MemoryRelationKind,
    pub author: RecordAuthor,
    pub committed_at_millis: i64,
    pub relation_digest: String,
    pub receipt_digest: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelationChangeDelivery {
    pub cursor: RelationChangeCursor,
    pub change: RelationChange,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelationChangesQuery {
    pub after: Option<RelationChangeCursor>,
    pub through: Option<RelationChangeCursor>,
    pub limit: usize,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelationChangesPage {
    pub active: bool,
    pub baseline: Option<RelationChangeCursor>,
    pub changes: Vec<RelationChangeDelivery>,
    pub next_cursor: Option<RelationChangeCursor>,
    pub high_watermark: Option<RelationChangeCursor>,
    pub complete: bool,
}
