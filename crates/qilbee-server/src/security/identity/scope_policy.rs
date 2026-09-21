//! Explicit company integration authority over externally managed identifiers.
use super::*;

const MAX_SELECTOR_IDS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompanyScopePolicyVersion {
    CompanyScopesV1,
}

/// `All` is deliberate authority, never inferred from an absent/empty list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum IdSelector {
    All {},
    Only { ids: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyScopePolicy {
    pub version: CompanyScopePolicyVersion,
    pub projects: IdSelector,
    pub agents: IdSelector,
    /// Selects assigned mission IDs. Missionless scopes have a separate switch.
    pub missions: IdSelector,
    pub allow_unassigned_mission: bool,
    pub visibilities: Vec<Visibility>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeAuthority {
    pub grants: Vec<ResourceScope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_policy: Option<CompanyScopePolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeAuthorityChange {
    pub previous: ScopeAuthority,
    pub current: ScopeAuthority,
}

impl From<&CredentialSpec> for ScopeAuthority {
    fn from(spec: &CredentialSpec) -> Self {
        Self {
            grants: spec.grants.clone(),
            scope_policy: spec.scope_policy.clone(),
        }
    }
}

impl IdSelector {
    fn allows(&self, value: &str) -> bool {
        match self {
            Self::All {} => true,
            Self::Only { ids } => ids.iter().any(|id| id == value),
        }
    }

    fn validate(&self) -> Result<()> {
        if let Self::Only { ids } = self {
            if ids.len() > MAX_SELECTOR_IDS
                || ids.iter().collect::<BTreeSet<_>>().len() != ids.len()
            {
                return Err(Error::ValidationError(
                    "Scope selectors accept at most 256 distinct identifiers".into(),
                ));
            }
            for id in ids {
                valid_id(id)?;
            }
        }
        Ok(())
    }
}

pub(super) fn validate(spec: &CredentialSpec) -> Result<()> {
    if let Some(policy) = &spec.scope_policy {
        if !spec.grants.is_empty() {
            return Err(Error::ValidationError(
                "Use either exact grants or a company scope policy, not both".into(),
            ));
        }
        // A constrained integration must not escape its limits by issuing keys.
        if spec.capabilities.contains(&Capability::CredentialAdmin) {
            return Err(Error::ValidationError(
                "Company integrations cannot hold credential_admin; use a separate administrator"
                    .into(),
            ));
        }
        policy.projects.validate()?;
        policy.agents.validate()?;
        policy.missions.validate()?;
        if policy.visibilities.is_empty()
            || policy.visibilities.len() > 2
            || (policy.visibilities.len() == 2 && policy.visibilities[0] == policy.visibilities[1])
        {
            return Err(Error::ValidationError(
                "Select one or two distinct scope visibilities".into(),
            ));
        }
    }
    Ok(())
}

pub(super) fn allows(spec: &CredentialSpec, scope: &ResourceScope) -> bool {
    match &spec.scope_policy {
        None => spec.grants.contains(scope),
        Some(policy) => {
            policy.projects.allows(&scope.project_id)
                && policy.agents.allows(&scope.agent_id)
                && policy.visibilities.contains(&scope.visibility)
                && match &scope.mission_id {
                    None => policy.allow_unassigned_mission,
                    Some(mission) => policy.missions.allows(mission),
                }
        }
    }
}

pub(super) fn validate_history(credential: &CredentialView) -> Result<()> {
    let invalid = || Error::DataCorruption("Inconsistent scope authority history".into());
    let mut current: Option<&ScopeAuthority> = None;
    for event in &credential.history {
        match (event.action.as_str(), &event.scope_authority_change) {
            ("scope_authority_changed", Some(change)) => {
                for authority in [&change.previous, &change.current] {
                    let mut spec = credential.spec.clone();
                    spec.grants = authority.grants.clone();
                    spec.scope_policy = authority.scope_policy.clone();
                    validate_spec(&spec, None).map_err(|_| invalid())?;
                }
                if current.is_some_and(|previous| previous != &change.previous) {
                    return Err(invalid());
                }
                current = Some(&change.current);
            }
            ("scope_authority_changed", None) | (_, Some(_)) => return Err(invalid()),
            _ => {}
        }
    }
    if current.is_some_and(|current| current != &ScopeAuthority::from(&credential.spec)) {
        return Err(invalid());
    }
    Ok(())
}

impl IdentityStore {
    /// Replace only scope authority, preserving the key, tenant, subject,
    /// capabilities and expiry. The current administrator and target revisions
    /// guard the same durable batch as the full before/after audit event.
    pub fn set_scope_authority(
        &self,
        admin: &str,
        id: Uuid,
        revision: u64,
        authority: ScopeAuthority,
    ) -> Result<CredentialView> {
        let now = chrono::Utc::now().timestamp_millis();
        let (actor, actor_bytes) = self.admin(admin, now)?;
        let (mut record, previous_bytes) = self.read_record(id)?;
        if record.credential.tenant_id != actor.credential.tenant_id
            || record.credential.revoked_at_millis.is_some()
        {
            return Err(denied());
        }
        if record.credential.revision != revision {
            return Err(conflict());
        }
        let previous = ScopeAuthority::from(&record.credential.spec);
        record.credential.spec.grants = authority.grants.clone();
        record.credential.spec.scope_policy = authority.scope_policy.clone();
        validate_spec(&record.credential.spec, Some(now))?;
        let next = revision.checked_add(1).ok_or_else(conflict)?;
        record.credential.revision = next;
        record.credential.history.push(CredentialEvent {
            revision: next,
            action: "scope_authority_changed".into(),
            actor_id: actor.credential.id,
            at_millis: now,
            scope_authority_change: Some(ScopeAuthorityChange {
                previous,
                current: authority,
            }),
        });
        let key = credential_key(id);
        let mut expected = vec![condition(&key, Some(previous_bytes))];
        if actor.credential.id != id {
            expected.push(condition(
                &credential_key(actor.credential.id),
                Some(actor_bytes),
            ));
        } else if expected[0].expected.as_deref() != Some(actor_bytes.as_slice()) {
            return Err(conflict());
        }
        self.commit_authenticated(admin, expected, vec![write(&key, encode(&record)?)])?;
        Ok(record.credential)
    }
}

#[cfg(test)]
mod tests;
