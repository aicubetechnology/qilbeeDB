//! Bounded administrative discovery of historical evidence for a selected resource.
use super::*;
use crate::learning::{
    AdmissionReceipt, EvaluationReceipt, ExperienceEvent, RegisteredProcedure, ToolDevelopmentEvent,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LearningEvidenceKind {
    EvaluationSubmission,
    PairedEvaluation,
    ExperienceEvent,
    ToolDevelopmentEvent,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LearningEvidenceRef {
    pub resource: LearningResourceRef,
    pub kind: LearningEvidenceKind,
    pub id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LearningEvidenceQuery {
    pub resource: LearningResourceRef,
    pub kind: LearningEvidenceKind,
    pub text: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default = "default_scan")]
    pub max_scanned_records: usize,
    pub cursor: Option<LearningCatalogCursor>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "record",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum LearningEvidenceDetails {
    EvaluationSubmission(AdmissionReceipt),
    PairedEvaluation(EvaluationReceipt),
    ExperienceEvent(ExperienceEvent),
    ToolDevelopmentEvent(ToolDevelopmentEvent),
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LearningEvidenceEntry {
    pub evidence: LearningEvidenceRef,
    pub title: String,
    pub status: String,
    pub revision: Option<u64>,
    pub recorded_at_millis: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LearningEvidencePage {
    pub entries: Vec<LearningEvidenceEntry>,
    pub next_cursor: Option<LearningCatalogCursor>,
    pub stop_reason: LearningCatalogStop,
    pub scanned_records: usize,
    pub scanned_record_bytes: usize,
    pub observed_at_millis: i64,
}
fn state(value: impl Serialize) -> String {
    serde_json::to_value(value)
        .expect("serializable state")
        .as_str()
        .expect("string state")
        .into()
}
impl LearningEvidenceDetails {
    fn summary(&self, evidence: LearningEvidenceRef) -> LearningEvidenceEntry {
        let (title, status, revision, recorded_at_millis) = match self {
            Self::EvaluationSubmission(r) => (
                r.submission
                    .detail
                    .as_ref()
                    .unwrap_or(&r.submission.evidence_ref)
                    .as_str(),
                state(r.outcome),
                None,
                r.recorded_at_millis,
            ),
            Self::PairedEvaluation(r) => (
                r.evaluation.evidence_ref.as_str(),
                state(r.evaluation.phase),
                None,
                r.recorded_at_millis,
            ),
            Self::ExperienceEvent(r) => (
                r.command.evidence.reference.as_str(),
                state(r.command.outcome),
                Some(r.record.revision),
                r.recorded_at_millis,
            ),
            Self::ToolDevelopmentEvent(r) => (
                r.record.reason.as_str(),
                state(r.record.state),
                Some(r.record.revision),
                r.recorded_at_millis,
            ),
        };
        LearningEvidenceEntry {
            evidence,
            title: title
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(160)
                .collect(),
            status,
            revision,
            recorded_at_millis,
        }
    }
}
struct Selection {
    namespace: String,
    prefix: Vec<u8>,
    procedure: Option<RegisteredProcedure>,
}
fn entry_id<'a>(prefix: &[u8], key: &'a [u8]) -> Result<&'a str> {
    let mut suffix = key.strip_prefix(prefix).ok_or_else(corrupt)?;
    let id = component(&mut suffix)?;
    validate_text(id, "evidence ID", 512).map_err(|_| corrupt())?;
    if !suffix.is_empty() {
        return Err(corrupt());
    }
    Ok(id)
}
impl LearningMemory {
    fn evidence_selection(
        &self,
        company: &str,
        resource: &LearningResourceRef,
        kind: LearningEvidenceKind,
    ) -> Result<Selection> {
        validate_text(company, "company", 256)?;
        let namespace = resource
            .namespace(company)?
            .ok_or_else(|| invalid("This resource does not have historical evidence"))?;
        let valid = matches!(
            (resource.kind, kind),
            (
                LearningResourceKind::Procedure | LearningResourceKind::Strategy,
                LearningEvidenceKind::EvaluationSubmission | LearningEvidenceKind::PairedEvaluation
            ) | (
                LearningResourceKind::Experience,
                LearningEvidenceKind::ExperienceEvent
            ) | (
                LearningResourceKind::ToolDevelopment,
                LearningEvidenceKind::ToolDevelopmentEvent
            )
        );
        if !valid {
            return Err(invalid(
                "The evidence kind does not belong to this resource",
            ));
        }
        let details = self
            .catalog_details(company, resource)?
            .ok_or_else(|| Error::KeyNotFound("Learning resource not found".into()))?;
        let procedure = match details {
            LearningResourceDetails::Procedure(p) => Some(p),
            LearningResourceDetails::Strategy(s) => Some(s.procedure),
            _ => None,
        };
        let prefix = if kind == LearningEvidenceKind::PairedEvaluation {
            let bound = procedure.as_ref().ok_or_else(corrupt)?;
            let mut key = scope_prefix(2, &bound.record.scope);
            append_component(&mut key, &resource.id);
            key
        } else {
            let tag = match kind {
                LearningEvidenceKind::EvaluationSubmission => 7,
                LearningEvidenceKind::ExperienceEvent => 13,
                LearningEvidenceKind::ToolDevelopmentEvent => 11,
                _ => unreachable!(),
            };
            let mut key = vec![tag];
            for value in [company, &namespace, &resource.id] {
                append_component(&mut key, value);
            }
            key
        };
        Ok(Selection {
            namespace,
            prefix,
            procedure,
        })
    }
    fn evidence_details(
        &self,
        company: &str,
        evidence: &LearningEvidenceRef,
        selected: &Selection,
    ) -> Result<Option<LearningEvidenceDetails>> {
        let namespace = &selected.namespace;
        let parent = &evidence.resource.id;
        let id = &evidence.id;
        validate_text(id, "evidence ID", 512)?;
        Ok(match evidence.kind {
            LearningEvidenceKind::EvaluationSubmission => self
                .admission(company, namespace, parent, id)?
                .map(LearningEvidenceDetails::EvaluationSubmission),
            LearningEvidenceKind::ExperienceEvent => self
                .experience_event(company, namespace, parent, id)?
                .map(LearningEvidenceDetails::ExperienceEvent),
            LearningEvidenceKind::ToolDevelopmentEvent => self
                .tool_development_event(company, namespace, parent, id)?
                .map(LearningEvidenceDetails::ToolDevelopmentEvent),
            LearningEvidenceKind::PairedEvaluation => {
                let bound = selected.procedure.as_ref().ok_or_else(corrupt)?;
                self.evaluation(&bound.record.scope, parent, id)?
                    .map(|r| {
                        r.evaluation.validate().map_err(|_| corrupt())?;
                        if r.evaluation.case_id != *id
                            || r.evaluation.evaluator_id
                                != bound.record.proposal.policy.evaluator_id
                            || r.evaluation.evaluation_contract
                                != bound.record.proposal.policy.evaluation_contract
                        {
                            return Err(corrupt());
                        }
                        Ok(LearningEvidenceDetails::PairedEvaluation(r))
                    })
                    .transpose()?
            }
        })
    }
    /// Trusted administration. The HTTP layer must derive the company from current authority.
    /// Pages serialize with learning writes but do not share a snapshot or restoration witness.
    pub fn company_learning_evidence(
        &self,
        company: &str,
        query: &LearningEvidenceQuery,
    ) -> Result<LearningEvidencePage> {
        if !(1..=50).contains(&query.limit) || !(1..=1000).contains(&query.max_scanned_records) {
            return Err(invalid(
                "Evidence limit must be 1–50 and scan budget 1–1000",
            ));
        }
        if let Some(text) = &query.text {
            validate_text(text, "evidence text filter", 256)?;
        }
        let filter_digest = registry::digest(&(
            "learning-evidence-v1",
            &query.resource,
            query.kind,
            &query.text,
        ))?;
        let _guard = self
            .inner
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Learning mutation lock poisoned".into()))?;
        let selected = self.evidence_selection(company, &query.resource, query.kind)?;
        let prefix = &selected.prefix;
        let after = query
            .cursor
            .as_ref()
            .map(|c| {
                if c.version != 1 || c.company_id != company || c.filter_digest != filter_digest {
                    return Err(invalid(
                        "Evidence cursor does not match this company, resource, kind and filter",
                    ));
                }
                let key = unhex(&c.position)?;
                entry_id(prefix, &key).map_err(|_| invalid("Invalid evidence cursor position"))?;
                Ok(key)
            })
            .transpose()?;
        let mut options = rocksdb::ReadOptions::default();
        options.set_iterate_lower_bound(prefix.clone());
        let mut upper = prefix.clone();
        while upper.last() == Some(&255) {
            upper.pop();
        }
        *upper.last_mut().ok_or_else(corrupt)? += 1;
        options.set_iterate_upper_bound(upper);
        let mut iterator = self.inner.db.raw_iterator_opt(options);
        iterator.seek(after.as_deref().unwrap_or(prefix));
        if iterator
            .key()
            .is_some_and(|key| Some(key) == after.as_deref())
        {
            iterator.next();
        }
        let mut page = LearningEvidencePage {
            entries: vec![],
            next_cursor: None,
            stop_reason: LearningCatalogStop::Exhausted,
            scanned_records: 0,
            scanned_record_bytes: 0,
            observed_at_millis: chrono::Utc::now().timestamp_millis(),
        };
        let text = query.text.as_ref().map(|v| v.to_lowercase());
        let mut last = None;
        while let Some(key) = iterator.key() {
            if !key.starts_with(prefix) {
                return Err(corrupt());
            }
            if page.entries.len() == query.limit {
                page.stop_reason = LearningCatalogStop::EntryLimit;
                break;
            }
            if page.scanned_records == query.max_scanned_records {
                page.stop_reason = LearningCatalogStop::ScanLimit;
                break;
            }
            let bytes = key
                .len()
                .checked_add(iterator.value().ok_or_else(corrupt)?.len())
                .ok_or_else(corrupt)?;
            if bytes > SCAN_BYTES {
                return Err(Error::DataCorruption(
                    "Learning evidence record exceeds its 4 MiB inspection ceiling".into(),
                ));
            }
            if page.scanned_record_bytes + bytes > SCAN_BYTES {
                page.stop_reason = LearningCatalogStop::ByteLimit;
                break;
            }
            let id = entry_id(prefix, key)?.to_string();
            let evidence = LearningEvidenceRef {
                resource: query.resource.clone(),
                kind: query.kind,
                id,
            };
            let details = self
                .evidence_details(company, &evidence, &selected)?
                .ok_or_else(corrupt)?;
            let entry = details.summary(evidence);
            page.scanned_records += 1;
            page.scanned_record_bytes += bytes;
            if text.as_ref().is_none_or(|v| {
                entry.title.to_lowercase().contains(v)
                    || entry.evidence.id.to_lowercase().contains(v)
                    || entry.status.to_lowercase().contains(v)
            }) {
                page.entries.push(entry);
            }
            last = Some(key.to_vec());
            iterator.next();
        }
        iterator.status().map_err(storage_error)?;
        if page.stop_reason != LearningCatalogStop::Exhausted {
            page.next_cursor = Some(LearningCatalogCursor {
                version: 1,
                company_id: company.into(),
                filter_digest,
                position: hex(&last.ok_or_else(corrupt)?),
            });
        }
        Ok(page)
    }
    /// Read the selected immutable receipt; its state is historical, not a current reuse decision.
    pub fn inspect_company_learning_evidence(
        &self,
        company: &str,
        evidence: &LearningEvidenceRef,
    ) -> Result<Option<LearningEvidenceDetails>> {
        let _guard = self
            .inner
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Learning mutation lock poisoned".into()))?;
        let selected = self.evidence_selection(company, &evidence.resource, evidence.kind)?;
        self.evidence_details(company, evidence, &selected)
    }
}

#[cfg(test)]
mod tests;
