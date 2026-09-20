//! Traverse only the immutable parent observations named by each receipt.
use super::{experience::*, *};
use serde::Deserialize;
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperienceLineage {
    pub origin: ExperienceReceipt,
    pub ancestors: Vec<ExperienceEvent>,
    pub next_parent: Option<ExperienceParent>,
    pub next_parent_digest: Option<String>,
    pub complete: bool,
}
impl LearningMemory {
    /// Bounded traversal from immediate parent toward the root; no latest-state substitution.
    pub fn experience_lineage(
        &self,
        tenant: &str,
        namespace: &str,
        attempt: &str,
        max_depth: usize,
    ) -> Result<ExperienceLineage> {
        if !(1..=64).contains(&max_depth) {
            return Err(Error::ValidationError(
                "Lineage depth must be 1 to 64".into(),
            ));
        }
        let origin = self
            .experience(tenant, namespace, attempt)?
            .ok_or_else(|| Error::KeyNotFound("Experience attempt".into()))?
            .receipt;
        let mut parent = origin.request.parent.clone();
        let mut digest = origin.parent_event_digest.clone();
        let mut seen = BTreeSet::from([attempt.to_string()]);
        let mut ancestors = Vec::new();
        while ancestors.len() < max_depth {
            let Some(reference) = parent else {
                break;
            };
            if !seen.insert(reference.attempt_id.clone()) {
                return Err(Error::DataCorruption("Experience lineage cycle".into()));
            }
            let event = self
                .experience_event(
                    tenant,
                    namespace,
                    &reference.attempt_id,
                    &reference.event_id,
                )?
                .ok_or_else(|| Error::DataCorruption("Pinned ancestor event missing".into()))?;
            if digest.as_ref() != Some(&event.event_digest) {
                return Err(Error::DataCorruption(
                    "Pinned ancestor digest differs".into(),
                ));
            }
            parent = event.record.receipt.request.parent.clone();
            digest = event.record.receipt.parent_event_digest.clone();
            ancestors.push(event);
        }
        Ok(ExperienceLineage {
            origin,
            ancestors,
            complete: parent.is_none(),
            next_parent: parent,
            next_parent_digest: digest,
        })
    }
}
