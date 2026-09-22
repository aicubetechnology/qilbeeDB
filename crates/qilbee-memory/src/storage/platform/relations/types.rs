//! Typed assertions remain claims, even when an authorized reviewer approves them.
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryRelationKind {
    SemanticRelated,
    SameEntity,
    TemporalBefore,
    CausalClaim,
    Supports,
    Contradicts,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationOrigin {
    ModelInference,
    ToolObservation,
    HumanStatement,
    ImportedAssertion,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelationModelIdentity {
    pub provider: String,
    pub model: String,
    pub revision: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelationProvenance {
    pub origin: RelationOrigin,
    pub method: String,
    pub method_revision: String,
    pub evidence_ref: String,
    pub model: Option<RelationModelIdentity>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryRelationInput {
    pub source: MemorySourceRef,
    pub target: MemorySourceRef,
    pub kind: MemoryRelationKind,
    pub provenance: RelationProvenance,
    pub valid_from_millis: Option<i64>,
    pub valid_until_millis: Option<i64>,
    /// Additional same-scope context used to infer the assertion, beyond its endpoints.
    /// Omission preserves the exact serialized form of pre-extension relations.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_sources: Vec<MemorySourceRef>,
}
impl MemoryRelationInput {
    pub(in crate::storage::platform) fn validate(&self) -> Result<()> {
        let p = &self.provenance;
        let mut evidence_ids = std::collections::BTreeSet::new();
        if self.evidence_sources.len() > MAX_MEMORY_SOURCES
            || self.evidence_sources.iter().any(|reference| {
                reference.revision == 0
                    || reference.record_id == self.source.record_id
                    || reference.record_id == self.target.record_id
                    || !evidence_ids.insert(reference.record_id)
            })
        {
            return Err(Error::ValidationError(
                "Relation evidence requires at most 16 unique positive same-scope source revisions, distinct from both endpoints".into(),
            ));
        }
        if self.source.record_id == self.target.record_id
            || self.source.revision == 0
            || self.target.revision == 0
            || !valid(&p.method, 256)
            || !valid(&p.method_revision, 256)
            || !valid(&p.evidence_ref, 2048)
            || (p.origin == RelationOrigin::ModelInference && p.model.is_none())
            || p.model.as_ref().is_some_and(|m| {
                !valid(&m.provider, 256) || !valid(&m.model, 256) || !valid(&m.revision, 256)
            })
            || [self.valid_from_millis, self.valid_until_millis]
                .into_iter()
                .flatten()
                .any(|t| chrono::DateTime::from_timestamp_millis(t).is_none())
            || self
                .valid_from_millis
                .zip(self.valid_until_millis)
                .is_some_and(|(from, until)| from >= until)
        {
            return Err(Error::ValidationError("Relations require distinct positive endpoint revisions, versioned provenance and an ordered validity interval; model inference requires complete model identity".into()));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum MemoryRelationOperation {
    Assert {
        relation: MemoryRelationInput,
    },
    Retire {
        relation_id: Uuid,
        expected_revision: u64,
        evidence_ref: String,
    },
    Restore {
        relation_id: Uuid,
        expected_revision: u64,
        evidence_ref: String,
    },
    Review {
        relation_id: Uuid,
        expected_revision: u64,
        disposition: MemoryReviewDisposition,
        evidence_ref: String,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryRelationCommand {
    pub contract_version: u32,
    pub idempotency_key: String,
    pub operation: MemoryRelationOperation,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationState {
    Active,
    Retired,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryRelation {
    pub schema_version: u32,
    pub relation_id: Uuid,
    pub revision: u64,
    pub input: MemoryRelationInput,
    pub reported_by: RecordAuthor,
    pub created_at_millis: i64,
    pub modified_at_millis: i64,
    pub state: RelationState,
    pub review: Option<MemoryReview>,
}
impl MemoryRelation {
    pub(super) fn indexed(&self) -> bool {
        self.state == RelationState::Active
            && self
                .review
                .as_ref()
                .is_none_or(|r| r.disposition != MemoryReviewDisposition::Rejected)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationAction {
    Asserted,
    Retired,
    Restored,
    Reviewed,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryRelationReceipt {
    pub contract_version: u32,
    pub idempotency_key: String,
    pub relation_id: Uuid,
    pub revision: u64,
    pub action: RelationAction,
    pub author: RecordAuthor,
    pub committed_at_millis: i64,
    pub evidence_ref: String,
    pub relation_digest: String,
    pub command_digest: String,
    pub receipt_digest: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationEligibilityReason {
    Eligible,
    Retired,
    Rejected,
    NotYetValid,
    Expired,
    EndpointUnavailable,
    EndpointRevisionChanged,
    EvidenceUnavailable,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelationEligibility {
    pub eligible: bool,
    pub reason: RelationEligibilityReason,
    pub evaluated_at_millis: i64,
    pub endpoint: Option<MemorySourceRef>,
    pub dependency_work: DependencyWork,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_failure: Option<MemoryEligibilityFailure>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryRelationInspection {
    pub relation: MemoryRelation,
    pub eligibility: RelationEligibility,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryRelationRevision {
    pub relation: MemoryRelation,
    pub receipt: MemoryRelationReceipt,
}
