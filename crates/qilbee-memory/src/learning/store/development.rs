//! Revision-checked development receipts from separately deployed workers.
use super::{executors::ToolExecutor, tools::*, *};
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDevelopmentRequest {
    pub id: String,
    pub executor_id: String,
    pub objective: String,
    pub parent_artifact_id: Option<String>,
    pub repair_evidence_ref: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDevelopmentReceipt {
    pub schema_version: u32,
    pub tenant: String,
    pub namespace: String,
    pub request: ToolDevelopmentRequest,
    pub request_digest: String,
    pub executor_profile_digest: String,
    pub actor: ToolActor,
    pub recorded_at_millis: i64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolDevelopmentState {
    Requested,
    PendingOrUnknown,
    CancellationRequested,
    Succeeded,
    Failed,
    Cancelled,
}
impl ToolDevelopmentState {
    fn terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDevelopment {
    pub receipt: ToolDevelopmentReceipt,
    pub revision: u64,
    pub state: ToolDevelopmentState,
    pub reason: String,
    pub last_event_id: Option<String>,
    pub artifact_id: Option<String>,
    pub artifact_digest: Option<String>,
    pub cost_units: Option<u64>,
    pub latency_ms: Option<u64>,
    pub observed_cost_units: Option<u64>,
    pub observed_latency_ms: Option<u64>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolReportOutcome {
    PendingOrUnknown,
    Succeeded,
    Failed,
    Cancelled,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDevelopmentReport {
    pub executor_profile_digest: String,
    pub outcome: ToolReportOutcome,
    pub evidence_ref: String,
    pub detail: Option<String>,
    pub cost_units: Option<u64>,
    pub latency_ms: Option<u64>,
    pub artifact: Option<ToolArtifactProposal>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ToolDevelopmentAction {
    Report { report: ToolDevelopmentReport },
    RequestCancellation { reason: String },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDevelopmentCommand {
    pub event_id: String,
    pub expected_revision: u64,
    pub action: ToolDevelopmentAction,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDevelopmentEvent {
    pub schema_version: u32,
    pub command: ToolDevelopmentCommand,
    pub actor: ToolActor,
    pub record: ToolDevelopment,
    pub recorded_at_millis: i64,
}
fn event_key(tenant: &str, namespace: &str, id: &str, event: &str) -> Result<Vec<u8>> {
    let mut key = tool_key(11, tenant, namespace, id)?;
    validate_text(event, "development event ID", 512)?;
    append_component(&mut key, event);
    Ok(key)
}
impl LearningMemory {
    pub fn create_tool_development(
        &self,
        tenant: &str,
        namespace: &str,
        request: ToolDevelopmentRequest,
        actor: ToolActor,
    ) -> Result<ToolDevelopmentReceipt> {
        let key = tool_key(10, tenant, namespace, &request.id)?;
        actor.validate()?;
        validate_text(&request.objective, "development objective", 8192)?;
        let _guard = self
            .inner
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Learning mutation lock poisoned".into()))?;
        if let Some(existing) = self.tool_development(tenant, namespace, &request.id)? {
            return if existing.receipt.request == request
                && existing.receipt.actor.subject_id == actor.subject_id
            {
                Ok(existing.receipt)
            } else {
                Err(Error::ConstraintViolation(
                    "Development request IDs are immutable".into(),
                ))
            };
        }
        let executor = self
            .tool_executor(tenant, &request.executor_id)?
            .ok_or_else(|| Error::KeyNotFound("Executor profile".into()))?;
        match (&request.parent_artifact_id, &request.repair_evidence_ref) {
            (None, None) => (),
            (Some(parent), Some(evidence)) => {
                validate_text(evidence, "repair evidence", 2048)?;
                self.tool_artifact(tenant, namespace, parent)?
                    .ok_or_else(|| Error::KeyNotFound("Repair parent artifact".into()))?;
            }
            _ => {
                return Err(Error::ValidationError(
                    "Repair requires both a parent and evidence".into(),
                ));
            }
        }
        let receipt = ToolDevelopmentReceipt {
            schema_version: 1,
            tenant: tenant.into(),
            namespace: namespace.into(),
            request_digest: registry::digest(&request)?,
            executor_profile_digest: executor.profile_digest,
            request,
            actor,
            recorded_at_millis: chrono::Utc::now().timestamp_millis(),
        };
        let record = ToolDevelopment {
            receipt: receipt.clone(),
            revision: 1,
            state: ToolDevelopmentState::Requested,
            reason: "request_recorded".into(),
            last_event_id: None,
            artifact_id: None,
            artifact_digest: None,
            cost_units: None,
            latency_ms: None,
            observed_cost_units: None,
            observed_latency_ms: None,
        };
        self.inner
            .db
            .put_opt(key, encode(&record)?, &write_options())
            .map_err(storage_error)?;
        Ok(receipt)
    }
    pub fn tool_development(
        &self,
        tenant: &str,
        namespace: &str,
        id: &str,
    ) -> Result<Option<ToolDevelopment>> {
        self.inner
            .db
            .get(tool_key(10, tenant, namespace, id)?)
            .map_err(storage_error)?
            .map(|bytes| {
                let record: ToolDevelopment = decode(&bytes)?;
                self.verify_development(tenant, namespace, id, &record)?;
                match &record.last_event_id {
                    Some(event) => {
                        let event = self
                            .tool_development_event(tenant, namespace, id, event)?
                            .ok_or_else(|| {
                                Error::DataCorruption("Development event missing".into())
                            })?;
                        if event.record != record {
                            return Err(Error::DataCorruption(
                                "Development state differs from its event".into(),
                            ));
                        }
                    }
                    None if record.revision == 1
                        && record.state == ToolDevelopmentState::Requested
                        && record.artifact_id.is_none()
                        && record.cost_units.is_none()
                        && record.latency_ms.is_none() =>
                    {
                        ()
                    }
                    _ => {
                        return Err(Error::DataCorruption(
                            "Development state has no originating event".into(),
                        ));
                    }
                }
                Ok(record)
            })
            .transpose()
    }
    fn verify_development(
        &self,
        tenant: &str,
        namespace: &str,
        id: &str,
        record: &ToolDevelopment,
    ) -> Result<ToolExecutor> {
        let receipt = &record.receipt;
        if receipt.schema_version != 1
            || receipt.tenant != tenant
            || receipt.namespace != namespace
            || receipt.request.id != id
            || receipt.request_digest != registry::digest(&receipt.request)?
        {
            return Err(Error::DataCorruption(
                "Development identity or digest mismatch".into(),
            ));
        }
        let executor = self
            .tool_executor(tenant, &receipt.request.executor_id)?
            .ok_or_else(|| Error::DataCorruption("Development executor missing".into()))?;
        if executor.profile_digest != receipt.executor_profile_digest {
            return Err(Error::DataCorruption("Development executor changed".into()));
        }
        if record.state == ToolDevelopmentState::Succeeded {
            let artifact_id = record.artifact_id.as_deref().ok_or_else(|| {
                Error::DataCorruption("Successful development has no artifact".into())
            })?;
            let artifact = self
                .tool_artifact(tenant, namespace, artifact_id)?
                .ok_or_else(|| Error::DataCorruption("Successful artifact missing".into()))?;
            if record.artifact_digest.as_deref() != Some(&artifact.artifact_digest)
                || record.cost_units.is_none()
                || record.latency_ms.is_none()
            {
                return Err(Error::DataCorruption(
                    "Successful artifact or accounting mismatch".into(),
                ));
            }
        } else if record.artifact_id.is_some() || record.artifact_digest.is_some() {
            return Err(Error::DataCorruption(
                "Non-successful development exposes an artifact".into(),
            ));
        }
        Ok(executor)
    }
    pub fn tool_development_event(
        &self,
        tenant: &str,
        namespace: &str,
        id: &str,
        event: &str,
    ) -> Result<Option<ToolDevelopmentEvent>> {
        self.inner
            .db
            .get(event_key(tenant, namespace, id, event)?)
            .map_err(storage_error)?
            .map(|bytes| {
                let receipt: ToolDevelopmentEvent = decode(&bytes)?;
                if receipt.schema_version != 1
                    || receipt.command.event_id != event
                    || receipt.record.last_event_id.as_deref() != Some(event)
                    || receipt.command.expected_revision.checked_add(1)
                        != Some(receipt.record.revision)
                {
                    return Err(Error::DataCorruption(
                        "Development event identity or revision mismatch".into(),
                    ));
                }
                self.verify_development(tenant, namespace, id, &receipt.record)?;
                Ok(receipt)
            })
            .transpose()
    }
    /// Persist one command and its resulting state in the same synchronous batch.
    pub fn apply_tool_development(
        &self,
        tenant: &str,
        namespace: &str,
        id: &str,
        command: ToolDevelopmentCommand,
        actor: ToolActor,
    ) -> Result<ToolDevelopmentEvent> {
        let key = event_key(tenant, namespace, id, &command.event_id)?;
        actor.validate()?;
        let _guard = self
            .inner
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Learning mutation lock poisoned".into()))?;
        let mut record = self
            .tool_development(tenant, namespace, id)?
            .ok_or_else(|| Error::KeyNotFound("Development request".into()))?;
        let executor = self.verify_development(tenant, namespace, id, &record)?;
        let expected_subject = match &command.action {
            ToolDevelopmentAction::Report { .. } => &executor.profile.subject_id,
            ToolDevelopmentAction::RequestCancellation { .. } => &record.receipt.actor.subject_id,
        };
        if &actor.subject_id != expected_subject {
            return Err(Error::Unauthorized(
                "Development actor does not match the bound subject".into(),
            ));
        }
        if let Some(existing) =
            self.tool_development_event(tenant, namespace, id, &command.event_id)?
        {
            return if existing.command == command && existing.actor.subject_id == actor.subject_id {
                Ok(existing)
            } else {
                Err(Error::ConstraintViolation(
                    "Development event IDs are immutable".into(),
                ))
            };
        }
        if command.expected_revision != record.revision {
            return Err(Error::TransactionConflict(
                "Development revision changed".into(),
            ));
        }
        if record.state.terminal() {
            return Err(Error::ConstraintViolation(
                "Development is terminal; create a new request".into(),
            ));
        }
        let mut batch = WriteBatch::default();
        match &command.action {
            ToolDevelopmentAction::RequestCancellation { reason } => {
                validate_text(reason, "cancellation reason", 2048)?;
                record.state = ToolDevelopmentState::CancellationRequested;
                record.reason = "cancellation_requested".into();
            }
            ToolDevelopmentAction::Report { report } => {
                validate_text(&report.evidence_ref, "worker evidence", 2048)?;
                if let Some(detail) = &report.detail {
                    validate_text(detail, "worker detail", 8192)?;
                }
                if report.executor_profile_digest != executor.profile_digest {
                    return Err(Error::ConstraintViolation(
                        "Worker report profile mismatch".into(),
                    ));
                }
                if report.outcome != ToolReportOutcome::Succeeded && report.artifact.is_some() {
                    return Err(Error::ValidationError(
                        "Only successful reports may propose an artifact".into(),
                    ));
                }
                if let (Some(old), Some(new)) = (record.observed_cost_units, report.cost_units) {
                    if new < old {
                        return Err(Error::ValidationError(
                            "Cumulative cost cannot decrease".into(),
                        ));
                    }
                }
                if let (Some(old), Some(new)) = (record.observed_latency_ms, report.latency_ms) {
                    if new < old {
                        return Err(Error::ValidationError(
                            "Cumulative latency cannot decrease".into(),
                        ));
                    }
                }
                record.observed_cost_units = report.cost_units.or(record.observed_cost_units);
                record.observed_latency_ms = report.latency_ms.or(record.observed_latency_ms);
                record.cost_units = report.cost_units;
                record.latency_ms = report.latency_ms;
                match report.outcome {
                    ToolReportOutcome::Succeeded => {
                        let proposal = report.artifact.clone().ok_or_else(|| {
                            Error::ValidationError("Successful report requires an artifact".into())
                        })?;
                        if proposal.runtime_image_digest != executor.profile.runtime_image_digest
                            || proposal.parent_artifact_id
                                != record.receipt.request.parent_artifact_id
                            || proposal.repair_evidence_ref
                                != record.receipt.request.repair_evidence_ref
                            || !proposal.source_refs.contains(&format!("development:{id}"))
                        {
                            return Err(Error::ConstraintViolation(
                                "Artifact runtime or development provenance mismatch".into(),
                            ));
                        }
                        // Validate before accepting even a provisional success report.
                        let artifact =
                            self.prepare_tool_artifact(tenant, namespace, proposal, actor.clone())?;
                        if report
                            .cost_units
                            .is_some_and(|cost| cost > executor.profile.max_cost_units)
                            || report
                                .latency_ms
                                .is_some_and(|latency| latency > executor.profile.max_latency_ms)
                        {
                            record.state = ToolDevelopmentState::Failed;
                            record.reason = "resource_limit_exceeded".into();
                        } else if report.cost_units.is_none() || report.latency_ms.is_none() {
                            if record.state != ToolDevelopmentState::CancellationRequested {
                                record.state = ToolDevelopmentState::PendingOrUnknown;
                            }
                            record.reason = "unknown_consumption".into();
                        } else {
                            batch.put(
                                tool_key(8, tenant, namespace, &artifact.proposal.id)?,
                                encode(&artifact)?,
                            );
                            record.artifact_id = Some(artifact.proposal.id);
                            record.artifact_digest = Some(artifact.artifact_digest);
                            record.state = ToolDevelopmentState::Succeeded;
                            record.reason = "worker_reported_success".into();
                        }
                    }
                    ToolReportOutcome::Failed => {
                        record.state = ToolDevelopmentState::Failed;
                        record.reason = "worker_reported_failure".into();
                    }
                    ToolReportOutcome::Cancelled => {
                        if record.state != ToolDevelopmentState::CancellationRequested {
                            return Err(Error::ConstraintViolation(
                                "Cancellation has not been requested".into(),
                            ));
                        }
                        record.state = ToolDevelopmentState::Cancelled;
                        record.reason = "worker_confirmed_cancellation".into();
                    }
                    ToolReportOutcome::PendingOrUnknown => {
                        if record.state != ToolDevelopmentState::CancellationRequested {
                            record.state = ToolDevelopmentState::PendingOrUnknown;
                        }
                        record.reason = "worker_outcome_unknown".into();
                    }
                }
            }
        }
        record.revision = record
            .revision
            .checked_add(1)
            .ok_or_else(|| Error::ConstraintViolation("Development revision exhausted".into()))?;
        record.last_event_id = Some(command.event_id.clone());
        let event = ToolDevelopmentEvent {
            schema_version: 1,
            command,
            actor,
            record,
            recorded_at_millis: chrono::Utc::now().timestamp_millis(),
        };
        batch.put(key, encode(&event)?);
        batch.put(tool_key(10, tenant, namespace, id)?, encode(&event.record)?);
        self.inner
            .db
            .write_opt(batch, &write_options())
            .map_err(storage_error)?;
        Ok(event)
    }
}
