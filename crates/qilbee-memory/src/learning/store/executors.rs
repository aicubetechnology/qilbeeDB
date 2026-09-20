//! Administrator-owned identities for external development executors.
use super::{
    tools::{ToolActor, image_digest, tool_key},
    *,
};
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolExecutorProfile {
    pub id: String,
    pub subject_id: String,
    pub runtime_image_digest: String,
    pub environment_revision: String,
    pub permissions_revision: String,
    pub max_cost_units: u64,
    pub max_latency_ms: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolExecutor {
    pub schema_version: u32,
    pub tenant: String,
    pub profile: ToolExecutorProfile,
    pub profile_digest: String,
    pub actor: ToolActor,
    pub recorded_at_millis: i64,
}
impl ToolExecutorProfile {
    fn validate(&self) -> Result<()> {
        for (label, value) in [
            ("executor ID", &self.id),
            ("executor subject", &self.subject_id),
            ("environment revision", &self.environment_revision),
            ("permissions revision", &self.permissions_revision),
        ] {
            validate_text(value, label, 512)?;
        }
        image_digest(&self.runtime_image_digest)?;
        if self.max_cost_units == 0 || self.max_latency_ms == 0 {
            return Err(Error::ValidationError(
                "Executor budgets must be positive".into(),
            ));
        }
        Ok(())
    }
}
impl LearningMemory {
    /// Trusted administrator interface; a profile declares authority, not isolation proof.
    pub fn register_tool_executor(
        &self,
        tenant: &str,
        profile: ToolExecutorProfile,
        actor: ToolActor,
    ) -> Result<ToolExecutor> {
        profile.validate()?;
        actor.validate()?;
        let _guard = self
            .inner
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Learning mutation lock poisoned".into()))?;
        if let Some(existing) = self.tool_executor(tenant, &profile.id)? {
            return if existing.profile == profile {
                Ok(existing)
            } else {
                Err(Error::ConstraintViolation(
                    "Executor profiles are immutable".into(),
                ))
            };
        }
        let record = ToolExecutor {
            schema_version: 1,
            tenant: tenant.into(),
            profile_digest: registry::digest(&profile)?,
            profile,
            actor,
            recorded_at_millis: chrono::Utc::now().timestamp_millis(),
        };
        self.inner
            .db
            .put_opt(
                tool_key(9, tenant, "executors", &record.profile.id)?,
                encode(&record)?,
                &write_options(),
            )
            .map_err(storage_error)?;
        Ok(record)
    }
    pub fn tool_executor(&self, tenant: &str, id: &str) -> Result<Option<ToolExecutor>> {
        self.inner
            .db
            .get(tool_key(9, tenant, "executors", id)?)
            .map_err(storage_error)?
            .map(|bytes| {
                let record: ToolExecutor = decode(&bytes)?;
                if record.schema_version != 1
                    || record.tenant != tenant
                    || record.profile.id != id
                    || record.profile_digest != registry::digest(&record.profile)?
                {
                    return Err(Error::DataCorruption(
                        "Executor identity or digest mismatch".into(),
                    ));
                }
                Ok(record)
            })
            .transpose()
    }
}
