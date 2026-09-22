//! Durable tenant credentials, exact grants and explicit company scope policies.

use qilbee_core::{Error, Result};
use qilbee_storage::StorageEngine;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, sync::Arc};
use uuid::Uuid;

mod master;
mod scope_policy;
pub use master::{
    DirectoryPage, GlobalCapability, GlobalCredentialSpec, GlobalCredentialView,
    IssuedGlobalCredential, LoginAccountView, LoginAuthority, LoginSession, TenantDirectoryEntry,
    TenantView,
};
pub use scope_policy::{
    CompanyScopePolicy, CompanyScopePolicyVersion, IdSelector, ScopeAuthority, ScopeAuthorityChange,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    MemoryRead,
    MemoryWrite,
    MemoryReview,
    MemoryCheckpoint,
    ProcedurePropose,
    ProcedureEvaluate,
    ExperienceRead,
    ExperienceWrite,
    ExperienceReport,
    ToolRead,
    ToolDevelop,
    ToolReport,
    ToolAdmin,
    /// Read company-wide policy/context definitions; memory grants do not narrow this authority.
    LearningMetadataRead,
    PolicyAdmin,
    CredentialAdmin,
}

use qilbee_memory::storage::platform::CompanyMemoryAddress;
pub use qilbee_memory::storage::platform::{
    MemoryResourceScope as ResourceScope, MemoryVisibility as Visibility,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CredentialSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_policy: Option<CompanyScopePolicy>,
    pub subject_id: String,
    pub capabilities: BTreeSet<Capability>,
    pub grants: Vec<ResourceScope>,
    pub expires_at_millis: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CredentialEvent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_authority_change: Option<ScopeAuthorityChange>,
    pub revision: u64,
    pub action: String,
    pub actor_id: Uuid,
    pub at_millis: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CredentialView {
    pub id: Uuid,
    pub tenant_id: String,
    pub spec: CredentialSpec,
    pub revision: u64,
    pub revoked_at_millis: Option<i64>,
    pub history: Vec<CredentialEvent>,
}

pub struct IssuedCredential {
    pub credential: CredentialView,
    pub secret: String,
}

impl std::fmt::Debug for IssuedCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IssuedCredential")
            .field("credential", &self.credential)
            .field("secret", &"[REDACTED]")
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct AuthorizedScope {
    pub credential_id: Uuid,
    pub tenant_id: String,
    pub subject_id: String,
    pub storage_namespace: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredCredential {
    schema_version: u32,
    credential: CredentialView,
    verifier: Vec<u8>,
}

/// Persistent credential authority. Raw storage and tenant bootstrap are trusted
/// operator interfaces, never anonymous network operations. Methods perform
/// blocking I/O and must run on a blocking worker when called from async servers.
pub struct IdentityStore {
    storage: Arc<StorageEngine>,
}

impl IdentityStore {
    pub fn new(storage: Arc<StorageEngine>) -> Self {
        Self { storage }
    }

    /// Initialize a tenant exactly once. There are no built-in passwords or keys.
    /// Losing the returned administrator secret requires operator recovery; calling
    /// bootstrap again never replaces an existing tenant's authority.
    pub fn bootstrap_tenant(&self, tenant: &str, subject: &str) -> Result<IssuedCredential> {
        valid_id(tenant)?;
        let now = chrono::Utc::now().timestamp_millis();
        let spec = CredentialSpec {
            scope_policy: None,
            subject_id: subject.into(),
            capabilities: [Capability::CredentialAdmin, Capability::PolicyAdmin].into(),
            grants: vec![],
            expires_at_millis: None,
        };
        validate_spec(&spec, Some(now))?;
        let id = Uuid::new_v4();
        let (secret, verifier) = generate_secret(id)?;
        let record = new_record(id, tenant, spec, verifier, id, now, "bootstrapped");
        // JSON string encoding makes tenant identity unambiguous without a hash collision boundary.
        let tenant_key = format!("identity/v1/tenant/{}", serialize_string(tenant)?);
        let credential_key = credential_key(id);
        self.commit(
            vec![
                condition(&tenant_key, None),
                condition(&credential_key, None),
            ],
            vec![
                write(&tenant_key, tenant.as_bytes().to_vec()),
                write(&credential_key, encode(&record)?),
            ],
        )?;
        Ok(IssuedCredential {
            credential: record.credential,
            secret,
        })
    }

    /// Issue a credential inside the authenticated administrator's tenant only.
    /// Capability names grant no other capabilities. Scopes use exact grants or
    /// an explicitly selected company integration policy.
    pub fn issue(&self, admin: &str, spec: CredentialSpec) -> Result<IssuedCredential> {
        let now = chrono::Utc::now().timestamp_millis();
        let (actor, actor_bytes) = self.admin(admin, now)?;
        validate_spec(&spec, Some(now))?;
        let id = Uuid::new_v4();
        let (secret, verifier) = generate_secret(id)?;
        let record = new_record(
            id,
            &actor.credential.tenant_id,
            spec,
            verifier,
            actor.credential.id,
            now,
            "issued",
        );
        let key = credential_key(id);
        self.commit_authenticated(
            admin,
            vec![
                condition(&credential_key(actor.credential.id), Some(actor_bytes)),
                condition(&key, None),
            ],
            vec![write(&key, encode(&record)?)],
        )?;
        Ok(IssuedCredential {
            credential: record.credential,
            secret,
        })
    }

    /// Always reads the current durable credential. No cached claims bypass
    /// revocation, rotation or expiration. The returned view contains no verifier.
    pub fn authenticate(&self, token: &str) -> Result<CredentialView> {
        self.authenticate_at(token, chrono::Utc::now().timestamp_millis())
    }

    fn authenticate_at(&self, token: &str, now: i64) -> Result<CredentialView> {
        Ok(self.authenticated_record(token, now)?.0.credential)
    }

    /// Derive the namespace from authenticated tenant and subject identity.
    /// Private scopes are owned by this subject; shared scopes are available to
    /// other subjects only when their grants or explicit policy allow the same
    /// scope in the authenticated tenant.
    pub fn authorize(
        &self,
        token: &str,
        capability: Capability,
        scope: &ResourceScope,
    ) -> Result<AuthorizedScope> {
        validate_scope(scope)?;
        let credential = self.authenticate(token)?;
        if !credential.spec.capabilities.contains(&capability)
            || !scope_policy::allows(&credential.spec, scope)
        {
            return Err(denied());
        }
        let private_subject = match scope.visibility {
            Visibility::Private => Some(credential.spec.subject_id.as_str()),
            Visibility::Shared => None,
        };
        let namespace =
            CompanyMemoryAddress::new(&credential.tenant_id, scope, private_subject)?.namespace()?;
        Ok(AuthorizedScope {
            credential_id: credential.id,
            storage_namespace: namespace,
            tenant_id: credential.tenant_id,
            subject_id: credential.spec.subject_id,
        })
    }

    /// Replace the secret while preserving tenant, subject, grants and expiry.
    /// The old secret ceases to authenticate when this atomic write commits.
    pub fn rotate(&self, admin: &str, id: Uuid, revision: u64) -> Result<IssuedCredential> {
        let (record, secret) = self.change(admin, id, revision, false)?;
        Ok(IssuedCredential {
            credential: record.credential,
            secret: secret.expect("rotation generates a secret"),
        })
    }

    /// Revoke a credential irreversibly through this API, with a revision guard.
    /// Issue a new credential rather than reviving a revoked identifier.
    pub fn revoke(&self, admin: &str, id: Uuid, revision: u64) -> Result<CredentialView> {
        Ok(self.change(admin, id, revision, true)?.0.credential)
    }

    /// Read sanitized credential metadata and its immutable revision history.
    pub fn inspect(&self, admin: &str, id: Uuid) -> Result<CredentialView> {
        let (actor, _) = self.admin(admin, chrono::Utc::now().timestamp_millis())?;
        let (record, _) = self.read_record(id)?;
        if record.credential.tenant_id != actor.credential.tenant_id {
            return Err(denied());
        }
        Ok(record.credential)
    }

    fn change(
        &self,
        admin: &str,
        id: Uuid,
        revision: u64,
        revoke: bool,
    ) -> Result<(StoredCredential, Option<String>)> {
        let now = chrono::Utc::now().timestamp_millis();
        let (actor, actor_bytes) = self.admin(admin, now)?;
        let (mut record, previous) = self.read_record(id)?;
        if record.credential.tenant_id != actor.credential.tenant_id {
            return Err(denied());
        }
        if record.credential.revision != revision {
            return Err(conflict());
        }
        if record.credential.revoked_at_millis.is_some() {
            return Err(denied());
        }
        let next = revision.checked_add(1).ok_or_else(conflict)?;
        let secret = if revoke {
            record.credential.revoked_at_millis = Some(now);
            None
        } else {
            let (secret, verifier) = generate_secret(id)?;
            record.verifier = verifier;
            Some(secret)
        };
        record.credential.revision = next;
        record.credential.history.push(CredentialEvent {
            scope_authority_change: None,
            revision: next,
            action: if revoke { "revoked" } else { "rotated" }.into(),
            actor_id: actor.credential.id,
            at_millis: now,
        });
        let key = credential_key(id);
        let mut expected = vec![condition(&key, Some(previous))];
        // Checking the author's exact revision in the same batch prevents a
        // concurrent revoke/rotation from being ignored by a later credential write.
        if actor.credential.id != id {
            expected.push(condition(
                &credential_key(actor.credential.id),
                Some(actor_bytes),
            ));
        } else if expected[0].expected.as_deref() != Some(actor_bytes.as_slice()) {
            return Err(conflict());
        }
        self.commit_authenticated(admin, expected, vec![write(&key, encode(&record)?)])?;
        Ok((record, secret))
    }

    fn admin(&self, token: &str, now: i64) -> Result<(StoredCredential, Vec<u8>)> {
        let pair = self.authenticated_record(token, now)?;
        if !pair
            .0
            .credential
            .spec
            .capabilities
            .contains(&Capability::CredentialAdmin)
        {
            return Err(denied());
        }
        Ok(pair)
    }

    fn authenticated_record(&self, token: &str, now: i64) -> Result<(StoredCredential, Vec<u8>)> {
        if token.starts_with("qdbst1_") {
            return self.tenant_session_record(token, now);
        }
        use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
        use hmac::{Hmac, Mac};
        // Limit parsing work and require canonical encodings for the 256-bit secret.
        if !token.is_ascii()
            || token.len() != 81
            || !token.starts_with("qdb1_")
            || &token[37..38] != "_"
        {
            return Err(denied());
        }
        let id = Uuid::parse_str(&token[5..37]).map_err(|_| denied())?;
        if id.simple().to_string() != token[5..37] {
            return Err(denied());
        }
        let secret = URL_SAFE_NO_PAD.decode(&token[38..]).map_err(|_| denied())?;
        if secret.len() != 32 || URL_SAFE_NO_PAD.encode(&secret) != token[38..] {
            return Err(denied());
        }
        let (record, bytes) = self.read_record(id)?;
        let mut mac = Hmac::<sha2::Sha256>::new_from_slice(b"qilbeedb-credential-verifier-v1")
            .expect("HMAC accepts any key size");
        mac.update(&secret);
        // verify_slice performs a constant-time comparison through the MAC API.
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

    fn read_record(&self, id: Uuid) -> Result<(StoredCredential, Vec<u8>)> {
        let bytes = self
            .storage
            .get_meta(&credential_key(id))?
            .ok_or_else(denied)?;
        let record: StoredCredential = serde_json::from_slice(&bytes)
            .map_err(|_| Error::DataCorruption("Invalid credential record".into()))?;
        if record.schema_version != 1
            || record.credential.id != id
            || record.verifier.len() != 32
            || record.credential.revision == 0
            || record.credential.history.len() as u64 != record.credential.revision
            || record
                .credential
                .history
                .iter()
                .enumerate()
                .any(|(index, event)| event.revision != index as u64 + 1)
        {
            return Err(Error::DataCorruption(
                "Unsupported or inconsistent credential record".into(),
            ));
        }
        valid_id(&record.credential.tenant_id)?;
        validate_spec(&record.credential.spec, None)?;
        scope_policy::validate_history(&record.credential)?;
        Ok((record, bytes))
    }

    fn commit(
        &self,
        mut expected: Vec<qilbee_storage::MetadataCondition>,
        mut writes: Vec<qilbee_storage::MetadataWrite>,
    ) -> Result<()> {
        let mut indexes = Vec::new();
        for update in &writes {
            if update.key.starts_with("identity/v1/credential/") {
                if let Some(bytes) = &update.value {
                    let record: StoredCredential = serde_json::from_slice(bytes)
                        .map_err(|_| Error::DataCorruption("Invalid indexed credential".into()))?;
                    if update.key != credential_key(record.credential.id) {
                        return Err(Error::DataCorruption("Credential identity mismatch".into()));
                    }
                    let index = tenant_credential_index(
                        &record.credential.tenant_id,
                        record.credential.id,
                    )?;
                    let value = serde_json::to_vec(&record.credential.id)
                        .map_err(|e| Error::Serialization(e.to_string()))?;
                    match self.storage.get_meta(&index)? {
                        None => {
                            expected.push(condition(&index, None));
                            indexes.push(write(&index, value));
                        }
                        Some(existing) if existing == value => {}
                        Some(_) => {
                            return Err(Error::DataCorruption(
                                "Invalid tenant credential index".into(),
                            ));
                        }
                    }
                }
            }
        }
        writes.extend(indexes);
        if self.storage.compare_and_write_meta(&expected, &writes)? {
            Ok(())
        } else {
            Err(conflict())
        }
    }
}

fn denied() -> Error {
    Error::Unauthorized("Credential is invalid or lacks the required authority".into())
}
fn conflict() -> Error {
    Error::TransactionConflict("Credential revision or tenant authority changed".into())
}
fn tenant_credential_prefix(tenant: &str) -> Result<String> {
    Ok(format!(
        "identity/v1/tenant-credential/{}/",
        serialize_string(tenant)?
    ))
}
fn tenant_credential_index(tenant: &str, id: Uuid) -> Result<String> {
    Ok(format!("{}{id}", tenant_credential_prefix(tenant)?))
}
fn credential_key(id: Uuid) -> String {
    format!("identity/v1/credential/{id}")
}
fn condition(key: &str, expected: Option<Vec<u8>>) -> qilbee_storage::MetadataCondition {
    qilbee_storage::MetadataCondition {
        key: key.into(),
        expected,
    }
}
fn write(key: &str, value: Vec<u8>) -> qilbee_storage::MetadataWrite {
    qilbee_storage::MetadataWrite {
        key: key.into(),
        value: Some(value),
    }
}
fn encode(record: &StoredCredential) -> Result<Vec<u8>> {
    serde_json::to_vec(record).map_err(|e| Error::Serialization(e.to_string()))
}
fn serialize_string(value: &str) -> Result<String> {
    serde_json::to_string(value).map_err(|e| Error::Serialization(e.to_string()))
}
fn valid_id(value: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
        Err(Error::ValidationError(
            "Identity components must contain 1..256 UTF-8 bytes and no control characters".into(),
        ))
    } else {
        Ok(())
    }
}
fn validate_scope(scope: &ResourceScope) -> Result<()> {
    valid_id(&scope.project_id)?;
    valid_id(&scope.agent_id)?;
    if let Some(mission) = &scope.mission_id {
        valid_id(mission)?;
    }
    Ok(())
}
fn validate_spec(spec: &CredentialSpec, now: Option<i64>) -> Result<()> {
    valid_id(&spec.subject_id)?;
    if spec.capabilities.is_empty() {
        return Err(Error::ValidationError(
            "At least one explicit capability is required".into(),
        ));
    }
    if now.is_some_and(|now| spec.expires_at_millis.is_some_and(|expires| expires <= now)) {
        return Err(Error::ValidationError(
            "Credential expiry must be in the future".into(),
        ));
    }
    scope_policy::validate(spec)?;
    for grant in &spec.grants {
        validate_scope(grant)?;
    }
    Ok(())
}
fn generate_secret(id: Uuid) -> Result<(String, Vec<u8>)> {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    use hmac::{Hmac, Mac};
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| Error::Internal("Operating system randomness unavailable".into()))?;
    let mut mac = Hmac::<sha2::Sha256>::new_from_slice(b"qilbeedb-credential-verifier-v1")
        .expect("HMAC accepts any key size");
    mac.update(&bytes);
    Ok((
        format!("qdb1_{}_{}", id.simple(), URL_SAFE_NO_PAD.encode(bytes)),
        mac.finalize().into_bytes().to_vec(),
    ))
}
fn new_record(
    id: Uuid,
    tenant: &str,
    spec: CredentialSpec,
    verifier: Vec<u8>,
    actor: Uuid,
    now: i64,
    action: &str,
) -> StoredCredential {
    StoredCredential {
        schema_version: 1,
        verifier,
        credential: CredentialView {
            id,
            tenant_id: tenant.into(),
            spec,
            revision: 1,
            revoked_at_millis: None,
            history: vec![CredentialEvent {
                scope_authority_change: None,
                revision: 1,
                action: action.into(),
                actor_id: actor,
                at_millis: now,
            }],
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qilbee_storage::StorageOptions;
    use tempfile::TempDir;

    fn store(path: &std::path::Path) -> IdentityStore {
        IdentityStore::new(Arc::new(
            StorageEngine::open(StorageOptions::for_testing(path)).unwrap(),
        ))
    }
    fn scope() -> ResourceScope {
        ResourceScope {
            project_id: "project".into(),
            mission_id: Some("mission".into()),
            agent_id: "agent".into(),
            visibility: Visibility::Shared,
        }
    }
    fn spec(subject: &str) -> CredentialSpec {
        CredentialSpec {
            scope_policy: None,
            subject_id: subject.into(),
            capabilities: [Capability::MemoryRead, Capability::ProcedurePropose].into(),
            grants: vec![scope()],
            expires_at_millis: None,
        }
    }
    #[test]
    fn identity_credentials_survive_reopen_without_persisting_secrets() {
        let dir = TempDir::new().unwrap();
        let (admin, key) = {
            let db = store(dir.path());
            let admin = db.bootstrap_tenant("tenant-a", "operator").unwrap();
            let key = db.issue(&admin.secret, spec("alice")).unwrap();
            let bytes = db
                .storage
                .get_meta(&format!("identity/v1/credential/{}", key.credential.id))
                .unwrap()
                .unwrap();
            let json = String::from_utf8(bytes).unwrap();
            assert!(!json.contains(&key.secret));
            assert!(!json.contains(&key.secret[38..]));
            assert!(!format!("{key:?}").contains(&key.secret));
            (admin, key)
        };
        let db = store(dir.path());
        assert_eq!(db.authenticate(&key.secret).unwrap().tenant_id, "tenant-a");
        assert_eq!(
            db.inspect(&admin.secret, key.credential.id)
                .unwrap()
                .history
                .len(),
            1
        );
        assert!(db.bootstrap_tenant("tenant-a", "different").is_err());
    }
    #[test]
    fn identity_exact_scopes_and_independent_capabilities_deny_escalation() {
        let dir = TempDir::new().unwrap();
        let db = store(dir.path());
        let admin = db.bootstrap_tenant("tenant-a", "operator").unwrap();
        let key = db.issue(&admin.secret, spec("alice")).unwrap();
        db.authorize(&key.secret, Capability::ProcedurePropose, &scope())
            .unwrap();
        for cap in [
            Capability::MemoryWrite,
            Capability::ProcedureEvaluate,
            Capability::PolicyAdmin,
            Capability::CredentialAdmin,
        ] {
            assert!(db.authorize(&key.secret, cap, &scope()).is_err());
        }
        assert!(db.issue(&key.secret, spec("mallory")).is_err());
        for changed in [
            ResourceScope {
                project_id: "other".into(),
                ..scope()
            },
            ResourceScope {
                mission_id: None,
                ..scope()
            },
            ResourceScope {
                agent_id: "other".into(),
                ..scope()
            },
            ResourceScope {
                visibility: Visibility::Private,
                ..scope()
            },
        ] {
            assert!(
                db.authorize(&key.secret, Capability::MemoryRead, &changed)
                    .is_err()
            );
        }
        assert!(
            db.authorize(&admin.secret, Capability::MemoryRead, &scope())
                .is_err()
        );
    }
    #[test]
    fn identity_tenant_and_private_subject_namespaces_cannot_alias() {
        let dir = TempDir::new().unwrap();
        let db = store(dir.path());
        let a = db.bootstrap_tenant("tenant-a", "operator").unwrap();
        let b = db.bootstrap_tenant("tenant-b", "operator").unwrap();
        let ka = db.issue(&a.secret, spec("alice")).unwrap();
        let kb = db.issue(&b.secret, spec("alice")).unwrap();
        let ns = |key: &str, scope: &ResourceScope| {
            db.authorize(key, Capability::MemoryRead, scope)
                .unwrap()
                .storage_namespace
        };
        assert_ne!(ns(&ka.secret, &scope()), ns(&kb.secret, &scope()));
        assert!(db.rotate(&a.secret, kb.credential.id, 1).is_err());
        assert!(db.revoke(&a.secret, kb.credential.id, 1).is_err());
        assert!(db.inspect(&a.secret, kb.credential.id).is_err());
        let private = ResourceScope {
            visibility: Visibility::Private,
            ..scope()
        };
        let private_spec = |name: &str| CredentialSpec {
            scope_policy: None,
            grants: vec![private.clone()],
            ..spec(name)
        };
        let alice = db.issue(&a.secret, private_spec("alice")).unwrap();
        let bob = db.issue(&a.secret, private_spec("bob")).unwrap();
        assert_ne!(ns(&alice.secret, &private), ns(&bob.secret, &private));
        let shared_bob = db.issue(&a.secret, spec("bob")).unwrap();
        assert_eq!(ns(&ka.secret, &scope()), ns(&shared_bob.secret, &scope()));
    }
    #[test]
    fn identity_rotation_and_revocation_are_revisioned_and_durable() {
        let dir = TempDir::new().unwrap();
        let (admin, key, rotated) = {
            let db = store(dir.path());
            let admin = db.bootstrap_tenant("tenant-a", "operator").unwrap();
            let key = db.issue(&admin.secret, spec("alice")).unwrap();
            let rotated = db.rotate(&admin.secret, key.credential.id, 1).unwrap();
            assert!(db.authenticate(&key.secret).is_err());
            assert!(db.rotate(&admin.secret, key.credential.id, 1).is_err());
            let revoked = db.revoke(&admin.secret, key.credential.id, 2).unwrap();
            assert_eq!(revoked.revision, 3);
            assert_eq!(
                revoked
                    .history
                    .iter()
                    .map(|e| e.action.as_str())
                    .collect::<Vec<_>>(),
                ["issued", "rotated", "revoked"]
            );
            (admin, key, rotated)
        };
        let db = store(dir.path());
        assert!(db.authenticate(&key.secret).is_err());
        assert!(db.authenticate(&rotated.secret).is_err());
        assert!(db.rotate(&admin.secret, key.credential.id, 3).is_err());
        assert_eq!(
            db.inspect(&admin.secret, key.credential.id)
                .unwrap()
                .revision,
            3
        );
    }
    #[test]
    fn identity_expiry_malformed_inputs_and_unknown_schema_fail_closed() {
        let dir = TempDir::new().unwrap();
        let db = store(dir.path());
        let admin = db.bootstrap_tenant("tenant-a", "operator").unwrap();
        let expires = chrono::Utc::now().timestamp_millis() + 60_000;
        let key = db
            .issue(
                &admin.secret,
                CredentialSpec {
                    scope_policy: None,
                    expires_at_millis: Some(expires),
                    ..spec("alice")
                },
            )
            .unwrap();
        assert!(db.authenticate_at(&key.secret, expires).is_err());
        assert!(
            db.issue(
                &admin.secret,
                CredentialSpec {
                    scope_policy: None,
                    expires_at_millis: Some(0),
                    ..spec("alice")
                }
            )
            .is_err()
        );
        assert!(db.issue(&admin.secret, spec(" ")).is_err());
        assert!(db.authenticate("qdb1_invalid_secret").is_err());
        assert!(db.authenticate(&format!("{}a", "é".repeat(40))).is_err());
        let mut changed = key.secret.clone().into_bytes();
        changed[45] = if changed[45] == b'A' { b'B' } else { b'A' };
        assert!(
            db.authenticate(std::str::from_utf8(&changed).unwrap())
                .is_err()
        );
        let storage_key = format!("identity/v1/credential/{}", key.credential.id);
        let mut value: serde_json::Value =
            serde_json::from_slice(&db.storage.get_meta(&storage_key).unwrap().unwrap()).unwrap();
        value["schema_version"] = 99.into();
        db.storage
            .put_meta(&storage_key, &serde_json::to_vec(&value).unwrap())
            .unwrap();
        assert!(db.authenticate(&key.secret).is_err());
    }
    #[test]
    fn identity_concurrent_bootstrap_and_rotation_have_one_winner() {
        let dir = TempDir::new().unwrap();
        let db = Arc::new(store(dir.path()));
        let gate = Arc::new(std::sync::Barrier::new(8));
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let db = db.clone();
                let gate = gate.clone();
                std::thread::spawn(move || {
                    gate.wait();
                    db.bootstrap_tenant("tenant-a", "operator")
                })
            })
            .collect();
        let winners: Vec<_> = threads
            .into_iter()
            .filter_map(|t| t.join().unwrap().ok())
            .collect();
        assert_eq!(winners.len(), 1);
        let admin = &winners[0];
        let key = db.issue(&admin.secret, spec("alice")).unwrap();
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let db = db.clone();
                let gate = gate.clone();
                let token = admin.secret.clone();
                let id = key.credential.id;
                std::thread::spawn(move || {
                    gate.wait();
                    db.rotate(&token, id, 1)
                })
            })
            .collect();
        assert_eq!(
            threads
                .into_iter()
                .filter_map(|t| t.join().unwrap().ok())
                .count(),
            1
        );
        assert_eq!(
            db.inspect(&admin.secret, key.credential.id)
                .unwrap()
                .revision,
            2
        );
    }
    #[test]
    fn identity_admin_self_rotation_and_strict_specs_preserve_authority() {
        let dir = TempDir::new().unwrap();
        let db = store(dir.path());
        let admin = db.bootstrap_tenant("tenant-a", "operator").unwrap();
        let replacement = db.rotate(&admin.secret, admin.credential.id, 1).unwrap();
        assert!(db.issue(&admin.secret, spec("alice")).is_err());
        db.issue(&replacement.secret, spec("alice")).unwrap();
        let mut payload = serde_json::to_value(spec("alice")).unwrap();
        payload["tenant_id"] = "tenant-b".into();
        assert!(serde_json::from_value::<CredentialSpec>(payload).is_err());
        db.revoke(&replacement.secret, replacement.credential.id, 2)
            .unwrap();
        assert!(db.issue(&replacement.secret, spec("alice")).is_err());
        assert!(db.bootstrap_tenant("tenant-a", "operator").is_err());
    }
}
