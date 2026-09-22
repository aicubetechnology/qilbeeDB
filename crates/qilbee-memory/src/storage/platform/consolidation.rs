//! Owner-scoped durable consolidation jobs for external inference workers.
use super::snapshot::MemorySnapshot;
use super::*;
use std::collections::BTreeSet;
mod types;
pub use types::*;
mod execution;
mod ledger;
mod reads;
const JOB: u8 = 0x70;
const INTEGRITY: u8 = 0x71;
const HISTORY: u8 = 0x72;
const RECEIPT: u8 = 0x73;
const MAX_JOB_BYTES: usize = 512 * 1024;
const MAX_HISTORY_BYTES: usize = 1024 * 1024;
fn valid(s: &str, max: usize) -> bool {
    !s.trim().is_empty() && s.len() <= max && !s.chars().any(char::is_control)
}
fn hash<T: Serialize>(value: &T) -> Result<String> {
    Ok(digest(&encode(value)?)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}
fn owner_prefix(kind: u8, namespace: &str, owner: &str) -> Result<Vec<u8>> {
    let mut key = record_prefix(kind, namespace);
    key.extend(encode(&owner)?);
    Ok(key)
}
fn job_key(kind: u8, namespace: &str, owner: &str, id: Uuid) -> Result<Vec<u8>> {
    let mut key = owner_prefix(kind, namespace, owner)?;
    key.extend(id.as_bytes());
    Ok(key)
}
fn history_key(namespace: &str, owner: &str, id: Uuid, revision: u64) -> Result<Vec<u8>> {
    let mut key = job_key(HISTORY, namespace, owner, id)?;
    key.extend(revision.to_be_bytes());
    Ok(key)
}
fn conflict(message: &str) -> Error {
    Error::TransactionConflict(message.into())
}
fn check_identity(namespace: &str, owner: &str) -> Result<()> {
    RocksDbMemoryStorage::validate_agent(namespace)?;
    if !valid(owner, 256) {
        return Err(Error::ValidationError("Invalid consolidation owner".into()));
    }
    Ok(())
}
fn lease_active(job: &ConsolidationJob, incarnation: Uuid, now: i64) -> bool {
    job.status == ConsolidationStatus::Running
        && job.attempts.last().is_some_and(|a| {
            a.outcome == ConsolidationOutcome::Running
                && a.storage_incarnation == incarnation
                && now >= a.claimed_at_millis
                && now < a.expires_at_millis
        })
}
#[cfg(test)]
mod tests;
