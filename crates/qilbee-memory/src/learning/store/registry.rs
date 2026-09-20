//! Immutable administrative policy and evaluation-context registry.
use super::*;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyAlgorithm {
    FixedBudgetHoeffdingV1,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyDefinition {
    pub algorithm: PolicyAlgorithm,
    pub parameters: LearningPolicy,
}

/// Exact version identities. Registry equality is not proof of experimental truth.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvaluationContext {
    pub task: String,
    pub baseline_revision: String,
    pub model_provider: String,
    pub model_revision: String,
    pub tools: BTreeMap<String, String>,
    pub environment_revision: String,
    pub evaluation_contract: String,
    pub dataset_revision: String,
    pub harness_revision: String,
    pub permissions_revision: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistryEntry<T> {
    pub schema_version: u32,
    pub tenant: String,
    pub id: String,
    pub payload: T,
    pub payload_digest: String,
    pub registered_by: String,
    pub recorded_at_millis: i64,
}

impl EvaluationContext {
    pub fn validate(&self) -> Result<()> {
        for (label, value) in [
            ("task", &self.task),
            ("baseline revision", &self.baseline_revision),
            ("model provider", &self.model_provider),
            ("model revision", &self.model_revision),
            ("environment revision", &self.environment_revision),
            ("evaluation contract", &self.evaluation_contract),
            ("dataset revision", &self.dataset_revision),
            ("harness revision", &self.harness_revision),
            ("permissions revision", &self.permissions_revision),
        ] {
            validate_text(value, label, 512)?;
        }
        if self.tools.len() > 128 {
            return Err(Error::ValidationError(
                "At most 128 tool identities are supported".into(),
            ));
        }
        for (name, revision) in &self.tools {
            validate_text(name, "tool name", 512)?;
            validate_text(revision, "tool revision", 512)?;
        }
        Ok(())
    }
}

pub(super) fn digest<T: Serialize>(value: &T) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(encode(value)?)))
}
fn registry_key(kind: u8, tenant: &str, id: &str) -> Vec<u8> {
    let mut key = vec![kind];
    append_component(&mut key, tenant);
    append_component(&mut key, id);
    key
}
impl LearningMemory {
    /// Trusted administrative interface. HTTP must authorize PolicyAdmin first.
    pub fn register_policy(
        &self,
        tenant: &str,
        id: &str,
        payload: PolicyDefinition,
        actor: &str,
    ) -> Result<RegistryEntry<PolicyDefinition>> {
        payload.parameters.validate()?;
        self.register(4, tenant, id, payload, actor)
    }
    pub fn register_context(
        &self,
        tenant: &str,
        id: &str,
        payload: EvaluationContext,
        actor: &str,
    ) -> Result<RegistryEntry<EvaluationContext>> {
        payload.validate()?;
        self.register(5, tenant, id, payload, actor)
    }
    pub fn policy(
        &self,
        tenant: &str,
        id: &str,
    ) -> Result<Option<RegistryEntry<PolicyDefinition>>> {
        self.registry_get(4, tenant, id)
    }
    pub fn context(
        &self,
        tenant: &str,
        id: &str,
    ) -> Result<Option<RegistryEntry<EvaluationContext>>> {
        self.registry_get(5, tenant, id)
    }

    fn register<T: Serialize + DeserializeOwned + PartialEq>(
        &self,
        kind: u8,
        tenant: &str,
        id: &str,
        payload: T,
        actor: &str,
    ) -> Result<RegistryEntry<T>> {
        validate_text(actor, "administrative actor", 512)?;
        let _guard = self
            .inner
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Learning mutation lock poisoned".into()))?;
        if let Some(existing) = self.registry_get::<T>(kind, tenant, id)? {
            return if existing.payload == payload {
                Ok(existing)
            } else {
                Err(Error::ConstraintViolation(
                    "Registry revisions are immutable; use a new ID".into(),
                ))
            };
        }
        let entry = RegistryEntry {
            schema_version: 1,
            tenant: tenant.into(),
            id: id.into(),
            payload_digest: digest(&payload)?,
            payload,
            registered_by: actor.into(),
            recorded_at_millis: chrono::Utc::now().timestamp_millis(),
        };
        self.inner
            .db
            .put_opt(
                registry_key(kind, tenant, id),
                encode(&entry)?,
                &write_options(),
            )
            .map_err(storage_error)?;
        Ok(entry)
    }
    fn registry_get<T: Serialize + DeserializeOwned>(
        &self,
        kind: u8,
        tenant: &str,
        id: &str,
    ) -> Result<Option<RegistryEntry<T>>> {
        validate_text(tenant, "tenant", 512)?;
        validate_text(id, "registry ID", 512)?;
        self.inner
            .db
            .get(registry_key(kind, tenant, id))
            .map_err(storage_error)?
            .map(|bytes| {
                let entry: RegistryEntry<T> = decode(&bytes)?;
                if entry.schema_version != 1
                    || entry.tenant != tenant
                    || entry.id != id
                    || entry.payload_digest != digest(&entry.payload)?
                {
                    return Err(Error::DataCorruption(
                        "Registry identity, version or digest mismatch".into(),
                    ));
                }
                Ok(entry)
            })
            .transpose()
    }
}

#[cfg(test)]
mod tests {
    use crate::learning::*;
    use std::{
        collections::BTreeMap,
        sync::{Arc, Barrier},
    };
    use tempfile::TempDir;

    fn policy() -> PolicyDefinition {
        PolicyDefinition {
            algorithm: PolicyAlgorithm::FixedBudgetHoeffdingV1,
            parameters: LearningPolicy {
                evaluator_id: "evaluator-subject".into(),
                evaluation_contract: "rubric-v1".into(),
                ..Default::default()
            },
        }
    }
    fn context() -> EvaluationContext {
        EvaluationContext {
            task: "task-v1".into(),
            baseline_revision: "baseline-v1".into(),
            model_provider: "provider".into(),
            model_revision: "model-v1".into(),
            tools: BTreeMap::from([("search".into(), "tool-digest-v1".into())]),
            environment_revision: "env-v1".into(),
            evaluation_contract: "rubric-v1".into(),
            dataset_revision: "held-out-v1".into(),
            harness_revision: "harness-v1".into(),
            permissions_revision: "permissions-v1".into(),
        }
    }
    #[test]
    fn registry_receipts_are_immutable_isolated_and_durable() {
        let dir = TempDir::new().unwrap();
        let db = LearningMemory::open(dir.path()).unwrap();
        let p = db
            .register_policy("tenant", "policy-v1", policy(), "admin")
            .unwrap();
        let c = db
            .register_context("tenant", "context-v1", context(), "admin")
            .unwrap();
        assert_eq!(
            p,
            db.register_policy("tenant", "policy-v1", policy(), "other-admin")
                .unwrap()
        );
        let mut changed = policy();
        changed.parameters.min_improvement = 0.2;
        assert!(
            db.register_policy("tenant", "policy-v1", changed, "admin")
                .is_err()
        );
        let mut changed = context();
        changed.model_revision = "model-v2".into();
        assert!(
            db.register_context("tenant", "context-v1", changed, "admin")
                .is_err()
        );
        assert!(db.policy("other-tenant", "policy-v1").unwrap().is_none());
        assert!(db.context("other-tenant", "context-v1").unwrap().is_none());
        assert_eq!(p.registered_by, "admin");
        drop(db);
        let db = LearningMemory::open(dir.path()).unwrap();
        assert_eq!(db.policy("tenant", "policy-v1").unwrap(), Some(p));
        assert_eq!(db.context("tenant", "context-v1").unwrap(), Some(c));
    }
    #[test]
    fn registry_validates_policy_context_and_strict_decoding() {
        let dir = TempDir::new().unwrap();
        let db = LearningMemory::open(dir.path()).unwrap();
        let mut p = policy();
        p.parameters.qualification_trials = 1;
        assert!(db.register_policy("tenant", "bad", p, "admin").is_err());
        let mut c = context();
        c.permissions_revision.clear();
        assert!(db.register_context("tenant", "bad", c, "admin").is_err());
        let mut value = serde_json::to_value(policy()).unwrap();
        value["algorithm"] = "qmn-sign-test".into();
        assert!(serde_json::from_value::<PolicyDefinition>(value).is_err());
        let mut value = serde_json::to_value(policy()).unwrap();
        value["parameters"]["unrecognized"] = true.into();
        assert!(serde_json::from_value::<PolicyDefinition>(value).is_err());
        let mut value = serde_json::to_value(context()).unwrap();
        value["unrecognized"] = true.into();
        assert!(serde_json::from_value::<EvaluationContext>(value).is_err());
    }
    #[test]
    fn registry_concurrent_writers_preserve_one_original_receipt() {
        let dir = TempDir::new().unwrap();
        let db = LearningMemory::open(dir.path()).unwrap();
        let barrier = Arc::new(Barrier::new(8));
        let workers: Vec<_> = (0..8)
            .map(|n| {
                let db = db.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    db.register_policy("tenant", "same", policy(), &format!("admin-{n}"))
                        .unwrap()
                })
            })
            .collect();
        let results: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
        assert!(results.iter().all(|r| r == &results[0]));
    }
}

#[cfg(test)]
mod integrity_tests {
    use super::*;
    #[test]
    fn registry_rejects_unsupported_schema_and_mismatched_digest() {
        let dir = tempfile::TempDir::new().unwrap();
        let db = LearningMemory::open(dir.path()).unwrap();
        let definition = PolicyDefinition {
            algorithm: PolicyAlgorithm::FixedBudgetHoeffdingV1,
            parameters: LearningPolicy {
                evaluator_id: "evaluator".into(),
                evaluation_contract: "contract".into(),
                ..Default::default()
            },
        };
        let entry = db
            .register_policy("tenant", "policy", definition, "admin")
            .unwrap();
        for corrupt_schema in [true, false] {
            let mut damaged = entry.clone();
            if corrupt_schema {
                damaged.schema_version = 99;
            } else {
                damaged.payload.parameters.min_improvement = 0.8;
            }
            db.inner
                .db
                .put(
                    registry_key(4, "tenant", "policy"),
                    encode(&damaged).unwrap(),
                )
                .unwrap();
            assert!(matches!(
                db.policy("tenant", "policy"),
                Err(Error::DataCorruption(_))
            ));
        }
    }
}
