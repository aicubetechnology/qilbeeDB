//! Company-prefixed discovery derived from canonical memory storage, not grants.
use super::*;
mod address;
mod inventory;
pub use address::{CompanyMemoryAddress, MemoryResourceScope, MemoryVisibility};
pub use inventory::*;

const WORKSPACE: u8 = 0x41;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyMemoryWorkspace {
    pub workspace_id: String,
    pub company_id: String,
    pub scope: MemoryResourceScope,
    pub private_subject_id: Option<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredWorkspace {
    schema_version: u32,
    workspace: CompanyMemoryWorkspace,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyMemoryWorkspacePage {
    pub workspaces: Vec<CompanyMemoryWorkspace>,
    pub next_after_workspace_id: Option<String>,
}
impl CompanyMemoryWorkspace {
    pub fn address(&self) -> Result<CompanyMemoryAddress> {
        let address = CompanyMemoryAddress::new(
            &self.company_id,
            &self.scope,
            self.private_subject_id.as_deref(),
        )?;
        if address.workspace_id()? != self.workspace_id {
            return Err(inconsistent());
        }
        Ok(address)
    }
}
fn validate_workspace_id(id: &str) -> Result<()> {
    if id.len() != 64
        || !id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(Error::ValidationError(
            "Workspace ID requires 64 lowercase hexadecimal characters".into(),
        ));
    }
    Ok(())
}
fn workspace_key(company: &str, id: &str) -> Vec<u8> {
    let mut key = record_prefix(WORKSPACE, company);
    key.extend_from_slice(id.as_bytes());
    key
}
fn decode_workspace(company: &str, id: &str, bytes: &[u8]) -> Result<CompanyMemoryWorkspace> {
    let record: StoredWorkspace = decode(bytes)?;
    let workspace = record.workspace;
    if record.schema_version != 1 || workspace.company_id != company || workspace.workspace_id != id
    {
        return Err(inconsistent());
    }
    workspace.address().map_err(|_| inconsistent())?;
    Ok(workspace)
}

impl RocksDbMemoryStorage {
    /// The caller must authenticate a company administrator before using this API.
    pub fn company_memory_workspace(
        &self,
        company: &str,
        id: &str,
    ) -> Result<Option<CompanyMemoryWorkspace>> {
        address::valid_id(company)?;
        validate_workspace_id(id)?;
        let snapshot = self.memory_snapshot();
        snapshot.company_workspace(company, id)
    }

    /// A live forward traversal over one company's catalog, including workspaces
    /// whose records are all deleted, expired, rejected or otherwise ineligible.
    pub fn company_memory_workspaces(
        &self,
        company: &str,
        after: Option<&str>,
        limit: usize,
    ) -> Result<CompanyMemoryWorkspacePage> {
        address::valid_id(company)?;
        if let Some(id) = after {
            validate_workspace_id(id)?;
        }
        if !(1..=100).contains(&limit) {
            return Err(Error::ValidationError(
                "Workspace directory limit must be 1–100".into(),
            ));
        }
        let prefix = record_prefix(WORKSPACE, company);
        let start = after
            .map(|id| workspace_key(company, id))
            .unwrap_or_else(|| prefix.clone());
        let snapshot = self.memory_snapshot();
        let mut workspaces = Vec::new();
        let mut more = false;
        for row in snapshot.db.iterator_cf(
            self.cf(super::super::cf::AGENT_META)?,
            rocksdb::IteratorMode::From(&start, rocksdb::Direction::Forward),
        ) {
            let (key, bytes) = row.map_err(storage_error)?;
            if !key.starts_with(&prefix) {
                break;
            }
            let id = std::str::from_utf8(&key[prefix.len()..]).map_err(|_| inconsistent())?;
            if after.is_some_and(|after| id <= after) {
                continue;
            }
            if workspaces.len() == limit {
                more = true;
                break;
            }
            let workspace = decode_workspace(company, id, &bytes)?;
            snapshot.require_workspace_source(&workspace)?;
            workspaces.push(workspace);
        }
        Ok(CompanyMemoryWorkspacePage {
            next_after_workspace_id: if more {
                workspaces
                    .last()
                    .map(|w: &CompanyMemoryWorkspace| w.workspace_id.clone())
            } else {
                None
            },
            workspaces,
        })
    }

    /// Called inside the memory mutation batch, or at startup before writers exist.
    pub(super) fn append_company_workspace(
        &self,
        namespace: &str,
        batch: &mut rocksdb::WriteBatch,
    ) -> Result<()> {
        let Some(address) = CompanyMemoryAddress::from_namespace(namespace)? else {
            return Ok(());
        };
        let workspace = CompanyMemoryWorkspace {
            workspace_id: address.workspace_id()?,
            company_id: address.company_id,
            scope: address.scope,
            private_subject_id: address.private_subject_id,
        };
        let key = workspace_key(&workspace.company_id, &workspace.workspace_id);
        let cf = self.cf(super::super::cf::AGENT_META)?;
        if let Some(bytes) = self.db.get_cf(cf, &key).map_err(storage_error)? {
            if decode_workspace(&workspace.company_id, &workspace.workspace_id, &bytes)?
                != workspace
            {
                return Err(inconsistent());
            }
        } else {
            batch.put_cf(
                cf,
                key,
                encode(&StoredWorkspace {
                    schema_version: 1,
                    workspace,
                })?,
            );
        }
        Ok(())
    }
}
impl snapshot::MemorySnapshot<'_> {
    fn company_workspace(&self, company: &str, id: &str) -> Result<Option<CompanyMemoryWorkspace>> {
        let record = self
            .db
            .get_cf(
                self.storage.cf(super::super::cf::AGENT_META)?,
                workspace_key(company, id),
            )
            .map_err(storage_error)?;
        record
            .map(|bytes| {
                let workspace = decode_workspace(company, id, &bytes)?;
                self.require_workspace_source(&workspace)?;
                Ok(workspace)
            })
            .transpose()
    }
    fn require_workspace_source(&self, workspace: &CompanyMemoryWorkspace) -> Result<()> {
        let prefix = record_prefix(0x10, &workspace.address()?.namespace()?);
        let mut iter = self
            .db
            .raw_iterator_cf(self.storage.cf(super::super::cf::EPISODES)?);
        iter.seek(&prefix);
        iter.status().map_err(storage_error)?;
        if !iter
            .key()
            .is_some_and(|key| key.starts_with(&prefix) && key.len() == prefix.len() + 16)
        {
            return Err(inconsistent());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
