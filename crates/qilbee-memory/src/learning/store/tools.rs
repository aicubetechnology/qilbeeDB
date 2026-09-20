//! Durable declarations for learned tools. This module never executes source.
use super::*;
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolActor {
    pub subject_id: String,
    pub credential_id: String,
}
impl ToolActor {
    pub(super) fn validate(&self) -> Result<()> {
        validate_text(&self.subject_id, "tool subject", 512)?;
        validate_text(&self.credential_id, "tool credential", 512)
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolArtifactProposal {
    pub id: String,
    pub source: String,
    pub dependency_lock: String,
    pub runtime_image_digest: String,
    pub entrypoint: String,
    pub input_schema: Value,
    pub output_schema: Value,
    pub source_refs: Vec<String>,
    pub parent_artifact_id: Option<String>,
    pub repair_evidence_ref: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolArtifact {
    pub schema_version: u32,
    pub tenant: String,
    pub namespace: String,
    pub proposal: ToolArtifactProposal,
    pub artifact_digest: String,
    pub source_digest: String,
    pub dependency_digest: String,
    pub actor: ToolActor,
    pub recorded_at_millis: i64,
}
impl ToolArtifactProposal {
    fn validate(&self) -> Result<()> {
        validate_text(&self.id, "artifact ID", 512)?;
        validate_text(&self.source, "artifact source", 24 * 1024)?;
        validate_text(&self.entrypoint, "entrypoint", 512)?;
        image_digest(&self.runtime_image_digest)?;
        if self.dependency_lock.len() > 8192 {
            return Err(Error::ValidationError(
                "Dependency lock exceeds 8192 bytes".into(),
            ));
        }
        for schema in [&self.input_schema, &self.output_schema] {
            if !(schema.is_object() || schema.is_boolean()) || encode(schema)?.len() > 8192 {
                return Err(Error::ValidationError(
                    "Schema declaration must be an object or boolean, at most 8192 bytes".into(),
                ));
            }
        }
        if self.source_refs.is_empty() || self.source_refs.len() > 32 {
            return Err(Error::ValidationError(
                "Expected 1 to 32 source references".into(),
            ));
        }
        for reference in &self.source_refs {
            validate_text(reference, "source reference", 2048)?;
        }
        match (&self.parent_artifact_id, &self.repair_evidence_ref) {
            (None, None) => (),
            (Some(parent), Some(evidence)) if parent != &self.id => {
                validate_text(parent, "parent artifact", 512)?;
                validate_text(evidence, "repair evidence", 2048)?;
            }
            _ => {
                return Err(Error::ValidationError(
                    "Repair requires a different parent artifact and an evidence reference".into(),
                ));
            }
        }
        Ok(())
    }
}
pub(super) fn image_digest(value: &str) -> Result<()> {
    if value.len() == 71
        && value.starts_with("sha256:")
        && value[7..]
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        Ok(())
    } else {
        Err(Error::ValidationError(
            "Expected sha256 followed by 64 lowercase hexadecimal digits".into(),
        ))
    }
}
pub(super) fn raw_digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
pub(super) fn tool_key(kind: u8, tenant: &str, namespace: &str, id: &str) -> Result<Vec<u8>> {
    validate_text(tenant, "tenant", 512)?;
    validate_text(namespace, "tool namespace", 4096)?;
    validate_text(id, "tool record ID", 512)?;
    let mut key = vec![kind];
    for component in [tenant, namespace, id] {
        append_component(&mut key, component);
    }
    Ok(key)
}
impl LearningMemory {
    /// Trusted library interface; network callers require scoped tool authority.
    pub fn register_tool_artifact(
        &self,
        tenant: &str,
        namespace: &str,
        proposal: ToolArtifactProposal,
        actor: ToolActor,
    ) -> Result<ToolArtifact> {
        let _guard = self
            .inner
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Learning mutation lock poisoned".into()))?;
        let artifact = self.prepare_tool_artifact(tenant, namespace, proposal, actor)?;
        self.inner
            .db
            .put_opt(
                tool_key(8, tenant, namespace, &artifact.proposal.id)?,
                encode(&artifact)?,
                &write_options(),
            )
            .map_err(storage_error)?;
        Ok(artifact)
    }
    pub(super) fn prepare_tool_artifact(
        &self,
        tenant: &str,
        namespace: &str,
        proposal: ToolArtifactProposal,
        actor: ToolActor,
    ) -> Result<ToolArtifact> {
        proposal.validate()?;
        actor.validate()?;
        if let Some(existing) = self.tool_artifact(tenant, namespace, &proposal.id)? {
            return if existing.proposal == proposal {
                Ok(existing)
            } else {
                Err(Error::ConstraintViolation(
                    "Artifact revisions are immutable".into(),
                ))
            };
        }
        if let Some(parent) = &proposal.parent_artifact_id {
            self.tool_artifact(tenant, namespace, parent)?
                .ok_or_else(|| Error::KeyNotFound("Parent artifact".into()))?;
        }
        Ok(ToolArtifact {
            schema_version: 1,
            tenant: tenant.into(),
            namespace: namespace.into(),
            artifact_digest: registry::digest(&proposal)?,
            source_digest: raw_digest(&proposal.source),
            dependency_digest: raw_digest(&proposal.dependency_lock),
            proposal,
            actor,
            recorded_at_millis: chrono::Utc::now().timestamp_millis(),
        })
    }
    pub fn tool_artifact(
        &self,
        tenant: &str,
        namespace: &str,
        id: &str,
    ) -> Result<Option<ToolArtifact>> {
        self.inner
            .db
            .get(tool_key(8, tenant, namespace, id)?)
            .map_err(storage_error)?
            .map(|bytes| {
                let record: ToolArtifact = decode(&bytes)?;
                if record.schema_version != 1
                    || record.tenant != tenant
                    || record.namespace != namespace
                    || record.proposal.id != id
                    || record.artifact_digest != registry::digest(&record.proposal)?
                    || record.source_digest != raw_digest(&record.proposal.source)
                    || record.dependency_digest != raw_digest(&record.proposal.dependency_lock)
                {
                    return Err(Error::DataCorruption(
                        "Artifact identity or digest mismatch".into(),
                    ));
                }
                Ok(record)
            })
            .transpose()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn artifact_reader_detects_digest_corruption() {
        let dir = tempfile::TempDir::new().unwrap();
        let store = LearningMemory::open(dir.path()).unwrap();
        let proposal = ToolArtifactProposal {
            id: "one".into(),
            source: "pass".into(),
            dependency_lock: String::new(),
            runtime_image_digest: format!("sha256:{}", "a".repeat(64)),
            entrypoint: "run".into(),
            input_schema: Value::Bool(true),
            output_schema: Value::Bool(true),
            source_refs: vec!["request".into()],
            parent_artifact_id: None,
            repair_evidence_ref: None,
        };
        let mut record = store
            .register_tool_artifact(
                "tenant",
                "scope",
                proposal,
                ToolActor {
                    subject_id: "worker".into(),
                    credential_id: "key".into(),
                },
            )
            .unwrap();
        record.proposal.source = "tampered".into();
        store
            .inner
            .db
            .put(
                tool_key(8, "tenant", "scope", "one").unwrap(),
                encode(&record).unwrap(),
            )
            .unwrap();
        assert!(matches!(
            store.tool_artifact("tenant", "scope", "one"),
            Err(Error::DataCorruption(_))
        ));
    }
}
