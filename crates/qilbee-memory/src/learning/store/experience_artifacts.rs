//! Verified stored-artifact identities attached to an authenticated observation.
use super::{experience::*, tools::tool_key, *};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExperienceArtifactRole {
    Baseline,
    Candidate,
    Output,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperienceArtifactRequest {
    pub id: String,
    pub event_id: String,
    pub artifact_id: String,
    pub artifact_digest: String,
    pub role: ExperienceArtifactRole,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperienceArtifactBinding {
    pub schema_version: u32,
    pub request: ExperienceArtifactRequest,
    pub attempt_id: String,
    pub receipt_digest: String,
    pub event_digest: String,
    pub source_digest: String,
    pub dependency_digest: String,
    pub actor: ExperienceActor,
    pub recorded_at_millis: i64,
    pub binding_digest: String,
}
impl ExperienceArtifactBinding {
    fn digest(&self) -> Result<String> {
        registry::digest(&(
            self.schema_version,
            &self.request,
            &self.attempt_id,
            &self.receipt_digest,
            &self.event_digest,
            &self.source_digest,
            &self.dependency_digest,
            &self.actor,
            self.recorded_at_millis,
        ))
    }
}
fn binding_key(tenant: &str, namespace: &str, attempt: &str, id: &str) -> Result<Vec<u8>> {
    let mut key = tool_key(14, tenant, namespace, attempt)?;
    validate_text(id, "artifact binding ID", 512)?;
    append_component(&mut key, id);
    Ok(key)
}
impl LearningMemory {
    /// Verify stored bytes, not external execution or the truth of the declared role.
    pub fn bind_experience_artifact(
        &self,
        tenant: &str,
        namespace: &str,
        attempt: &str,
        request: ExperienceArtifactRequest,
        actor: ExperienceActor,
    ) -> Result<ExperienceArtifactBinding> {
        let key = binding_key(tenant, namespace, attempt, &request.id)?;
        validate_text(&request.event_id, "event ID", 512)?;
        validate_text(&request.artifact_id, "artifact ID", 512)?;
        experience::validate_digest(&request.artifact_digest)?;
        actor.validate()?;
        let _guard = self
            .inner
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Learning mutation lock poisoned".into()))?;
        let event = self
            .experience_event(tenant, namespace, attempt, &request.event_id)?
            .ok_or_else(|| Error::KeyNotFound("Experience event".into()))?;
        if actor.subject_id != event.record.receipt.request.reporter_subject_id {
            return Err(Error::Unauthorized(
                "Only the bound reporter can bind artifacts".into(),
            ));
        }
        if let Some(existing) =
            self.experience_artifact_binding(tenant, namespace, attempt, &request.id)?
        {
            return if existing.request == request && existing.actor.subject_id == actor.subject_id {
                Ok(existing)
            } else {
                Err(Error::ConstraintViolation(
                    "Artifact binding IDs are immutable".into(),
                ))
            };
        }
        let artifact = self
            .tool_artifact(tenant, namespace, &request.artifact_id)?
            .ok_or_else(|| Error::KeyNotFound("Tool artifact".into()))?;
        if artifact.artifact_digest != request.artifact_digest {
            return Err(Error::ConstraintViolation(
                "Artifact digest differs from stored content".into(),
            ));
        }
        let mut binding = ExperienceArtifactBinding {
            schema_version: 1,
            request,
            attempt_id: attempt.into(),
            receipt_digest: event.record.receipt.receipt_digest,
            event_digest: event.event_digest,
            source_digest: artifact.source_digest,
            dependency_digest: artifact.dependency_digest,
            actor,
            recorded_at_millis: chrono::Utc::now().timestamp_millis(),
            binding_digest: String::new(),
        };
        binding.binding_digest = binding.digest()?;
        self.inner
            .db
            .put_opt(key, encode(&binding)?, &write_options())
            .map_err(storage_error)?;
        Ok(binding)
    }
    pub fn experience_artifact_binding(
        &self,
        tenant: &str,
        namespace: &str,
        attempt: &str,
        id: &str,
    ) -> Result<Option<ExperienceArtifactBinding>> {
        self.inner
            .db
            .get(binding_key(tenant, namespace, attempt, id)?)
            .map_err(storage_error)?
            .map(|bytes| {
                let binding: ExperienceArtifactBinding = decode(&bytes)?;
                let event = self
                    .experience_event(tenant, namespace, attempt, &binding.request.event_id)?
                    .ok_or_else(|| {
                        Error::DataCorruption("Bound experience event missing".into())
                    })?;
                let artifact = self
                    .tool_artifact(tenant, namespace, &binding.request.artifact_id)?
                    .ok_or_else(|| Error::DataCorruption("Bound artifact missing".into()))?;
                binding
                    .actor
                    .validate()
                    .map_err(|_| Error::DataCorruption("Invalid binding actor".into()))?;
                if binding.schema_version != 1
                    || binding.attempt_id != attempt
                    || binding.request.id != id
                    || binding.receipt_digest != event.record.receipt.receipt_digest
                    || binding.event_digest != event.event_digest
                    || binding.actor.subject_id != event.record.receipt.request.reporter_subject_id
                    || binding.request.artifact_digest != artifact.artifact_digest
                    || binding.source_digest != artifact.source_digest
                    || binding.dependency_digest != artifact.dependency_digest
                    || binding.binding_digest != binding.digest()?
                {
                    return Err(Error::DataCorruption(
                        "Artifact binding identity or digest mismatch".into(),
                    ));
                }
                Ok(binding)
            })
            .transpose()
    }
}
