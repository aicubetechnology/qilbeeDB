use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsolidationSpec {
    pub sources: Vec<MemorySourceRef>,
    pub objective: String,
    pub policy_ref: String,
    pub extractor: RelationProvenance,
    pub max_relations: usize,
    pub max_attempts: u32,
    pub lease_millis: u64,
    pub max_attempt_millis: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum ConsolidationUsage {
    Unknown,
    Reported {
        model_calls: u64,
        input_tokens: u64,
        output_tokens: u64,
        cost_microusd: Option<u64>,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsolidationStatus {
    Ready,
    Running,
    Published,
    Cancelled,
    Exhausted,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsolidationOutcome {
    Running,
    Published,
    Failed,
    Unknown,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsolidationAttempt {
    pub number: u32,
    pub worker_id: String,
    pub credential_id: Uuid,
    pub fence: Uuid,
    pub storage_incarnation: Uuid,
    pub claimed_at_millis: i64,
    pub expires_at_millis: i64,
    pub ended_at_millis: Option<i64>,
    pub outcome: ConsolidationOutcome,
    pub usage: ConsolidationUsage,
    pub evidence_ref: Option<String>,
    pub usage_evidence_ref: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsolidationAssertion {
    pub source: MemorySourceRef,
    pub target: MemorySourceRef,
    pub kind: MemoryRelationKind,
    pub valid_from_millis: Option<i64>,
    pub valid_until_millis: Option<i64>,
    pub evidence_ref: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsolidationJob {
    pub schema_version: u32,
    pub job_id: Uuid,
    pub revision: u64,
    pub created_by: RecordAuthor,
    pub created_at_millis: i64,
    pub modified_at_millis: i64,
    pub spec: ConsolidationSpec,
    pub status: ConsolidationStatus,
    pub attempts: Vec<ConsolidationAttempt>,
    pub output_receipts: Vec<MemoryRelationReceipt>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ConsolidationOperation {
    Create {
        spec: ConsolidationSpec,
    },
    Claim {
        job_id: Uuid,
        expected_revision: u64,
        worker_id: String,
    },
    Renew {
        job_id: Uuid,
        expected_revision: u64,
        fence: Uuid,
    },
    Publish {
        job_id: Uuid,
        expected_revision: u64,
        fence: Uuid,
        assertions: Vec<ConsolidationAssertion>,
        usage: ConsolidationUsage,
        evidence_ref: String,
    },
    Fail {
        job_id: Uuid,
        expected_revision: u64,
        fence: Uuid,
        usage: ConsolidationUsage,
        evidence_ref: String,
    },
    RecoverExpired {
        job_id: Uuid,
        expected_revision: u64,
        evidence_ref: String,
    },
    Cancel {
        job_id: Uuid,
        expected_revision: u64,
        evidence_ref: String,
    },
    ReconcileUsage {
        job_id: Uuid,
        expected_revision: u64,
        attempt_number: u32,
        usage: ConsolidationUsage,
        evidence_ref: String,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsolidationCommand {
    pub contract_version: u32,
    pub idempotency_key: String,
    pub operation: ConsolidationOperation,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsolidationAction {
    Created,
    Claimed,
    Renewed,
    Published,
    Failed,
    Recovered,
    Cancelled,
    UsageReconciled,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsolidationReceipt {
    pub contract_version: u32,
    pub idempotency_key: String,
    pub job_id: Uuid,
    pub revision: u64,
    pub action: ConsolidationAction,
    pub author: RecordAuthor,
    pub committed_at_millis: i64,
    pub job_digest: String,
    pub command_digest: String,
    pub receipt_digest: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsolidationRevision {
    pub job: ConsolidationJob,
    pub receipt: ConsolidationReceipt,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsolidationInspection {
    pub job: ConsolidationJob,
    pub evaluated_at_millis: i64,
    pub lease_active: bool,
    pub recoverable: bool,
    pub source_failure: Option<MemoryEligibilityFailure>,
    pub dependency_work: DependencyWork,
}
impl ConsolidationSpec {
    pub(super) fn validate(&self) -> Result<()> {
        let ids: BTreeSet<_> = self.sources.iter().map(|s| s.record_id).collect();
        if !(2..=16).contains(&self.sources.len())
            || ids.len() != self.sources.len()
            || self.sources.iter().any(|s| s.revision == 0)
            || !valid(&self.objective, 4096)
            || !valid(&self.policy_ref, 2048)
            || !(1..=16).contains(&self.max_relations)
            || !(1..=32).contains(&self.max_attempts)
            || !(1_000..=900_000).contains(&self.lease_millis)
            || !(self.lease_millis..=86_400_000).contains(&self.max_attempt_millis)
        {
            return Err(Error::ValidationError("Consolidation requires 2-16 unique source revisions, an objective and policy reference, 1-16 outputs, 1-32 attempts, a 1-900 second lease and at most 24 hours per attempt".into()));
        }
        // Use the same provenance contract as the assertions this job can publish.
        MemoryRelationInput {
            source: self.sources[0].clone(),
            target: self.sources[1].clone(),
            kind: MemoryRelationKind::SemanticRelated,
            provenance: self.extractor.clone(),
            valid_from_millis: None,
            valid_until_millis: None,
            evidence_sources: Vec::new(),
        }
        .validate()
    }
}
impl ConsolidationOperation {
    pub(super) fn target(&self) -> Option<(Uuid, u64)> {
        match self {
            Self::Create { .. } => None,
            Self::Claim {
                job_id,
                expected_revision,
                ..
            }
            | Self::Renew {
                job_id,
                expected_revision,
                ..
            }
            | Self::Publish {
                job_id,
                expected_revision,
                ..
            }
            | Self::Fail {
                job_id,
                expected_revision,
                ..
            }
            | Self::RecoverExpired {
                job_id,
                expected_revision,
                ..
            }
            | Self::Cancel {
                job_id,
                expected_revision,
                ..
            }
            | Self::ReconcileUsage {
                job_id,
                expected_revision,
                ..
            } => Some((*job_id, *expected_revision)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsolidationQuery {
    pub limit: usize,
    pub scan_limit: usize,
    pub after: Option<Uuid>,
    pub status: Option<ConsolidationStatus>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsolidationSummary {
    pub job_id: Uuid,
    pub revision: u64,
    pub status: ConsolidationStatus,
    pub created_at_millis: i64,
    pub modified_at_millis: i64,
    pub attempts: usize,
    pub lease_active: bool,
    pub recoverable: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsolidationPage {
    pub jobs: Vec<ConsolidationSummary>,
    pub next_after: Option<Uuid>,
    pub records_examined: usize,
    pub record_bytes: usize,
    pub evaluated_at_millis: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsolidationContext {
    pub job_id: Uuid,
    pub job_revision: u64,
    pub fence: Uuid,
    pub records: Vec<MemoryRecord>,
    pub evaluated_at_millis: i64,
    pub record_bytes: usize,
    pub dependency_work: DependencyWork,
}

/// Current workspace jobs across owners; transport requires native company administration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdminConsolidationCursor {
    pub owner_id: String,
    pub job_id: Uuid,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdminConsolidationQuery {
    pub limit: usize,
    pub scan_limit: usize,
    pub after: Option<AdminConsolidationCursor>,
    pub status: Option<ConsolidationStatus>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdminConsolidationEntry {
    pub owner_id: String,
    pub summary: ConsolidationSummary,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdminConsolidationPage {
    pub jobs: Vec<AdminConsolidationEntry>,
    pub next_after: Option<AdminConsolidationCursor>,
    pub records_examined: usize,
    pub record_bytes: usize,
    pub evaluated_at_millis: i64,
}
