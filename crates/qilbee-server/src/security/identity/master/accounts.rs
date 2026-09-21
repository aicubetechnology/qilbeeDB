//! Durable human accounts authenticate to bounded sessions, never reveal API keys.
use super::*;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};

const SESSION_MILLIS: i64 = 15 * 60 * 1000;
const MAX_SESSIONS: usize = 32;
const LOCK_MILLIS: i64 = 5 * 60 * 1000;
const SESSION_DOMAIN: &[u8] = b"qilbeedb-login-session-v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LoginAuthority {
    Global,
    Tenant { tenant_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoginAccountView {
    pub id: Uuid,
    pub username: String,
    pub authority: LoginAuthority,
    pub credential_id: Uuid,
    pub revision: u64,
    pub disabled_at_millis: Option<i64>,
    pub history: Vec<CredentialEvent>,
}

#[derive(Serialize)]
pub struct LoginSession {
    pub token: String,
    pub token_type: &'static str,
    pub expires_at_millis: i64,
    pub account: LoginAccountView,
}

impl std::fmt::Debug for LoginSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoginSession")
            .field("token", &"[REDACTED]")
            .field("account", &self.account)
            .field("expires_at_millis", &self.expires_at_millis)
            .finish()
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionVerifier {
    id: Uuid,
    verifier: Vec<u8>,
    parent_revision: u64,
    expires_at_millis: i64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Account {
    schema_version: u32,
    view: LoginAccountView,
    password_hash: String,
    failed_attempts: u32,
    blocked_until_millis: i64,
    sessions: Vec<SessionVerifier>,
}

fn account_key(id: Uuid) -> String {
    format!("identity/v1/login-account/{id}")
}
fn name_key(authority: &LoginAuthority, username: &str) -> Result<String> {
    valid_id(username)?;
    if let LoginAuthority::Tenant { tenant_id } = authority {
        valid_id(tenant_id)?;
    }
    Ok(format!(
        "identity/v1/login-name/{}",
        serde_json::to_string(&(authority, username))
            .map_err(|e| Error::Serialization(e.to_string()))?
    ))
}

fn password_hash(password: &str) -> Result<String> {
    if !(8..=1024).contains(&password.len()) {
        return Err(Error::ValidationError(
            "Passwords must contain 8..1024 UTF-8 bytes".into(),
        ));
    }
    let salt = SaltString::generate(&mut rand::rngs::OsRng);
    // Argon2id v19, m=19456 KiB, t=2, p=1. A random salt is generated per change.
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|p| p.to_string())
        .map_err(|_| Error::Internal("Password hashing failed".into()))
}

fn verify_password(password: &str, hash: &str) -> Result<bool> {
    let parsed = PasswordHash::new(hash)
        .map_err(|_| Error::DataCorruption("Invalid password verifier".into()))?;
    if parsed.algorithm.as_str() != "argon2id"
        || parsed.version != Some(19)
        || parsed.params.get_decimal("m") != Some(19456)
        || parsed.params.get_decimal("t") != Some(2)
        || parsed.params.get_decimal("p") != Some(1)
    {
        return Err(Error::DataCorruption(
            "Unsupported password verifier parameters".into(),
        ));
    }
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

impl IdentityStore {
    fn read_account(&self, id: Uuid) -> Result<(Account, Vec<u8>)> {
        let bytes = self
            .storage
            .get_meta(&account_key(id))?
            .ok_or_else(denied)?;
        let account: Account = serde_json::from_slice(&bytes)
            .map_err(|_| Error::DataCorruption("Invalid login account".into()))?;
        if account.schema_version != 1
            || account.view.id != id
            || account.view.revision == 0
            || account.view.history.len() as u64 != account.view.revision
            || account
                .view
                .history
                .iter()
                .enumerate()
                .any(|(i, e)| e.revision != i as u64 + 1)
            || account.sessions.len() > MAX_SESSIONS
            || account
                .sessions
                .iter()
                .any(|s| s.verifier.len() != 32 || s.parent_revision == 0)
            || account
                .sessions
                .iter()
                .map(|s| s.id)
                .collect::<BTreeSet<_>>()
                .len()
                != account.sessions.len()
            || account.password_hash.len() > 256
        {
            return Err(Error::DataCorruption("Inconsistent login account".into()));
        }
        name_key(&account.view.authority, &account.view.username)?;
        Ok((account, bytes))
    }

    fn account_admin(
        &self,
        token: &str,
        global: bool,
    ) -> Result<(LoginAuthority, Uuid, qilbee_storage::MetadataCondition)> {
        let now = chrono::Utc::now().timestamp_millis();
        if global {
            let (actor, bytes) = self.authenticated_global(token, now)?;
            if !actor.credential.is_master {
                return Err(denied());
            }
            Ok((
                LoginAuthority::Global,
                actor.credential.id,
                condition(&global_key(actor.credential.id), Some(bytes)),
            ))
        } else {
            let (actor, bytes) = self.admin(token, now)?;
            Ok((
                LoginAuthority::Tenant {
                    tenant_id: actor.credential.tenant_id,
                },
                actor.credential.id,
                condition(&credential_key(actor.credential.id), Some(bytes)),
            ))
        }
    }

    fn account_parent(
        &self,
        authority: &LoginAuthority,
        id: Uuid,
        now: i64,
    ) -> Result<(u64, Option<i64>, qilbee_storage::MetadataCondition)> {
        match authority {
            LoginAuthority::Global => {
                let (parent, bytes) = self.read_global(id)?;
                if parent.credential.revoked_at_millis.is_some()
                    || parent
                        .credential
                        .spec
                        .expires_at_millis
                        .is_some_and(|e| now >= e)
                {
                    return Err(denied());
                }
                Ok((
                    parent.credential.revision,
                    parent.credential.spec.expires_at_millis,
                    condition(&global_key(id), Some(bytes)),
                ))
            }
            LoginAuthority::Tenant { tenant_id } => {
                let (parent, bytes) = self.read_record(id)?;
                if &parent.credential.tenant_id != tenant_id
                    || parent.credential.revoked_at_millis.is_some()
                    || parent
                        .credential
                        .spec
                        .expires_at_millis
                        .is_some_and(|e| now >= e)
                {
                    return Err(denied());
                }
                Ok((
                    parent.credential.revision,
                    parent.credential.spec.expires_at_millis,
                    condition(&credential_key(id), Some(bytes)),
                ))
            }
        }
    }

    /// Only the installation master or an authenticated administrator of this tenant can appoint users.
    pub fn create_login_account(
        &self,
        token: &str,
        global: bool,
        username: &str,
        password: &str,
        credential_id: Uuid,
    ) -> Result<LoginAccountView> {
        let (authority, actor, actor_fence) = self.account_admin(token, global)?;
        let index = name_key(&authority, username)?;
        let now = chrono::Utc::now().timestamp_millis();
        let (_, _, parent_fence) = self.account_parent(&authority, credential_id, now)?;
        let hash = password_hash(password)?;
        let id = Uuid::new_v4();
        let account = Account {
            schema_version: 1,
            view: LoginAccountView {
                id,
                username: username.into(),
                authority,
                credential_id,
                revision: 1,
                disabled_at_millis: None,
                history: vec![CredentialEvent {
                    revision: 1,
                    action: "login_account_created".into(),
                    actor_id: actor,
                    at_millis: now,
                }],
            },
            password_hash: hash,
            failed_attempts: 0,
            blocked_until_millis: 0,
            sessions: vec![],
        };
        self.commit_authenticated(
            token,
            vec![
                actor_fence,
                parent_fence,
                condition(&index, None),
                condition(&account_key(id), None),
            ],
            vec![
                write(&index, serialize(&id)?),
                write(&account_key(id), serialize(&account)?),
            ],
        )?;
        Ok(account.view)
    }

    pub fn inspect_login_account(
        &self,
        token: &str,
        global: bool,
        id: Uuid,
    ) -> Result<LoginAccountView> {
        let (authority, _, _) = self.account_admin(token, global)?;
        let (account, _) = self.read_account(id)?;
        if account.view.authority != authority {
            return Err(denied());
        }
        Ok(account.view)
    }

    /// Password replacement and disablement invalidate every session atomically; API keys are unchanged.
    pub fn change_login_account(
        &self,
        token: &str,
        global: bool,
        id: Uuid,
        revision: u64,
        password: Option<&str>,
    ) -> Result<LoginAccountView> {
        let (authority, actor, actor_fence) = self.account_admin(token, global)?;
        let (mut account, previous) = self.read_account(id)?;
        if account.view.authority != authority || account.view.disabled_at_millis.is_some() {
            return Err(denied());
        }
        if account.view.revision != revision {
            return Err(conflict());
        }
        let now = chrono::Utc::now().timestamp_millis();
        let action = if let Some(password) = password {
            account.password_hash = password_hash(password)?;
            "login_password_changed"
        } else {
            account.view.disabled_at_millis = Some(now);
            "login_account_disabled"
        };
        account.sessions.clear();
        account.failed_attempts = 0;
        account.blocked_until_millis = 0;
        account.view.revision = revision.checked_add(1).ok_or_else(conflict)?;
        account.view.history.push(CredentialEvent {
            revision: account.view.revision,
            action: action.into(),
            actor_id: actor,
            at_millis: now,
        });
        self.commit_authenticated(
            token,
            vec![actor_fence, condition(&account_key(id), Some(previous))],
            vec![write(&account_key(id), serialize(&account)?)],
        )?;
        Ok(account.view)
    }

    pub fn login(
        &self,
        authority: LoginAuthority,
        username: &str,
        password: &str,
    ) -> Result<LoginSession> {
        self.login_at(
            authority,
            username,
            password,
            chrono::Utc::now().timestamp_millis(),
        )
    }

    fn login_at(
        &self,
        authority: LoginAuthority,
        username: &str,
        password: &str,
        now: i64,
    ) -> Result<LoginSession> {
        let index = name_key(&authority, username)?;
        if password.len() > 1024 {
            return Err(Error::ValidationError("Password is too long".into()));
        }
        let Some(index_bytes) = self.storage.get_meta(&index)? else {
            // Unknown users incur the same Argon2 work without persisting attacker-selected names.
            password_hash(if password.len() >= 8 {
                password
            } else {
                "unknown-account-dummy"
            })?;
            return Err(denied());
        };
        let id: Uuid = serde_json::from_slice(&index_bytes)
            .map_err(|_| Error::DataCorruption("Invalid login index".into()))?;
        let (mut account, previous) = self.read_account(id)?;
        if account.view.authority != authority || account.view.username != username {
            return Err(Error::DataCorruption("Inconsistent login index".into()));
        }
        let valid = verify_password(password, &account.password_hash)?;
        if !valid || account.view.disabled_at_millis.is_some() || now < account.blocked_until_millis
        {
            // A durable five-minute lock follows five failures; attempts during a lock do not extend it.
            if now >= account.blocked_until_millis && account.view.disabled_at_millis.is_none() {
                if account.blocked_until_millis != 0 {
                    account.failed_attempts = 0;
                    account.blocked_until_millis = 0;
                }
                account.failed_attempts += 1;
                if account.failed_attempts >= 5 {
                    account.blocked_until_millis = now.saturating_add(LOCK_MILLIS);
                }
                self.commit(
                    vec![condition(&account_key(id), Some(previous))],
                    vec![write(&account_key(id), serialize(&account)?)],
                )?;
            }
            return Err(denied());
        }
        let (parent_revision, parent_expiry, parent_fence) =
            self.account_parent(&authority, account.view.credential_id, now)?;
        let expires = parent_expiry.map_or(now.saturating_add(SESSION_MILLIS), |e| {
            e.min(now.saturating_add(SESSION_MILLIS))
        });
        let session_id = Uuid::new_v4();
        let mut bytes = [0u8; 32];
        use rand::RngCore;
        rand::rngs::OsRng
            .try_fill_bytes(&mut bytes)
            .map_err(|_| Error::Internal("Operating system randomness unavailable".into()))?;
        let mut mac = Hmac::<sha2::Sha256>::new_from_slice(SESSION_DOMAIN)
            .expect("HMAC accepts any key size");
        mac.update(&bytes);
        account
            .sessions
            .retain(|s| now < s.expires_at_millis && s.parent_revision == parent_revision);
        if account.sessions.len() == MAX_SESSIONS {
            account.sessions.remove(0);
        }
        account.sessions.push(SessionVerifier {
            id: session_id,
            verifier: mac.finalize().into_bytes().to_vec(),
            parent_revision,
            expires_at_millis: expires,
        });
        account.failed_attempts = 0;
        account.blocked_until_millis = 0;
        self.commit(
            vec![
                condition(&index, Some(index_bytes)),
                condition(&account_key(id), Some(previous)),
                parent_fence,
            ],
            vec![write(&account_key(id), serialize(&account)?)],
        )?;
        let prefix = if authority == LoginAuthority::Global {
            "qdbsg1_"
        } else {
            "qdbst1_"
        };
        Ok(LoginSession {
            token: format!(
                "{prefix}{}_{}_{}",
                id.simple(),
                session_id.simple(),
                URL_SAFE_NO_PAD.encode(bytes)
            ),
            token_type: "Bearer",
            expires_at_millis: expires,
            account: account.view,
        })
    }

    fn session_account(
        &self,
        token: &str,
        now: i64,
    ) -> Result<(Account, Vec<u8>, Uuid, qilbee_storage::MetadataCondition)> {
        if !token.is_ascii()
            || token.len() != 116
            || !(token.starts_with("qdbsg1_") || token.starts_with("qdbst1_"))
            || &token[39..40] != "_"
            || &token[72..73] != "_"
        {
            return Err(denied());
        }
        let id = Uuid::parse_str(&token[7..39]).map_err(|_| denied())?;
        let sid = Uuid::parse_str(&token[40..72]).map_err(|_| denied())?;
        if id.simple().to_string() != token[7..39] || sid.simple().to_string() != token[40..72] {
            return Err(denied());
        }
        let secret = URL_SAFE_NO_PAD.decode(&token[73..]).map_err(|_| denied())?;
        if secret.len() != 32 || URL_SAFE_NO_PAD.encode(&secret) != token[73..] {
            return Err(denied());
        }
        let (account, bytes) = self.read_account(id)?;
        if account.view.disabled_at_millis.is_some()
            || (account.view.authority == LoginAuthority::Global) != token.starts_with("qdbsg1_")
        {
            return Err(denied());
        }
        let session = account
            .sessions
            .iter()
            .find(|s| s.id == sid)
            .ok_or_else(denied)?;
        let mut mac = Hmac::<sha2::Sha256>::new_from_slice(SESSION_DOMAIN)
            .expect("HMAC accepts any key size");
        mac.update(&secret);
        mac.verify_slice(&session.verifier).map_err(|_| denied())?;
        if now >= session.expires_at_millis {
            return Err(denied());
        }
        let (revision, _, parent_fence) =
            self.account_parent(&account.view.authority, account.view.credential_id, now)?;
        if revision != session.parent_revision {
            return Err(denied());
        }
        Ok((account, bytes, sid, parent_fence))
    }

    pub fn logout(&self, token: &str) -> Result<()> {
        let (mut account, previous, sid, parent_fence) =
            self.session_account(token, chrono::Utc::now().timestamp_millis())?;
        account.sessions.retain(|s| s.id != sid);
        self.commit(
            vec![
                condition(&account_key(account.view.id), Some(previous)),
                parent_fence,
            ],
            vec![write(&account_key(account.view.id), serialize(&account)?)],
        )
    }

    pub(in crate::security::identity) fn tenant_session_record(
        &self,
        token: &str,
        now: i64,
    ) -> Result<(StoredCredential, Vec<u8>)> {
        let (account, _, _, fence) = self.session_account(token, now)?;
        if !matches!(account.view.authority, LoginAuthority::Tenant { .. }) {
            return Err(denied());
        }
        let bytes = fence.expected.ok_or_else(denied)?;
        let record = serde_json::from_slice(&bytes)
            .map_err(|_| Error::DataCorruption("Invalid session authority".into()))?;
        Ok((record, bytes))
    }

    pub(super) fn global_session_record(
        &self,
        token: &str,
        now: i64,
    ) -> Result<(StoredGlobalCredential, Vec<u8>)> {
        let (account, _, _, fence) = self.session_account(token, now)?;
        if account.view.authority != LoginAuthority::Global {
            return Err(denied());
        }
        let bytes = fence.expected.ok_or_else(denied)?;
        let record = serde_json::from_slice(&bytes)
            .map_err(|_| Error::DataCorruption("Invalid session authority".into()))?;
        Ok((record, bytes))
    }

    pub(in crate::security::identity) fn commit_authenticated(
        &self,
        token: &str,
        mut expected: Vec<qilbee_storage::MetadataCondition>,
        writes: Vec<qilbee_storage::MetadataWrite>,
    ) -> Result<()> {
        if token.starts_with("qdbsg1_") || token.starts_with("qdbst1_") {
            let (account, bytes, _, parent_fence) =
                self.session_account(token, chrono::Utc::now().timestamp_millis())?;
            expected.push(condition(&account_key(account.view.id), Some(bytes)));
            expected.push(parent_fence);
        }
        // The actor may also be the target, or the account's underlying authority.
        let mut unique = std::collections::BTreeMap::new();
        for c in expected {
            if let Some(old) = unique.insert(c.key, c.expected.clone()) {
                if old != c.expected {
                    return Err(conflict());
                }
            }
        }
        self.commit(
            unique
                .into_iter()
                .map(|(key, expected)| qilbee_storage::MetadataCondition { key, expected })
                .collect(),
            writes,
        )
    }
}

#[cfg(test)]
mod tests;
