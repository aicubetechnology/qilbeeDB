//! Scoped administrative directories contain sanitized views, never verifiers.
use super::*;

#[derive(Debug, Serialize)]
pub struct DirectoryPage<T: Serialize> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}
#[derive(Debug, Serialize)]
pub struct TenantDirectoryEntry {
    pub tenant_id: String,
    pub registration: Option<TenantView>,
}

fn page<T: Serialize>(items: Vec<T>, keys: Vec<String>, more: bool) -> Result<DirectoryPage<T>> {
    let result = DirectoryPage {
        items,
        next_cursor: if more {
            keys.last().map(|key| URL_SAFE_NO_PAD.encode(key))
        } else {
            None
        },
    };
    if serialize(&result)?.len() > 4 * 1024 * 1024 {
        return Err(Error::ValidationError(
            "Directory response exceeds 4 MiB; lower the page limit".into(),
        ));
    }
    Ok(result)
}

impl IdentityStore {
    pub fn prepare_administration_directory(&self) -> Result<()> {
        self.storage.ensure_ordered_metadata_keys()?;
        let mut after = None;
        loop {
            let (keys, more) =
                self.storage
                    .scan_meta_keys("identity/v1/credential/", after.as_deref(), 100)?;
            for key in &keys {
                let id = Uuid::parse_str(
                    key.strip_prefix("identity/v1/credential/")
                        .ok_or_else(denied)?,
                )
                .map_err(|_| Error::DataCorruption("Invalid credential directory key".into()))?;
                let (record, bytes) = self.read_record(id)?;
                let index = tenant_credential_index(&record.credential.tenant_id, id)?;
                let value = serialize(&id)?;
                match self.storage.get_meta(&index)? {
                    Some(existing) if existing == value => {}
                    Some(_) => {
                        return Err(Error::DataCorruption(
                            "Invalid tenant credential index".into(),
                        ));
                    }
                    None => self.commit(
                        vec![condition(key, Some(bytes)), condition(&index, None)],
                        vec![write(&index, value)],
                    )?,
                }
            }
            if !more {
                break;
            }
            after = keys.last().cloned();
        }
        Ok(())
    }

    fn directory_keys(
        &self,
        prefix: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<(Vec<String>, bool)> {
        let after = cursor
            .map(|cursor| {
                if cursor.len() > 8192 {
                    return Err(Error::ValidationError("Invalid directory cursor".into()));
                }
                let bytes = URL_SAFE_NO_PAD
                    .decode(cursor)
                    .map_err(|_| Error::ValidationError("Invalid directory cursor".into()))?;
                if URL_SAFE_NO_PAD.encode(&bytes) != cursor {
                    return Err(Error::ValidationError("Invalid directory cursor".into()));
                }
                String::from_utf8(bytes)
                    .map_err(|_| Error::ValidationError("Invalid directory cursor".into()))
            })
            .transpose()?;
        self.storage.scan_meta_keys(prefix, after.as_deref(), limit)
    }

    pub fn list_tenants(
        &self,
        token: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<DirectoryPage<TenantDirectoryEntry>> {
        self.global_authority(token, GlobalCapability::TenantInspect)?;
        let (keys, more) = self.directory_keys("identity/v1/tenant/", cursor, limit)?;
        let mut items = Vec::new();
        for key in &keys {
            let tenant: String =
                serde_json::from_str(key.strip_prefix("identity/v1/tenant/").ok_or_else(denied)?)
                    .map_err(|_| Error::DataCorruption("Invalid tenant index".into()))?;
            let registration = self.inspect_tenant(token, &tenant)?;
            items.push(TenantDirectoryEntry {
                tenant_id: tenant,
                registration,
            });
        }
        page(items, keys, more)
    }

    pub fn list_tenant_credentials(
        &self,
        token: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<DirectoryPage<CredentialView>> {
        let (actor, _) = self.admin(token, chrono::Utc::now().timestamp_millis())?;
        let prefix = tenant_credential_prefix(&actor.credential.tenant_id)?;
        let (keys, more) = self.directory_keys(&prefix, cursor, limit)?;
        let mut items = Vec::new();
        for key in &keys {
            let id = Uuid::parse_str(key.strip_prefix(&prefix).ok_or_else(denied)?)
                .map_err(|_| Error::DataCorruption("Invalid credential index".into()))?;
            let (record, _) = self.read_record(id).map_err(|_| {
                Error::DataCorruption("Missing or invalid credential directory record".into())
            })?;
            if record.credential.tenant_id != actor.credential.tenant_id {
                return Err(Error::DataCorruption(
                    "Credential directory crosses tenant boundary".into(),
                ));
            }
            items.push(record.credential);
        }
        page(items, keys, more)
    }

    pub fn list_global_credentials(
        &self,
        token: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<DirectoryPage<GlobalCredentialView>> {
        let (actor, _) = self.global_authority(token, GlobalCapability::GlobalCredentialAdmin)?;
        // Master-only enumeration prevents a delegated subset from learning other authorities.
        if !actor.credential.is_master {
            return Err(denied());
        }
        let (keys, more) = self.directory_keys("identity/v1/global-credential/", cursor, limit)?;
        let mut items = Vec::new();
        for key in &keys {
            let id = Uuid::parse_str(
                key.strip_prefix("identity/v1/global-credential/")
                    .ok_or_else(denied)?,
            )
            .map_err(|_| Error::DataCorruption("Invalid global credential index".into()))?;
            items.push(self.read_global(id)?.0.credential);
        }
        page(items, keys, more)
    }

    pub fn list_login_accounts(
        &self,
        token: &str,
        global: bool,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<DirectoryPage<LoginAccountView>> {
        let authority = if global {
            let (actor, _) =
                self.authenticated_global(token, chrono::Utc::now().timestamp_millis())?;
            if !actor.credential.is_master {
                return Err(denied());
            }
            LoginAuthority::Global
        } else {
            let (actor, _) = self.admin(token, chrono::Utc::now().timestamp_millis())?;
            LoginAuthority::Tenant {
                tenant_id: actor.credential.tenant_id,
            }
        };
        let encoded =
            serde_json::to_string(&authority).map_err(|e| Error::Serialization(e.to_string()))?;
        let prefix = format!("identity/v1/login-name/[{encoded},");
        let (keys, more) = self.directory_keys(&prefix, cursor, limit)?;
        let mut items = Vec::new();
        for key in &keys {
            let bytes = self
                .storage
                .get_meta(key)?
                .ok_or_else(|| Error::DataCorruption("Missing account directory record".into()))?;
            let id = serde_json::from_slice(&bytes)
                .map_err(|_| Error::DataCorruption("Invalid account directory record".into()))?;
            items.push(self.inspect_login_account(token, global, id)?);
        }
        page(items, keys, more)
    }
}

#[cfg(test)]
mod tests;
