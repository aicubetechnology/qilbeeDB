//! Provenance-bound derived memories and request-local dependency work accounting.
use super::snapshot::MemorySnapshot;
use super::*;
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_MEMORY_SOURCES: usize = 16;
pub const MAX_DEPENDENCY_RECORDS: usize = 4096;
pub const MAX_DEPENDENCY_BYTES: usize = 16 * 1024 * 1024;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemorySourceRef {
    pub record_id: Uuid,
    pub revision: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryDerivation {
    pub sources: Vec<MemorySourceRef>,
    pub method: String,
    pub method_revision: String,
    pub evidence_ref: String,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DependencyWork {
    pub records_examined: usize,
    pub bytes_examined: usize,
}
#[derive(Default)]
pub(super) struct DependencyState {
    pub work: DependencyWork,
    pub cache: BTreeMap<(String, Uuid), Option<DependencyRecord>>,
    pub limits: Option<(usize, usize)>,
    pub exhausted: Option<DependencyBudgetStop>,
    pub lookahead_bytes: usize,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DependencyBudgetStop {
    Records,
    Bytes,
}
#[derive(Clone)]
pub(super) struct DependencyRecord {
    pub record_bytes: usize,
    pub revision: u64,
    pub reason: MemoryEligibilityReason,
    pub derivation: Option<MemoryDerivation>,
}
impl MemoryDerivation {
    pub(super) fn validate(&self) -> Result<()> {
        let ids: BTreeSet<_> = self.sources.iter().map(|r| r.record_id).collect();
        let valid = |s: &str, max: usize| {
            !s.trim().is_empty() && s.len() <= max && !s.chars().any(char::is_control)
        };
        if self.sources.is_empty()
            || self.sources.len() > MAX_MEMORY_SOURCES
            || ids.len() != self.sources.len()
            || self.sources.iter().any(|r| r.revision == 0)
            || !valid(&self.method, 256)
            || !valid(&self.method_revision, 256)
            || !valid(&self.evidence_ref, 2048)
        {
            return Err(Error::ValidationError("Derived memory requires 1-16 unique positive source revisions and method/evidence identities".into()));
        }
        Ok(())
    }
}
impl MemorySnapshot<'_> {
    pub(super) fn dependency_work(&self) -> DependencyWork {
        self.dependencies.borrow().work.clone()
    }
    pub(super) fn dependency(&self, namespace: &str, id: Uuid) -> Result<Option<DependencyRecord>> {
        let key = (namespace.to_owned(), id);
        if let Some(record) = self.dependencies.borrow().cache.get(&key) {
            return Ok(record.clone());
        }
        if self.dependencies.borrow().limits.is_some() {
            return self.bounded_dependency(namespace, id, key);
        }
        if self.dependencies.borrow().work.records_examined >= MAX_DEPENDENCY_RECORDS {
            return Err(Error::ValidationError(
                "Dependency record budget exhausted; reduce the candidate scan limit".into(),
            ));
        }
        let bytes = self
            .db
            .get_cf(
                self.storage.cf(super::super::cf::EPISODES)?,
                record_key(0x10, namespace, id),
            )
            .map_err(storage_error)?;
        let len = bytes.as_ref().map_or(0, Vec::len);
        if self
            .dependencies
            .borrow()
            .work
            .bytes_examined
            .saturating_add(len)
            > MAX_DEPENDENCY_BYTES
        {
            return Err(Error::ValidationError(
                "Dependency byte budget exhausted; reduce the candidate scan limit".into(),
            ));
        }
        let index = self
            .db
            .get_cf(
                self.storage.cf(super::super::cf::EPISODE_INDEX)?,
                record_key(0x11, namespace, id),
            )
            .map_err(storage_error)?;
        let record = decode_record_pair(id, bytes, index)?.map(|r| DependencyRecord {
            record_bytes: len,
            revision: r.revision,
            reason: super::eligibility::record_reason(&r, self.now),
            derivation: r.derivation,
        });
        let mut state = self.dependencies.borrow_mut();
        state.work.records_examined += 1;
        state.work.bytes_examined += len;
        state.cache.insert(key, record.clone());
        Ok(record)
    }
}

impl MemorySnapshot<'_> {
    fn bounded_dependency(
        &self,
        namespace: &str,
        id: Uuid,
        cache_key: (String, Uuid),
    ) -> Result<Option<DependencyRecord>> {
        let (record_limit, byte_limit) = self
            .dependencies
            .borrow()
            .limits
            .ok_or_else(|| Error::Internal("Missing bounded dependency limits".into()))?;
        {
            let mut state = self.dependencies.borrow_mut();
            if state.exhausted.is_some() {
                return Err(Error::ValidationError(
                    "Dependency inspection budget exhausted".into(),
                ));
            }
            if state.work.records_examined >= record_limit {
                state.exhausted = Some(DependencyBudgetStop::Records);
                return Err(Error::ValidationError(
                    "Dependency record budget exhausted".into(),
                ));
            }
            // Missing lookups and byte-blocked lookups still consume a record attempt.
            state.work.records_examined += 1;
        }
        let bytes = self
            .db
            .get_cf(
                self.storage.cf(super::super::cf::EPISODES)?,
                record_key(0x10, namespace, id),
            )
            .map_err(storage_error)?;
        let len = bytes.as_ref().map_or(0, Vec::len);
        self.admit_dependency_bytes(len, byte_limit)?;
        let index = self
            .db
            .get_cf(
                self.storage.cf(super::super::cf::EPISODE_INDEX)?,
                record_key(0x11, namespace, id),
            )
            .map_err(storage_error)?;
        self.admit_dependency_bytes(index.as_ref().map_or(0, Vec::len), byte_limit)?;
        let record = decode_record_pair(id, bytes, index)?.map(|r| DependencyRecord {
            record_bytes: len,
            revision: r.revision,
            reason: super::eligibility::record_reason(&r, self.now),
            derivation: r.derivation,
        });
        self.dependencies
            .borrow_mut()
            .cache
            .insert(cache_key, record.clone());
        Ok(record)
    }

    fn admit_dependency_bytes(&self, len: usize, limit: usize) -> Result<()> {
        let mut state = self.dependencies.borrow_mut();
        if len > limit.saturating_sub(state.work.bytes_examined) {
            state.exhausted = Some(DependencyBudgetStop::Bytes);
            state.lookahead_bytes = len;
            return Err(Error::ValidationError(
                "Dependency byte budget exhausted".into(),
            ));
        }
        state.work.bytes_examined += len;
        Ok(())
    }
}
