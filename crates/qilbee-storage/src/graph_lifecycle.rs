//! Durable named graph generations and allocation state.
use super::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

#[path = "graph_lifecycle/allocation.rs"]
mod allocation;
#[path = "graph_lifecycle/catalog.rs"]
mod catalog;

const CATALOG: &str = "graphs";
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphIdentity {
    schema_version: u32,
    name: String,
    id: GraphId,
    active: bool,
}
impl GraphIdentity {
    pub fn id(&self) -> GraphId {
        self.id
    }
    pub fn name(&self) -> &str {
        &self.name
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalog {
    schema_version: u32,
    graphs: BTreeMap<String, GraphId>,
}
fn identity_key(id: GraphId) -> String {
    format!("graph/v2/identity/{:016x}", id.as_internal())
}
fn invalid() -> Error {
    Error::DataCorruption("Inconsistent durable graph metadata".into())
}
fn inactive() -> Error {
    Error::InvalidGraphOperation("The graph generation is no longer active".into())
}
fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    serde_json::to_vec(value).map_err(|e| Error::Serialization(e.to_string()))
}
fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    serde_json::from_slice(bytes).map_err(|_| invalid())
}
fn validate_name(name: &str) -> Result<()> {
    if name.trim().is_empty() || name.len() > 1024 || name.chars().any(char::is_control) {
        return Err(Error::ValidationError(
            "Graph names require 1–1024 UTF-8 bytes without control characters".into(),
        ));
    }
    Ok(())
}
impl StorageEngine {
    fn stage_meta(&self, batch: &mut WriteBatch, key: &str, value: &[u8]) -> Result<()> {
        let cf = self.cf(cf::META)?;
        batch.put_cf(cf, KeyBuilder::meta(key), value);
        batch.put_cf(cf, ordered_meta_key(key), []);
        Ok(())
    }
    fn sync_graph_batch(&self, batch: WriteBatch) -> Result<()> {
        let mut options = rocksdb::WriteOptions::default();
        options.disable_wal(false);
        options.set_sync(true);
        self.db
            .write_opt(batch, &options)
            .map_err(|e| Error::Storage(e.to_string()))
    }
    fn stored_graph_identity(&self, id: GraphId) -> Result<Option<GraphIdentity>> {
        self.get_meta(&identity_key(id))?
            .map(|bytes| {
                let value: GraphIdentity = decode(&bytes)?;
                if value.schema_version != 1 || value.id != id {
                    return Err(invalid());
                }
                validate_name(&value.name).map_err(|_| invalid())?;
                Ok(value)
            })
            .transpose()
    }
    /// Check a previously obtained named graph handle. Already started reads may
    /// finish during deletion; mutations repeat this check under the writer lock.
    pub fn validate_graph_identity(&self, identity: &GraphIdentity) -> Result<()> {
        match self.stored_graph_identity(identity.id)? {
            Some(current) if current == *identity && current.active => Ok(()),
            Some(current) if current.name != identity.name => Err(invalid()),
            _ => Err(inactive()),
        }
    }
}

#[cfg(test)]
#[path = "graph_lifecycle/tests.rs"]
mod tests;
