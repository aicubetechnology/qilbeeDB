//! Bounded transitive provenance checks and metadata-only explanations.
use super::snapshot::MemorySnapshot;
use super::*;
use std::collections::{BTreeMap, BTreeSet};
pub const MAX_DERIVATION_DEPTH: usize = 8;
pub const MAX_DERIVATION_NODES: usize = 64;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryEligibilityReason {
    Eligible,
    Deleted,
    Expired,
    Rejected,
    SourceMissing,
    SourceRevisionChanged,
    DependencyCycle,
    DepthLimit,
    NodeLimit,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryEligibilityFailure {
    pub record_id: Uuid,
    pub expected_revision: Option<u64>,
    pub actual_revision: Option<u64>,
    pub reason: MemoryEligibilityReason,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryEligibility {
    pub record_id: Uuid,
    pub revision: u64,
    pub eligible: bool,
    pub evaluated_at_millis: i64,
    pub first_failure: Option<MemoryEligibilityFailure>,
    pub dependencies_checked: usize,
    pub max_depth_examined: usize,
    pub all_dependencies_checked: bool,
    pub dependency_work: DependencyWork,
}
/// Current evidence observation for exact knowledge source revisions.
/// No payloads are returned. Failure short-circuits; success checks every root.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryEvidenceEligibility {
    pub eligible: bool,
    pub evaluated_at_millis: i64,
    pub first_failure: Option<MemoryEligibilityFailure>,
    pub all_dependencies_checked: bool,
    pub dependency_work: DependencyWork,
}

pub(crate) struct MemoryEvidenceView<'a> {
    snapshot: MemorySnapshot<'a>,
}
impl RocksDbMemoryStorage {
    pub(crate) fn evidence_view(&self) -> MemoryEvidenceView<'_> {
        MemoryEvidenceView {
            snapshot: self.memory_snapshot(),
        }
    }
    /// Inspect exact sources in one authorized namespace and one request snapshot.
    pub fn inspect_memory_evidence(
        &self,
        namespace: &str,
        sources: &[MemorySourceRef],
    ) -> Result<MemoryEvidenceEligibility> {
        self.evidence_view().inspect(namespace, sources)
    }
}
impl MemoryEvidenceView<'_> {
    pub(crate) fn observed_at_millis(&self) -> i64 {
        self.snapshot.now
    }
    pub(crate) fn inspect(
        &self,
        namespace: &str,
        sources: &[MemorySourceRef],
    ) -> Result<MemoryEvidenceEligibility> {
        RocksDbMemoryStorage::validate_agent(namespace)?;
        let ids: BTreeSet<_> = sources.iter().map(|source| source.record_id).collect();
        if sources.is_empty()
            || sources.len() > MAX_MEMORY_SOURCES
            || ids.len() != sources.len()
            || sources.iter().any(|s| s.revision == 0)
        {
            return Err(Error::ValidationError(
                "Evidence requires 1-16 unique positive memory source revisions".into(),
            ));
        }
        let snapshot = &self.snapshot;
        let first_failure = snapshot.relation_evidence_failure(namespace, sources)?;
        Ok(MemoryEvidenceEligibility {
            eligible: first_failure.is_none(),
            evaluated_at_millis: snapshot.now,
            all_dependencies_checked: first_failure.is_none(),
            first_failure,
            dependency_work: snapshot.dependency_work(),
        })
    }
}

pub(super) fn record_reason(record: &MemoryRecord, now: i64) -> MemoryEligibilityReason {
    if record.payload.is_none() {
        MemoryEligibilityReason::Deleted
    } else if record
        .review
        .as_ref()
        .is_some_and(|r| r.disposition == MemoryReviewDisposition::Rejected)
    {
        MemoryEligibilityReason::Rejected
    } else if !visible(record, now) {
        MemoryEligibilityReason::Expired
    } else {
        MemoryEligibilityReason::Eligible
    }
}
#[derive(Default)]
struct Walk {
    active: BTreeSet<Uuid>,
    seen: BTreeSet<Uuid>,
    heights: BTreeMap<Uuid, usize>,
    max_depth: usize,
    failure: Option<MemoryEligibilityFailure>,
}
impl Walk {
    fn fail(
        &mut self,
        id: Uuid,
        expected: Option<u64>,
        actual: Option<u64>,
        reason: MemoryEligibilityReason,
    ) -> Option<usize> {
        self.failure = Some(MemoryEligibilityFailure {
            record_id: id,
            expected_revision: expected,
            actual_revision: actual,
            reason,
        });
        None
    }
}
impl MemorySnapshot<'_> {
    /// Check all declared relation context in one bounded walk and dependency cache.
    /// Context roots have the same depth origin as directly read canonical memories.
    pub(super) fn relation_evidence_failure(
        &self,
        namespace: &str,
        sources: &[MemorySourceRef],
    ) -> Result<Option<MemoryEligibilityFailure>> {
        let mut walk = Walk::default();
        for source in sources {
            if self.walk_source(namespace, source, 0, &mut walk)?.is_none() {
                break;
            }
        }
        Ok(walk.failure)
    }

    fn walk_source(
        &self,
        namespace: &str,
        source: &MemorySourceRef,
        depth: usize,
        walk: &mut Walk,
    ) -> Result<Option<usize>> {
        let id = source.record_id;
        let expected = Some(source.revision);
        walk.max_depth = walk.max_depth.max(depth);
        if depth > MAX_DERIVATION_DEPTH {
            return Ok(walk.fail(id, expected, None, MemoryEligibilityReason::DepthLimit));
        }
        if walk.active.contains(&id) {
            return Ok(walk.fail(id, expected, None, MemoryEligibilityReason::DependencyCycle));
        }
        if !walk.seen.contains(&id) && walk.seen.len() == MAX_DERIVATION_NODES {
            return Ok(walk.fail(id, expected, None, MemoryEligibilityReason::NodeLimit));
        }
        walk.seen.insert(id);
        let Some(record) = self.dependency(namespace, id)? else {
            return Ok(walk.fail(id, expected, None, MemoryEligibilityReason::SourceMissing));
        };
        let actual = Some(record.revision);
        if record.revision != source.revision {
            return Ok(walk.fail(
                id,
                expected,
                actual,
                MemoryEligibilityReason::SourceRevisionChanged,
            ));
        }
        if record.reason != MemoryEligibilityReason::Eligible {
            return Ok(walk.fail(id, expected, actual, record.reason));
        }
        if let Some(height) = walk.heights.get(&id).copied() {
            walk.max_depth = walk.max_depth.max(depth + height);
            if depth + height > MAX_DERIVATION_DEPTH {
                return Ok(walk.fail(id, expected, actual, MemoryEligibilityReason::DepthLimit));
            }
            return Ok(Some(height));
        }
        walk.active.insert(id);
        let mut height = 0;
        if let Some(derivation) = record.derivation {
            derivation.validate().map_err(|_| inconsistent())?;
            for child in &derivation.sources {
                let Some(child_height) = self.walk_source(namespace, child, depth + 1, walk)?
                else {
                    return Ok(None);
                };
                height = height.max(child_height + 1);
            }
        }
        walk.active.remove(&id);
        walk.heights.insert(id, height);
        Ok(Some(height))
    }
    fn walk_derivation(
        &self,
        namespace: &str,
        root: Option<Uuid>,
        derivation: &MemoryDerivation,
    ) -> Result<Walk> {
        derivation.validate().map_err(|_| inconsistent())?;
        let mut walk = Walk::default();
        if let Some(id) = root {
            walk.active.insert(id);
        }
        for source in &derivation.sources {
            if self.walk_source(namespace, source, 1, &mut walk)?.is_none() {
                break;
            }
        }
        Ok(walk)
    }
    pub(super) fn eligible(&self, namespace: &str, record: &MemoryRecord) -> Result<bool> {
        Ok(self.explain_eligibility(namespace, record)?.eligible)
    }
    pub(super) fn explain_eligibility(
        &self,
        namespace: &str,
        record: &MemoryRecord,
    ) -> Result<MemoryEligibility> {
        let reason = record_reason(record, self.now);
        let walk = if reason != MemoryEligibilityReason::Eligible {
            Walk {
                failure: Some(MemoryEligibilityFailure {
                    record_id: record.record_id,
                    expected_revision: Some(record.revision),
                    actual_revision: Some(record.revision),
                    reason,
                }),
                ..Default::default()
            }
        } else if let Some(d) = &record.derivation {
            self.walk_derivation(namespace, Some(record.record_id), d)?
        } else {
            Walk::default()
        };
        let eligible = walk.failure.is_none();
        Ok(MemoryEligibility {
            record_id: record.record_id,
            revision: record.revision,
            eligible,
            evaluated_at_millis: self.now,
            first_failure: walk.failure,
            dependencies_checked: walk.seen.len(),
            max_depth_examined: walk.max_depth,
            all_dependencies_checked: eligible,
            dependency_work: self.dependency_work(),
        })
    }
    pub(super) fn validate_derivation_sources(
        &self,
        namespace: &str,
        derivation: &MemoryDerivation,
    ) -> Result<()> {
        derivation.validate()?;
        if let Some(failure) = self.walk_derivation(namespace, None, derivation)?.failure {
            return Err(match failure.reason {
                MemoryEligibilityReason::SourceMissing => {
                    Error::KeyNotFound("Eligible source memory".into())
                }
                MemoryEligibilityReason::DepthLimit
                | MemoryEligibilityReason::NodeLimit
                | MemoryEligibilityReason::DependencyCycle => Error::ValidationError(
                    "Derivation exceeds graph bounds or contains a cycle".into(),
                ),
                _ => Error::TransactionConflict("Source revision is no longer eligible".into()),
            });
        }
        Ok(())
    }
}
impl RocksDbMemoryStorage {
    /// The caller must authorize metadata inspection in this exact namespace.
    pub fn explain_memory_eligibility(
        &self,
        namespace: &str,
        id: Uuid,
    ) -> Result<Option<MemoryEligibility>> {
        Self::validate_agent(namespace)?;
        let snapshot = self.memory_snapshot();
        snapshot
            .record(namespace, id)?
            .as_ref()
            .map(|r| snapshot.explain_eligibility(namespace, r))
            .transpose()
    }
}
