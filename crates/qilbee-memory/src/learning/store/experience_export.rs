//! Reproducible export of an explicit observation cohort, without promotion or inference.
use super::{experience::*, *};
use serde::Deserialize;
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperienceExportRef {
    pub attempt_id: String,
    pub event_id: String,
    pub event_digest: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperienceExportRequest {
    pub context_digest: String,
    pub accounting_unit: String,
    pub events: Vec<ExperienceExportRef>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperienceAccountingSummary {
    pub known_reports: u32,
    pub unknown_reports: u32,
    pub reported_total: Option<String>,
    pub observed_lower_bound: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperienceExportSummary {
    pub attempts: u32,
    pub succeeded: u32,
    pub failed: u32,
    pub cancelled: u32,
    pub unknown: u32,
    pub cost_units: ExperienceAccountingSummary,
    pub latency_ms: ExperienceAccountingSummary,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperienceExport {
    pub method_version: String,
    pub coverage: String,
    pub context_digest: String,
    pub accounting_unit: String,
    pub events: Vec<ExperienceEvent>,
    pub summary: ExperienceExportSummary,
    pub export_digest: String,
}
fn accounting(
    values: impl Iterator<Item = (Option<u64>, Option<u64>)>,
) -> ExperienceAccountingSummary {
    let mut known = 0;
    let mut unknown = 0;
    let mut reported = 0u128;
    let mut lower = 0u128;
    for (report, observed) in values {
        match report {
            Some(value) => {
                known += 1;
                reported += u128::from(value);
            }
            None => unknown += 1,
        }
        lower += u128::from(observed.unwrap_or(0));
    }
    ExperienceAccountingSummary {
        known_reports: known,
        unknown_reports: unknown,
        reported_total: (unknown == 0).then(|| reported.to_string()),
        observed_lower_bound: lower.to_string(),
    }
}
impl LearningMemory {
    /// All-or-error export; caller supplies the frozen cohort, not a query over live state.
    pub fn export_experiences(
        &self,
        tenant: &str,
        namespace: &str,
        request: ExperienceExportRequest,
    ) -> Result<ExperienceExport> {
        experience::validate_digest(&request.context_digest)?;
        validate_text(&request.accounting_unit, "accounting unit", 512)?;
        if !(1..=64).contains(&request.events.len()) {
            return Err(Error::ValidationError(
                "An export requires 1 to 64 distinct attempts".into(),
            ));
        }
        let mut seen = BTreeSet::new();
        let mut events = Vec::new();
        for reference in request.events {
            experience::validate_digest(&reference.event_digest)?;
            if !seen.insert(reference.attempt_id.clone()) {
                return Err(Error::ValidationError(
                    "An attempt may occur only once per export".into(),
                ));
            }
            let event = self
                .experience_event(
                    tenant,
                    namespace,
                    &reference.attempt_id,
                    &reference.event_id,
                )?
                .ok_or_else(|| Error::KeyNotFound("Export observation".into()))?;
            if event.event_digest != reference.event_digest
                || event.record.receipt.context_digest != request.context_digest
                || event.record.receipt.request.accounting_unit != request.accounting_unit
            {
                return Err(Error::ConstraintViolation(
                    "Export digest, context or accounting unit differs".into(),
                ));
            }
            events.push(event);
        }
        events.sort_by(|a, b| {
            a.record
                .receipt
                .request
                .id
                .cmp(&b.record.receipt.request.id)
        });
        let mut summary = ExperienceExportSummary {
            attempts: events.len() as u32,
            succeeded: 0,
            failed: 0,
            cancelled: 0,
            unknown: 0,
            cost_units: accounting(
                events
                    .iter()
                    .map(|e| (e.record.reported_cost_units, e.record.observed_cost_units)),
            ),
            latency_ms: accounting(
                events
                    .iter()
                    .map(|e| (e.record.reported_latency_ms, e.record.observed_latency_ms)),
            ),
        };
        for event in &events {
            match event.command.outcome {
                ExperienceOutcome::Succeeded => summary.succeeded += 1,
                ExperienceOutcome::Failed => summary.failed += 1,
                ExperienceOutcome::Cancelled => summary.cancelled += 1,
                ExperienceOutcome::Unknown => summary.unknown += 1,
            }
        }
        let mut export = ExperienceExport {
            method_version: "qilbee.experience-export.v1".into(),
            coverage: "explicit_event_set".into(),
            context_digest: request.context_digest,
            accounting_unit: request.accounting_unit,
            events,
            summary,
            export_digest: String::new(),
        };
        export.export_digest = registry::digest(&(
            &export.method_version,
            &export.coverage,
            &export.context_digest,
            &export.accounting_unit,
            &export.events,
            &export.summary,
        ))?;
        Ok(export)
    }
}
