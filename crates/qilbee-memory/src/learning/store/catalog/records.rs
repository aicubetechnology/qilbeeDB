use super::*;
use crate::learning::{
    EvaluationContext, ExperienceRecord, PolicyDefinition, RegisteredProcedure, RegistryEntry,
    StrategyCandidateReceipt, ToolArtifact, ToolDevelopment, ToolExecutor,
};

/// Exact stored identities and current states. These records do not establish
/// experimental truth, execution isolation, or suitability for agent reuse.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "record",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum LearningResourceDetails {
    Experience(ExperienceRecord),
    Procedure(RegisteredProcedure),
    Strategy(StrategyDetails),
    ToolArtifact(ToolArtifact),
    ToolDevelopment(ToolDevelopment),
    Policy(RegistryEntry<PolicyDefinition>),
    Context(RegistryEntry<EvaluationContext>),
    Executor(ToolExecutor),
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StrategyDetails {
    pub candidate: StrategyCandidateReceipt,
    pub procedure: RegisteredProcedure,
}
fn status(value: impl Serialize) -> String {
    // All callers supply enums whose wire representation is a string.
    serde_json::to_value(value)
        .expect("serializable resource state")
        .as_str()
        .expect("string resource state")
        .to_lowercase()
}
fn title(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(160)
        .collect()
}
impl LearningResourceDetails {
    pub(super) fn summary(&self, resource: LearningResourceRef) -> LearningCatalogEntry {
        let (name, state, revision, timestamp) = match self {
            Self::Experience(r) => (
                &r.receipt.request.input.reference,
                r.outcome.map(status).unwrap_or_else(|| "unreported".into()),
                Some(r.revision),
                r.receipt.recorded_at_millis,
            ),
            Self::Procedure(r) => (
                &r.record.proposal.task,
                status(r.record.state),
                None,
                r.record.created_at_millis,
            ),
            Self::Strategy(r) => (
                &r.candidate.request.instructions,
                status(r.procedure.record.state),
                None,
                r.procedure.record.created_at_millis,
            ),
            Self::ToolArtifact(r) => (
                &r.proposal.entrypoint,
                "registered".into(),
                None,
                r.recorded_at_millis,
            ),
            Self::ToolDevelopment(r) => (
                &r.receipt.request.objective,
                status(r.state),
                Some(r.revision),
                r.receipt.recorded_at_millis,
            ),
            Self::Policy(r) => (&r.id, "registered".into(), None, r.recorded_at_millis),
            Self::Context(r) => (
                &r.payload.task,
                "registered".into(),
                None,
                r.recorded_at_millis,
            ),
            Self::Executor(r) => (
                &r.profile.id,
                "registered".into(),
                None,
                r.recorded_at_millis,
            ),
        };
        LearningCatalogEntry {
            resource,
            title: title(name),
            status: state,
            revision,
            recorded_at_millis: timestamp,
        }
    }
}
impl LearningMemory {
    pub(super) fn catalog_summary(
        &self,
        company: &str,
        details: &LearningResourceDetails,
        resource: LearningResourceRef,
    ) -> Result<LearningCatalogEntry> {
        let mut entry = details.summary(resource);
        if let LearningResourceDetails::Experience(record) = details {
            let context = self
                .context(company, &record.receipt.request.context_id)?
                .ok_or_else(corrupt)?;
            entry.title = title(&context.payload.task);
        }
        Ok(entry)
    }
    pub(super) fn catalog_details(
        &self,
        company: &str,
        resource: &LearningResourceRef,
    ) -> Result<Option<LearningResourceDetails>> {
        let namespace = resource.namespace(company)?;
        // Global resources never consume a namespace; validation rejects ambiguous selections.
        let namespace = namespace.as_deref().unwrap_or("");
        let id = &resource.id;
        Ok(match resource.kind {
            LearningResourceKind::Experience => self
                .experience(company, namespace, id)?
                .map(LearningResourceDetails::Experience),
            LearningResourceKind::Procedure => self
                .registered_procedure(company, namespace, id)?
                .map(LearningResourceDetails::Procedure),
            LearningResourceKind::Strategy => {
                match self.strategy_candidate(company, namespace, id)? {
                    Some(candidate) => Some(LearningResourceDetails::Strategy(StrategyDetails {
                        candidate,
                        procedure: self
                            .registered_procedure(company, namespace, id)?
                            .ok_or_else(corrupt)?,
                    })),
                    None => None,
                }
            }
            LearningResourceKind::ToolArtifact => self
                .tool_artifact(company, namespace, id)?
                .map(LearningResourceDetails::ToolArtifact),
            LearningResourceKind::ToolDevelopment => self
                .tool_development(company, namespace, id)?
                .map(LearningResourceDetails::ToolDevelopment),
            LearningResourceKind::Policy => self
                .policy(company, id)?
                .map(LearningResourceDetails::Policy),
            LearningResourceKind::Context => self
                .context(company, id)?
                .map(LearningResourceDetails::Context),
            LearningResourceKind::Executor => self
                .tool_executor(company, id)?
                .map(LearningResourceDetails::Executor),
        })
    }
}
