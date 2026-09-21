//! Global administration and tenant data credentials use separate authorities.
use super::*;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};

mod accounts;
mod directory;
pub use accounts::{LoginAccountView, LoginAuthority, LoginSession};
pub use directory::{DirectoryPage, TenantDirectoryEntry};

const BOOTSTRAP_KEY: &str = "identity/v1/global-bootstrap";
const VERIFIER_DOMAIN: &[u8] = b"qilbeedb-global-credential-verifier-v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GlobalCapability {
    TenantCreate,
    TenantInspect,
    TenantAdmin,
    GlobalCredentialAdmin,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GlobalCredentialSpec {
    pub subject_id: String,
    pub capabilities: BTreeSet<GlobalCapability>,
    pub expires_at_millis: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GlobalCredentialView {
    pub id: Uuid,
    pub is_master: bool,
    pub spec: GlobalCredentialSpec,
    pub revision: u64,
    pub revoked_at_millis: Option<i64>,
    pub history: Vec<CredentialEvent>,
}

pub struct IssuedGlobalCredential {
    pub credential: GlobalCredentialView,
    pub secret: String,
}

impl std::fmt::Debug for IssuedGlobalCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IssuedGlobalCredential")
            .field("credential", &self.credential)
            .field("secret", &"[REDACTED]")
            .finish()
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredGlobalCredential {
    schema_version: u32,
    credential: GlobalCredentialView,
    verifier: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TenantView {
    pub schema_version: u32,
    pub tenant_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub created_at_millis: i64,
    pub created_by: Uuid,
    pub initial_admin_id: Uuid,
}

fn all_capabilities() -> BTreeSet<GlobalCapability> {
    [
        GlobalCapability::TenantCreate,
        GlobalCapability::TenantInspect,
        GlobalCapability::TenantAdmin,
        GlobalCapability::GlobalCredentialAdmin,
    ]
    .into()
}

fn validate_global_spec(spec: &GlobalCredentialSpec, now: Option<i64>) -> Result<()> {
    valid_id(&spec.subject_id)?;
    if spec.capabilities.is_empty()
        || now.is_some_and(|n| spec.expires_at_millis.is_some_and(|expires| expires <= n))
    {
        return Err(Error::ValidationError(
            "Explicit global capabilities and a future expiry, if set, are required".into(),
        ));
    }
    Ok(())
}

fn serialize<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    serde_json::to_vec(value).map_err(|e| Error::Serialization(e.to_string()))
}

fn global_key(id: Uuid) -> String {
    format!("identity/v1/global-credential/{id}")
}
fn tenant_key(tenant: &str) -> Result<String> {
    Ok(format!("identity/v1/tenant/{}", serialize_string(tenant)?))
}
fn registration_key(tenant: &str) -> Result<String> {
    Ok(format!(
        "identity/v1/tenant-registration/{}",
        serialize_string(tenant)?
    ))
}

fn global_secret(id: Uuid) -> Result<(String, Vec<u8>)> {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| Error::Internal("Operating system randomness unavailable".into()))?;
    let mut mac =
        Hmac::<sha2::Sha256>::new_from_slice(VERIFIER_DOMAIN).expect("HMAC accepts any key size");
    mac.update(&bytes);
    Ok((
        format!("qdbg1_{}_{}", id.simple(), URL_SAFE_NO_PAD.encode(bytes)),
        mac.finalize().into_bytes().to_vec(),
    ))
}

fn global_record(
    id: Uuid,
    spec: GlobalCredentialSpec,
    verifier: Vec<u8>,
    actor: Uuid,
    now: i64,
    is_master: bool,
) -> StoredGlobalCredential {
    StoredGlobalCredential {
        schema_version: 1,
        verifier,
        credential: GlobalCredentialView {
            id,
            is_master,
            spec,
            revision: 1,
            revoked_at_millis: None,
            history: vec![CredentialEvent {
                scope_authority_change: None,
                revision: 1,
                action: if is_master { "bootstrapped" } else { "issued" }.into(),
                actor_id: actor,
                at_millis: now,
            }],
        },
    }
}

impl IdentityStore {
    /// Exclusive local operator operation. Existing or revoked masters are never reset.
    pub fn bootstrap_master(&self, subject: &str) -> Result<IssuedGlobalCredential> {
        let spec = GlobalCredentialSpec {
            subject_id: subject.into(),
            capabilities: all_capabilities(),
            expires_at_millis: None,
        };
        validate_global_spec(&spec, None)?;
        let id = Uuid::new_v4();
        let (secret, verifier) = global_secret(id)?;
        let record = global_record(
            id,
            spec,
            verifier,
            id,
            chrono::Utc::now().timestamp_millis(),
            true,
        );
        let key = global_key(id);
        self.commit(
            vec![condition(BOOTSTRAP_KEY, None), condition(&key, None)],
            vec![
                write(BOOTSTRAP_KEY, serialize(&id)?),
                write(&key, serialize(&record)?),
            ],
        )?;
        Ok(IssuedGlobalCredential {
            credential: record.credential,
            secret,
        })
    }

    fn read_global(&self, id: Uuid) -> Result<(StoredGlobalCredential, Vec<u8>)> {
        let bytes = self.storage.get_meta(&global_key(id))?.ok_or_else(denied)?;
        let record: StoredGlobalCredential = serde_json::from_slice(&bytes)
            .map_err(|_| Error::DataCorruption("Invalid global authority".into()))?;
        if record.schema_version != 1
            || record.verifier.len() != 32
            || record.credential.id != id
            || record.credential.revision == 0
            || record.credential.history.len() as u64 != record.credential.revision
            || record
                .credential
                .history
                .iter()
                .enumerate()
                .any(|(i, e)| e.revision != i as u64 + 1)
            || validate_global_spec(&record.credential.spec, None).is_err()
            || (record.credential.is_master
                && record.credential.spec.capabilities != all_capabilities())
        {
            return Err(Error::DataCorruption(
                "Inconsistent global authority".into(),
            ));
        }
        if record.credential.is_master
            && self.storage.get_meta(BOOTSTRAP_KEY)?.as_deref() != Some(serialize(&id)?.as_slice())
        {
            return Err(Error::DataCorruption(
                "Inconsistent installation master".into(),
            ));
        }
        Ok((record, bytes))
    }

    fn authenticated_global(
        &self,
        token: &str,
        now: i64,
    ) -> Result<(StoredGlobalCredential, Vec<u8>)> {
        if token.starts_with("qdbsg1_") {
            return self.global_session_record(token, now);
        }
        if !token.is_ascii()
            || token.len() != 82
            || !token.starts_with("qdbg1_")
            || &token[38..39] != "_"
        {
            return Err(denied());
        }
        let id = Uuid::parse_str(&token[6..38]).map_err(|_| denied())?;
        if id.simple().to_string() != token[6..38] {
            return Err(denied());
        }
        let secret = URL_SAFE_NO_PAD.decode(&token[39..]).map_err(|_| denied())?;
        if secret.len() != 32 || URL_SAFE_NO_PAD.encode(&secret) != token[39..] {
            return Err(denied());
        }
        let (record, bytes) = self.read_global(id)?;
        let mut mac = Hmac::<sha2::Sha256>::new_from_slice(VERIFIER_DOMAIN)
            .expect("HMAC accepts any key size");
        mac.update(&secret);
        mac.verify_slice(&record.verifier).map_err(|_| denied())?;
        if record.credential.revoked_at_millis.is_some()
            || record
                .credential
                .spec
                .expires_at_millis
                .is_some_and(|expires| now >= expires)
        {
            return Err(denied());
        }
        Ok((record, bytes))
    }

    pub fn authenticate_global(&self, token: &str) -> Result<GlobalCredentialView> {
        Ok(self
            .authenticated_global(token, chrono::Utc::now().timestamp_millis())?
            .0
            .credential)
    }

    fn global_authority(
        &self,
        token: &str,
        capability: GlobalCapability,
    ) -> Result<(StoredGlobalCredential, Vec<u8>)> {
        let pair = self.authenticated_global(token, chrono::Utc::now().timestamp_millis())?;
        if !pair.0.credential.spec.capabilities.contains(&capability) {
            return Err(denied());
        }
        Ok(pair)
    }

    /// Delegation is an explicit subset; tenant credentials cannot enter this authority.
    pub fn issue_global(
        &self,
        token: &str,
        spec: GlobalCredentialSpec,
    ) -> Result<IssuedGlobalCredential> {
        let (actor, previous) =
            self.global_authority(token, GlobalCapability::GlobalCredentialAdmin)?;
        let now = chrono::Utc::now().timestamp_millis();
        validate_global_spec(&spec, Some(now))?;
        if !spec
            .capabilities
            .is_subset(&actor.credential.spec.capabilities)
            || actor
                .credential
                .spec
                .expires_at_millis
                .is_some_and(|expires| spec.expires_at_millis.is_none_or(|child| child > expires))
        {
            return Err(denied());
        }
        let id = Uuid::new_v4();
        let (secret, verifier) = global_secret(id)?;
        let record = global_record(id, spec, verifier, actor.credential.id, now, false);
        let key = global_key(id);
        self.commit_authenticated(
            token,
            vec![
                condition(&global_key(actor.credential.id), Some(previous)),
                condition(&key, None),
            ],
            vec![write(&key, serialize(&record)?)],
        )?;
        Ok(IssuedGlobalCredential {
            credential: record.credential,
            secret,
        })
    }

    pub fn inspect_global(&self, token: &str, id: Uuid) -> Result<GlobalCredentialView> {
        let (actor, _) = self.global_authority(token, GlobalCapability::GlobalCredentialAdmin)?;
        let (record, _) = self.read_global(id)?;
        if !record
            .credential
            .spec
            .capabilities
            .is_subset(&actor.credential.spec.capabilities)
        {
            return Err(denied());
        }
        Ok(record.credential)
    }

    /// Tenant existence, registration receipt, first credential and actor revision are atomic.
    pub fn register_tenant(
        &self,
        token: &str,
        tenant: &str,
        subject: &str,
    ) -> Result<(TenantView, IssuedCredential)> {
        self.register_tenant_named(token, tenant, subject, None)
    }

    pub fn register_tenant_named(
        &self,
        token: &str,
        tenant: &str,
        subject: &str,
        display_name: Option<&str>,
    ) -> Result<(TenantView, IssuedCredential)> {
        if let Some(name) = display_name {
            valid_id(name)?;
        }
        let (actor, actor_bytes) = self.global_authority(token, GlobalCapability::TenantCreate)?;
        valid_id(tenant)?;
        let now = chrono::Utc::now().timestamp_millis();
        let spec = tenant_admin_spec(subject);
        validate_spec(&spec, Some(now))?;
        let id = Uuid::new_v4();
        let (secret, verifier) = generate_secret(id)?;
        let credential = new_record(
            id,
            tenant,
            spec,
            verifier,
            actor.credential.id,
            now,
            "tenant_registered_by_global_authority",
        );
        let registration = TenantView {
            schema_version: 1,
            tenant_id: tenant.into(),
            display_name: display_name.map(str::to_owned),
            created_at_millis: now,
            created_by: actor.credential.id,
            initial_admin_id: id,
        };
        let tenant_key = tenant_key(tenant)?;
        let registration_key = registration_key(tenant)?;
        let key = credential_key(id);
        self.commit_authenticated(
            token,
            vec![
                condition(&global_key(actor.credential.id), Some(actor_bytes)),
                condition(&tenant_key, None),
                condition(&registration_key, None),
                condition(&key, None),
            ],
            vec![
                write(&tenant_key, tenant.as_bytes().to_vec()),
                write(&registration_key, serialize(&registration)?),
                write(&key, encode(&credential)?),
            ],
        )?;
        Ok((
            registration,
            IssuedCredential {
                credential: credential.credential,
                secret,
            },
        ))
    }

    /// Legacy locally bootstrapped tenants have no fabricated global registration receipt.
    pub fn inspect_tenant(&self, token: &str, tenant: &str) -> Result<Option<TenantView>> {
        self.global_authority(token, GlobalCapability::TenantInspect)?;
        valid_id(tenant)?;
        let Some(marker) = self.storage.get_meta(&tenant_key(tenant)?)? else {
            return Err(Error::KeyNotFound("Tenant does not exist".into()));
        };
        if marker != tenant.as_bytes() {
            return Err(Error::DataCorruption(
                "Inconsistent tenant authority".into(),
            ));
        }
        self.storage
            .get_meta(&registration_key(tenant)?)?
            .map(|bytes| {
                let record: TenantView = serde_json::from_slice(&bytes)
                    .map_err(|_| Error::DataCorruption("Invalid tenant registration".into()))?;
                if record.schema_version != 1 || record.tenant_id != tenant {
                    return Err(Error::DataCorruption(
                        "Inconsistent tenant registration".into(),
                    ));
                }
                Ok(record)
            })
            .transpose()
    }

    /// Global tenant administration appoints an auditable tenant administrator, never an implicit data grant.
    pub fn issue_tenant_admin(
        &self,
        token: &str,
        tenant: &str,
        subject: &str,
    ) -> Result<IssuedCredential> {
        let (actor, actor_bytes) = self.global_authority(token, GlobalCapability::TenantAdmin)?;
        valid_id(tenant)?;
        let tenant_key = tenant_key(tenant)?;
        let marker = self
            .storage
            .get_meta(&tenant_key)?
            .ok_or_else(|| Error::KeyNotFound("Tenant does not exist".into()))?;
        if marker != tenant.as_bytes() {
            return Err(Error::DataCorruption(
                "Inconsistent tenant authority".into(),
            ));
        }
        let now = chrono::Utc::now().timestamp_millis();
        let spec = tenant_admin_spec(subject);
        validate_spec(&spec, Some(now))?;
        let id = Uuid::new_v4();
        let (secret, verifier) = generate_secret(id)?;
        let credential = new_record(
            id,
            tenant,
            spec,
            verifier,
            actor.credential.id,
            now,
            "admin_issued_by_global_authority",
        );
        let key = credential_key(id);
        self.commit_authenticated(
            token,
            vec![
                condition(&global_key(actor.credential.id), Some(actor_bytes)),
                condition(&tenant_key, Some(marker)),
                condition(&key, None),
            ],
            vec![write(&key, encode(&credential)?)],
        )?;
        Ok(IssuedCredential {
            credential: credential.credential,
            secret,
        })
    }

    pub fn rotate_global(
        &self,
        token: &str,
        id: Uuid,
        revision: u64,
    ) -> Result<IssuedGlobalCredential> {
        let (credential, secret) = self.change_global(token, id, revision, false)?;
        Ok(IssuedGlobalCredential {
            credential,
            secret: secret.expect("rotation creates a secret"),
        })
    }

    pub fn revoke_global(
        &self,
        token: &str,
        id: Uuid,
        revision: u64,
    ) -> Result<GlobalCredentialView> {
        Ok(self.change_global(token, id, revision, true)?.0)
    }

    fn change_global(
        &self,
        token: &str,
        id: Uuid,
        revision: u64,
        revoke: bool,
    ) -> Result<(GlobalCredentialView, Option<String>)> {
        let now = chrono::Utc::now().timestamp_millis();
        let (actor, actor_bytes) = self.authenticated_global(token, now)?;
        let (mut target, previous) = self.read_global(id)?;
        if actor.credential.id != id
            && (target.credential.is_master
                || !actor
                    .credential
                    .spec
                    .capabilities
                    .contains(&GlobalCapability::GlobalCredentialAdmin)
                || !target
                    .credential
                    .spec
                    .capabilities
                    .is_subset(&actor.credential.spec.capabilities))
        {
            return Err(denied());
        }
        if target.credential.revision != revision {
            return Err(conflict());
        }
        if target.credential.revoked_at_millis.is_some() {
            return Err(denied());
        }
        let next = revision.checked_add(1).ok_or_else(conflict)?;
        let secret = if revoke {
            target.credential.revoked_at_millis = Some(now);
            None
        } else {
            let (secret, verifier) = global_secret(id)?;
            target.verifier = verifier;
            Some(secret)
        };
        target.credential.revision = next;
        target.credential.history.push(CredentialEvent {
            scope_authority_change: None,
            revision: next,
            action: if revoke { "revoked" } else { "rotated" }.into(),
            actor_id: actor.credential.id,
            at_millis: now,
        });
        let key = global_key(id);
        let mut conditions = vec![condition(&key, Some(previous))];
        if actor.credential.id != id {
            conditions.push(condition(
                &global_key(actor.credential.id),
                Some(actor_bytes),
            ));
        } else if conditions[0].expected.as_deref() != Some(actor_bytes.as_slice()) {
            return Err(conflict());
        }
        self.commit_authenticated(token, conditions, vec![write(&key, serialize(&target)?)])?;
        Ok((target.credential, secret))
    }

    /// Offline recovery requires exclusive storage access and an explicit revision.
    pub fn recover_master(&self, expected_revision: u64) -> Result<IssuedGlobalCredential> {
        let bootstrap = self.storage.get_meta(BOOTSTRAP_KEY)?.ok_or_else(denied)?;
        let id: Uuid = serde_json::from_slice(&bootstrap)
            .map_err(|_| Error::DataCorruption("Invalid bootstrap marker".into()))?;
        let (mut record, previous) = self.read_global(id)?;
        if !record.credential.is_master {
            return Err(Error::DataCorruption(
                "Bootstrap authority is not a master".into(),
            ));
        }
        if record.credential.revision != expected_revision {
            return Err(conflict());
        }
        let next = expected_revision.checked_add(1).ok_or_else(conflict)?;
        let (secret, verifier) = global_secret(id)?;
        record.verifier = verifier;
        record.credential.revision = next;
        record.credential.revoked_at_millis = None;
        record.credential.history.push(CredentialEvent {
            scope_authority_change: None,
            revision: next,
            action: "recovered_by_local_operator".into(),
            actor_id: id,
            at_millis: chrono::Utc::now().timestamp_millis(),
        });
        let key = global_key(id);
        self.commit(
            vec![
                condition(BOOTSTRAP_KEY, Some(bootstrap)),
                condition(&key, Some(previous)),
            ],
            vec![write(&key, serialize(&record)?)],
        )?;
        Ok(IssuedGlobalCredential {
            credential: record.credential,
            secret,
        })
    }
}

fn tenant_admin_spec(subject: &str) -> CredentialSpec {
    CredentialSpec {
        scope_policy: None,
        subject_id: subject.into(),
        capabilities: [Capability::CredentialAdmin, Capability::PolicyAdmin].into(),
        grants: vec![],
        expires_at_millis: None,
    }
}

#[cfg(test)]
mod tests;
