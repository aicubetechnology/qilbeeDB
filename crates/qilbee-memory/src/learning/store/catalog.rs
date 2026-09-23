//! Company administration over retained learning records, independent of writer grants.
use super::*;
use crate::storage::platform::{CompanyMemoryAddress, MemoryResourceScope, MemoryVisibility};
use serde::Deserialize;

mod origins;
pub use origins::*;
mod evidence;
mod metadata;
mod records;
pub use evidence::*;
pub use metadata::*;
#[cfg(test)]
mod tests;
pub use records::{KnowledgeDetails, LearningResourceDetails, StrategyDetails};

const SCAN_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LearningResourceKind {
    Experience,
    Procedure,
    Knowledge,
    Strategy,
    ToolArtifact,
    ToolDevelopment,
    Policy,
    Context,
    Executor,
}
impl LearningResourceKind {
    fn prefix(self, company: &str) -> Vec<u8> {
        let mut key = vec![match self {
            Self::Experience => 12,
            Self::Procedure => 6,
            Self::Knowledge => 16,
            Self::Strategy => 15,
            Self::ToolArtifact => 8,
            Self::ToolDevelopment => 10,
            Self::Policy => 4,
            Self::Context => 5,
            Self::Executor => 9,
        }];
        append_component(&mut key, company);
        if self == Self::Executor {
            append_component(&mut key, "executors");
        }
        key
    }
    fn scoped(self) -> bool {
        !matches!(self, Self::Policy | Self::Context | Self::Executor)
    }
}

/// A selection returned by the catalog, not an authorization token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LearningResourceRef {
    pub kind: LearningResourceKind,
    pub id: String,
    pub scope: Option<MemoryResourceScope>,
    pub private_subject_id: Option<String>,
}
impl LearningResourceRef {
    fn namespace(&self, company: &str) -> Result<Option<String>> {
        validate_text(&self.id, "resource ID", 512)?;
        match (&self.scope, self.kind.scoped(), &self.private_subject_id) {
            (Some(scope), true, subject) => {
                CompanyMemoryAddress::new(company, scope, subject.as_deref())?
                    .namespace()
                    .map(Some)
            }
            (None, false, None) => Ok(None),
            _ => Err(invalid("The selected resource has an incompatible scope")),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LearningCatalogFilter {
    pub project_id: Option<String>,
    pub agent_id: Option<String>,
    pub mission_id: Option<String>,
    pub visibility: Option<MemoryVisibility>,
    pub private_subject_id: Option<String>,
    /// Case-insensitive substring over the displayed title and resource ID only.
    pub text: Option<String>,
}
impl LearningCatalogFilter {
    fn has_scope(&self) -> bool {
        self.project_id.is_some()
            || self.agent_id.is_some()
            || self.mission_id.is_some()
            || self.visibility.is_some()
            || self.private_subject_id.is_some()
    }
    fn matches(&self, resource: &LearningResourceRef) -> bool {
        let Some(scope) = &resource.scope else {
            return !self.has_scope();
        };
        self.project_id
            .as_ref()
            .is_none_or(|v| v == &scope.project_id)
            && self.agent_id.as_ref().is_none_or(|v| v == &scope.agent_id)
            && self
                .mission_id
                .as_ref()
                .is_none_or(|v| Some(v) == scope.mission_id.as_ref())
            && self.visibility.is_none_or(|v| v == scope.visibility)
            && self
                .private_subject_id
                .as_ref()
                .is_none_or(|v| Some(v) == resource.private_subject_id.as_ref())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LearningCatalogCursor {
    pub version: u32,
    pub company_id: String,
    pub filter_digest: String,
    /// Lowercase hexadecimal storage position; live continuation, not a snapshot.
    pub position: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LearningCatalogQuery {
    pub kind: LearningResourceKind,
    #[serde(default)]
    pub filter: LearningCatalogFilter,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default = "default_scan")]
    pub max_scanned_records: usize,
    pub cursor: Option<LearningCatalogCursor>,
}
fn default_limit() -> usize {
    25
}
fn default_scan() -> usize {
    100
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LearningCatalogEntry {
    pub resource: LearningResourceRef,
    pub title: String,
    pub status: String,
    pub revision: Option<u64>,
    pub recorded_at_millis: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LearningCatalogStop {
    Exhausted,
    EntryLimit,
    ScanLimit,
    ByteLimit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LearningCatalogPage {
    pub entries: Vec<LearningCatalogEntry>,
    pub next_cursor: Option<LearningCatalogCursor>,
    pub stop_reason: LearningCatalogStop,
    pub scanned_records: usize,
    /// Primary catalog keys and values only; excludes dependency verification reads.
    pub scanned_record_bytes: usize,
    pub skipped_non_platform_records: usize,
    pub observed_at_millis: i64,
}

fn invalid(message: &str) -> Error {
    Error::ValidationError(message.into())
}
fn corrupt() -> Error {
    Error::DataCorruption("Learning catalog identity is inconsistent".into())
}
fn component<'a>(bytes: &mut &'a [u8]) -> Result<&'a str> {
    let size = u32::from_be_bytes(
        bytes
            .get(..4)
            .ok_or_else(corrupt)?
            .try_into()
            .map_err(|_| corrupt())?,
    ) as usize;
    *bytes = &bytes[4..];
    let text =
        std::str::from_utf8(bytes.get(..size).ok_or_else(corrupt)?).map_err(|_| corrupt())?;
    *bytes = &bytes[size..];
    Ok(text)
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn unhex(text: &str) -> Result<Vec<u8>> {
    if text.is_empty()
        || text.len() > 12_000
        || text.len() % 2 != 0
        || !text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(invalid("Invalid catalog cursor position"));
    }
    text.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16)
                .map_err(|_| invalid("Invalid catalog cursor position"))
        })
        .collect()
}
fn resource_at(
    kind: LearningResourceKind,
    company: &str,
    prefix: &[u8],
    key: &[u8],
) -> Result<Option<LearningResourceRef>> {
    let mut suffix = key.strip_prefix(prefix).ok_or_else(corrupt)?;
    let address = if kind.scoped() {
        let namespace = component(&mut suffix)?;
        validate_text(namespace, "stored namespace", 4096).map_err(|_| corrupt())?;
        let address = CompanyMemoryAddress::from_namespace(namespace)?;
        if address.as_ref().is_some_and(|a| a.company_id != company) {
            return Err(corrupt());
        }
        address
    } else {
        None
    };
    let id = component(&mut suffix)?.to_string();
    validate_text(&id, "stored resource ID", 512).map_err(|_| corrupt())?;
    if !suffix.is_empty() {
        return Err(corrupt());
    }
    if kind.scoped() && address.is_none() {
        return Ok(None);
    }
    Ok(Some(LearningResourceRef {
        kind,
        id,
        scope: address.as_ref().map(|a| a.scope.clone()),
        private_subject_id: address.and_then(|a| a.private_subject_id),
    }))
}

impl LearningMemory {
    /// Trusted company administration. HTTP must derive company from its current
    /// administrator credential; the library does not authenticate callers.
    pub fn company_learning_catalog(
        &self,
        company: &str,
        query: &LearningCatalogQuery,
    ) -> Result<LearningCatalogPage> {
        self.company_learning_catalog_observed(company, query, |_, _| Ok(()))
    }

    fn company_learning_catalog_observed(
        &self,
        company: &str,
        query: &LearningCatalogQuery,
        mut observe: impl FnMut(&LearningResourceDetails, &LearningCatalogEntry) -> Result<()>,
    ) -> Result<LearningCatalogPage> {
        validate_text(company, "company", 256)?;
        if !(1..=50).contains(&query.limit) || !(1..=1000).contains(&query.max_scanned_records) {
            return Err(invalid("Catalog limit must be 1–50 and scan budget 1–1000"));
        }
        for value in [
            &query.filter.project_id,
            &query.filter.agent_id,
            &query.filter.mission_id,
            &query.filter.private_subject_id,
            &query.filter.text,
        ]
        .into_iter()
        .flatten()
        {
            validate_text(value, "catalog filter", 256)?;
        }
        if !query.kind.scoped() && query.filter.has_scope() {
            return Err(invalid(
                "Company registry resources do not have a workspace scope",
            ));
        }
        let prefix = query.kind.prefix(company);
        let filter_digest = registry::digest(&(query.kind, &query.filter))?;
        let after = query
            .cursor
            .as_ref()
            .map(|cursor| {
                if cursor.version != 1
                    || cursor.company_id != company
                    || cursor.filter_digest != filter_digest
                {
                    return Err(invalid(
                        "Catalog cursor does not match the company, resource kind and filters",
                    ));
                }
                let key = unhex(&cursor.position)?;
                resource_at(query.kind, company, &prefix, &key)
                    .map_err(|_| invalid("Invalid catalog cursor position"))?;
                Ok(key)
            })
            .transpose()?;
        let _guard = self
            .inner
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Learning mutation lock poisoned".into()))?;
        let mut options = rocksdb::ReadOptions::default();
        options.set_iterate_lower_bound(prefix.clone());
        let mut upper = prefix.clone();
        while upper.last() == Some(&255) {
            upper.pop();
        }
        *upper.last_mut().ok_or_else(corrupt)? += 1;
        options.set_iterate_upper_bound(upper);
        let mut iterator = self.inner.db.raw_iterator_opt(options);
        iterator.seek(after.as_deref().unwrap_or(&prefix));
        if iterator
            .key()
            .is_some_and(|key| Some(key) == after.as_deref())
        {
            iterator.next();
        }
        let mut page = LearningCatalogPage {
            entries: vec![],
            next_cursor: None,
            stop_reason: LearningCatalogStop::Exhausted,
            scanned_records: 0,
            scanned_record_bytes: 0,
            skipped_non_platform_records: 0,
            observed_at_millis: chrono::Utc::now().timestamp_millis(),
        };
        let text = query.filter.text.as_ref().map(|v| v.to_lowercase());
        let mut last = None;
        while let Some(key) = iterator.key() {
            if !key.starts_with(&prefix) {
                return Err(corrupt());
            }
            let stop = if page.entries.len() == query.limit {
                Some(LearningCatalogStop::EntryLimit)
            } else if page.scanned_records == query.max_scanned_records {
                Some(LearningCatalogStop::ScanLimit)
            } else {
                None
            };
            if let Some(stop) = stop {
                page.stop_reason = stop;
                break;
            }
            let value = iterator.value().ok_or_else(corrupt)?;
            let bytes = key.len().checked_add(value.len()).ok_or_else(corrupt)?;
            if bytes > SCAN_BYTES {
                return Err(Error::DataCorruption(
                    "Learning catalog record exceeds its 4 MiB inspection ceiling".into(),
                ));
            }
            if page.scanned_record_bytes + bytes > SCAN_BYTES {
                page.stop_reason = LearningCatalogStop::ByteLimit;
                break;
            }
            let resource = resource_at(query.kind, company, &prefix, key)?;
            page.scanned_records += 1;
            page.scanned_record_bytes += bytes;
            match resource {
                Some(resource) if query.filter.matches(&resource) => {
                    // Version 1 enumerates ordinary resources only. The reader
                    // verifies combined evidence before returning this conflict;
                    // corrupt mandatory origins remain errors, never skipped.
                    let details = match self.catalog_details(company, &resource) {
                        Err(Error::UnsupportedKnowledgeOrigin) => None,
                        result => Some(result?.ok_or_else(corrupt)?),
                    };
                    if let Some(details) = details {
                        let entry = self.catalog_summary(company, &details, resource)?;
                        if text.as_ref().is_none_or(|v| {
                            entry.title.to_lowercase().contains(v)
                                || entry.resource.id.to_lowercase().contains(v)
                        }) {
                            observe(&details, &entry)?;
                            page.entries.push(entry);
                        }
                    }
                }
                None => page.skipped_non_platform_records += 1,
                _ => (),
            }
            last = Some(key.to_vec());
            iterator.next();
        }
        iterator.status().map_err(storage_error)?;
        if page.stop_reason != LearningCatalogStop::Exhausted {
            page.next_cursor = Some(LearningCatalogCursor {
                version: 1,
                company_id: company.into(),
                filter_digest,
                position: hex(&last.ok_or_else(corrupt)?),
            });
        }
        Ok(page)
    }

    /// Read current verified details for a selection, with no delegated credential.
    pub fn inspect_company_learning_resource(
        &self,
        company: &str,
        resource: &LearningResourceRef,
    ) -> Result<Option<LearningResourceDetails>> {
        validate_text(company, "company", 256)?;
        resource.namespace(company)?;
        let _guard = self
            .inner
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Learning mutation lock poisoned".into()))?;
        self.catalog_details(company, resource)
    }
}

impl LearningMemory {
    pub fn inspect_company_knowledge(
        &self,
        memory: &crate::RocksDbMemoryStorage,
        company: &str,
        resource: &LearningResourceRef,
    ) -> Result<Option<super::knowledge::KnowledgeInspection>> {
        if resource.kind != LearningResourceKind::Knowledge {
            return Err(invalid("A knowledge resource reference is required"));
        }
        let namespace = resource
            .namespace(company)?
            .ok_or_else(|| invalid("Knowledge scope is required"))?;
        self.inspect_knowledge(memory, company, &namespace, &resource.id)
    }
}

impl LearningMemory {
    /// Versioned company inspection preserves minimal origin disclosure and
    /// derives the namespace exclusively from the authenticated company.
    pub fn inspect_company_knowledge_with_origin(
        &self,
        memory: &crate::RocksDbMemoryStorage,
        company: &str,
        resource: &LearningResourceRef,
    ) -> Result<
        Option<(
            super::knowledge::KnowledgeInspection,
            super::knowledge_origin::KnowledgeOriginDescriptor,
        )>,
    > {
        if resource.kind != LearningResourceKind::Knowledge {
            return Err(invalid("A knowledge resource reference is required"));
        }
        let namespace = resource
            .namespace(company)?
            .ok_or_else(|| invalid("Knowledge scope is required"))?;
        self.inspect_knowledge_with_origin(memory, company, &namespace, &resource.id)
    }
}

impl LearningMemory {
    /// Contract-v2 details expose only a verified origin descriptor. Generic
    /// procedures remain visible with null origin; reserved markers never do.
    pub fn inspect_company_learning_resource_with_origin(
        &self,
        company: &str,
        resource: &LearningResourceRef,
    ) -> Result<
        Option<(
            LearningResourceDetails,
            Option<super::knowledge_origin::KnowledgeOriginDescriptor>,
        )>,
    > {
        validate_text(company, "company", 256)?;
        let namespace = resource.namespace(company)?;
        let _guard = self
            .inner
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Learning mutation lock poisoned".into()))?;
        if !matches!(
            resource.kind,
            LearningResourceKind::Knowledge | LearningResourceKind::Procedure
        ) {
            return Ok(self
                .catalog_details(company, resource)?
                .map(|details| (details, None)));
        }
        let namespace =
            namespace.ok_or_else(|| invalid("Knowledge or procedure scope is required"))?;
        let binding = self.registered_procedure(company, &namespace, &resource.id)?;
        let receipt = self.knowledge_receipt_any_origin(company, &namespace, &resource.id)?;
        if let Some(receipt) = receipt {
            let origin = super::knowledge_origin::descriptor_from_verified_receipt(&receipt)?;
            let binding = binding
                .ok_or_else(|| Error::DataCorruption("Knowledge procedure is missing".into()))?;
            let details = if resource.kind == LearningResourceKind::Knowledge {
                LearningResourceDetails::Knowledge(KnowledgeDetails {
                    receipt,
                    procedure: binding,
                })
            } else {
                LearningResourceDetails::Procedure(binding)
            };
            return Ok(Some((details, Some(origin))));
        }
        if let Some(binding) = binding {
            let sources = &binding.record.proposal.source_refs;
            if super::knowledge::has_knowledge_binding_marker(sources)
                || super::knowledge_origin::has_origin_marker(sources)
            {
                return Err(Error::DataCorruption(
                    "Mandatory knowledge binding is missing".into(),
                ));
            }
            if resource.kind == LearningResourceKind::Procedure {
                return Ok(Some((LearningResourceDetails::Procedure(binding), None)));
            }
        }
        Ok(None)
    }
}
