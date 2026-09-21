//! Bounded administrative inspection with current records and explicit eligibility.
use super::*;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompanyMemoryView {
    Current,
    #[default]
    Retained,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyMemoryQuery {
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default = "default_scan_limit")]
    pub scan_limit: usize,
    pub after: Option<Uuid>,
    #[serde(default)]
    pub view: CompanyMemoryView,
    pub text_contains: Option<String>,
    pub tag: Option<String>,
    pub episode_type: Option<EpisodeType>,
}
fn default_limit() -> usize {
    25
}
fn default_scan_limit() -> usize {
    500
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompanyInventoryStopReason {
    Exhausted,
    RecordLimit,
    ScanLimit,
    ByteLimit,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyMemoryEntry {
    pub record: MemoryRecord,
    pub eligibility: MemoryEligibility,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyMemoryInventoryPage {
    pub entries: Vec<CompanyMemoryEntry>,
    pub next_after: Option<Uuid>,
    pub stop_reason: CompanyInventoryStopReason,
    pub evaluated_at_millis: i64,
    pub scanned_records: usize,
    pub record_bytes: usize,
    pub dependency_work: DependencyWork,
}
#[derive(Debug, Clone)]
pub struct CompanyMemoryInventory {
    pub workspace: CompanyMemoryWorkspace,
    pub page: CompanyMemoryInventoryPage,
}
#[derive(Debug, Clone)]
pub struct CompanyMemoryInspection {
    pub workspace: CompanyMemoryWorkspace,
    pub entry: CompanyMemoryEntry,
    pub record_bytes: usize,
}
#[derive(Debug, Clone)]
pub struct CompanyMemoryGraph {
    pub workspace: CompanyMemoryWorkspace,
    pub graph: MemoryEvidenceGraph,
}
#[derive(Debug, Clone)]
pub struct CompanyTypedMemoryGraph {
    pub workspace: CompanyMemoryWorkspace,
    pub graph: TypedMemoryGraph,
}
#[derive(Debug, Clone)]
pub struct CompanyMemoryRelationInspection {
    pub workspace: CompanyMemoryWorkspace,
    pub inspection: MemoryRelationInspection,
}
#[derive(Debug, Clone)]
pub struct CompanyMemoryRelationRevision {
    pub workspace: CompanyMemoryWorkspace,
    pub history: MemoryRelationRevision,
}
impl CompanyMemoryQuery {
    fn validate(&self) -> Result<()> {
        if !(1..=100).contains(&self.limit) || !(1..=10_000).contains(&self.scan_limit) {
            return Err(Error::ValidationError(
                "Company inventory limit must be 1–100 and scan limit 1–10000".into(),
            ));
        }
        if self
            .text_contains
            .as_ref()
            .is_some_and(|text| text.len() > 4096)
            || self.tag.as_ref().is_some_and(|tag| tag.len() > 256)
        {
            return Err(Error::ValidationError(
                "Inventory text filter exceeds 4096 bytes or tag exceeds 256 bytes".into(),
            ));
        }
        Ok(())
    }
    fn matches(&self, record: &MemoryRecord, needle: Option<&str>) -> bool {
        let Some(payload) = &record.payload else {
            return self.text_contains.is_none()
                && self.tag.is_none()
                && self.episode_type.is_none();
        };
        self.episode_type
            .as_ref()
            .is_none_or(|kind| kind == &payload.episode_type)
            && self
                .tag
                .as_ref()
                .is_none_or(|tag| payload.tags.contains(tag))
            && needle.is_none_or(|needle| {
                [
                    payload.content.primary.as_str(),
                    payload.content.secondary.as_deref().unwrap_or(""),
                    payload.content.context.as_deref().unwrap_or(""),
                ]
                .iter()
                .any(|value| value.to_lowercase().contains(needle))
            })
    }
}
impl RocksDbMemoryStorage {
    /// The caller authenticates a company administrator; workspace IDs confer no authority.
    pub fn read_company_memory_typed_graph(
        &self,
        company: &str,
        workspace_id: &str,
        query: &TypedMemoryGraphQuery,
    ) -> Result<Option<CompanyTypedMemoryGraph>> {
        address::valid_id(company)?;
        validate_workspace_id(workspace_id)?;
        query.validate()?;
        let snapshot = self.memory_snapshot();
        let Some(workspace) = snapshot.company_workspace(company, workspace_id)? else {
            return Ok(None);
        };
        let graph = snapshot.typed_graph(&workspace.address()?.namespace()?, query)?;
        Ok(Some(CompanyTypedMemoryGraph { workspace, graph }))
    }
    /// Retained assertions remain visible to the authorized company administrator.
    pub fn inspect_company_memory_relation(
        &self,
        company: &str,
        workspace_id: &str,
        relation_id: Uuid,
    ) -> Result<Option<CompanyMemoryRelationInspection>> {
        address::valid_id(company)?;
        validate_workspace_id(workspace_id)?;
        let snapshot = self.memory_snapshot();
        let Some(workspace) = snapshot.company_workspace(company, workspace_id)? else {
            return Ok(None);
        };
        let namespace = workspace.address()?.namespace()?;
        let Some(relation) = snapshot.relation(&namespace, relation_id)? else {
            return Ok(None);
        };
        let eligibility = snapshot.relation_eligibility(&namespace, &relation)?;
        Ok(Some(CompanyMemoryRelationInspection {
            workspace,
            inspection: MemoryRelationInspection {
                relation,
                eligibility,
            },
        }))
    }
    /// Historical relation metadata uses the same company boundary as retained memory.
    pub fn company_memory_relation_revision(
        &self,
        company: &str,
        workspace_id: &str,
        relation_id: Uuid,
        revision: u64,
    ) -> Result<Option<CompanyMemoryRelationRevision>> {
        address::valid_id(company)?;
        validate_workspace_id(workspace_id)?;
        if revision == 0 {
            return Err(Error::ValidationError(
                "Relation revision must be positive".into(),
            ));
        }
        let snapshot = self.memory_snapshot();
        let Some(workspace) = snapshot.company_workspace(company, workspace_id)? else {
            return Ok(None);
        };
        let Some(history) = snapshot.relation_revision(
            &workspace.address()?.namespace()?,
            relation_id,
            revision,
        )?
        else {
            return Ok(None);
        };
        Ok(Some(CompanyMemoryRelationRevision { workspace, history }))
    }
    /// Resolve the company-owned workspace and eligible evidence in one snapshot.
    /// The caller must authenticate a company administrator first.
    pub fn read_company_memory_graph(
        &self,
        company: &str,
        workspace_id: &str,
        query: &MemoryGraphQuery,
    ) -> Result<Option<CompanyMemoryGraph>> {
        address::valid_id(company)?;
        validate_workspace_id(workspace_id)?;
        query.validate()?;
        let snapshot = self.memory_snapshot();
        let Some(workspace) = snapshot.company_workspace(company, workspace_id)? else {
            return Ok(None);
        };
        let graph = snapshot.evidence_graph(&workspace.address()?.namespace()?, query)?;
        Ok(Some(CompanyMemoryGraph { workspace, graph }))
    }
    /// Administrative access is deliberately separate from agent retrieval. The
    /// caller authenticates a company administrator; a workspace ID is not a grant.
    pub fn query_company_memory(
        &self,
        company: &str,
        workspace_id: &str,
        query: &CompanyMemoryQuery,
    ) -> Result<Option<CompanyMemoryInventory>> {
        address::valid_id(company)?;
        validate_workspace_id(workspace_id)?;
        query.validate()?;
        let snapshot = self.memory_snapshot();
        let Some(workspace) = snapshot.company_workspace(company, workspace_id)? else {
            return Ok(None);
        };
        let namespace = workspace.address()?.namespace()?;
        let prefix = record_prefix(0x10, &namespace);
        let start = query
            .after
            .map(|id| record_key(0x10, &namespace, id))
            .unwrap_or_else(|| prefix.clone());
        let mut page = CompanyMemoryInventoryPage {
            entries: vec![],
            next_after: None,
            stop_reason: CompanyInventoryStopReason::Exhausted,
            evaluated_at_millis: snapshot.now,
            scanned_records: 0,
            record_bytes: 0,
            dependency_work: DependencyWork::default(),
        };
        let needle = query.text_contains.as_ref().map(|text| text.to_lowercase());
        let mut last_examined = None;
        for row in snapshot.db.iterator_cf(
            self.cf(super::super::super::cf::EPISODES)?,
            rocksdb::IteratorMode::From(&start, rocksdb::Direction::Forward),
        ) {
            let (key, bytes) = row.map_err(storage_error)?;
            if !key.starts_with(&prefix) {
                break;
            }
            let id = Uuid::from_slice(&key[prefix.len()..]).map_err(|_| inconsistent())?;
            if query.after.is_some_and(|after| id <= after) {
                continue;
            }
            let stop = if page.entries.len() == query.limit {
                Some(CompanyInventoryStopReason::RecordLimit)
            } else if page.scanned_records == query.scan_limit {
                Some(CompanyInventoryStopReason::ScanLimit)
            } else if page.record_bytes.saturating_add(bytes.len()) > MAX_MEMORY_READ_BYTES {
                Some(CompanyInventoryStopReason::ByteLimit)
            } else {
                None
            };
            if let Some(reason) = stop {
                if last_examined.is_none() {
                    return Err(Error::ValidationError(
                        "A retained record exceeds the 8 MiB inventory byte budget".into(),
                    ));
                }
                page.next_after = last_examined;
                page.stop_reason = reason;
                break;
            }
            page.record_bytes += bytes.len();
            page.scanned_records += 1;
            last_examined = Some(id);
            let index = snapshot
                .db
                .get_cf(
                    self.cf(super::super::super::cf::EPISODE_INDEX)?,
                    record_key(0x11, &namespace, id),
                )
                .map_err(storage_error)?;
            let record =
                decode_record_pair(id, Some(bytes.to_vec()), index)?.ok_or_else(inconsistent)?;
            if !query.matches(&record, needle.as_deref()) {
                continue;
            }
            let entry = snapshot.company_entry(&namespace, record)?;
            if query.view == CompanyMemoryView::Retained || entry.eligibility.eligible {
                page.entries.push(entry);
            }
        }
        page.dependency_work = snapshot.dependency_work();
        Ok(Some(CompanyMemoryInventory { workspace, page }))
    }

    pub fn inspect_company_memory(
        &self,
        company: &str,
        workspace_id: &str,
        id: Uuid,
    ) -> Result<Option<CompanyMemoryInspection>> {
        address::valid_id(company)?;
        validate_workspace_id(workspace_id)?;
        let snapshot = self.memory_snapshot();
        let Some(workspace) = snapshot.company_workspace(company, workspace_id)? else {
            return Ok(None);
        };
        let namespace = workspace.address()?.namespace()?;
        let bytes = snapshot
            .db
            .get_cf(
                self.cf(super::super::super::cf::EPISODES)?,
                record_key(0x10, &namespace, id),
            )
            .map_err(storage_error)?;
        let record_bytes = bytes.as_ref().map_or(0, Vec::len);
        if record_bytes > MAX_MEMORY_READ_BYTES {
            return Err(Error::ValidationError(
                "A retained record exceeds the 8 MiB inventory byte budget".into(),
            ));
        }
        let index = snapshot
            .db
            .get_cf(
                self.cf(super::super::super::cf::EPISODE_INDEX)?,
                record_key(0x11, &namespace, id),
            )
            .map_err(storage_error)?;
        let Some(record) = decode_record_pair(id, bytes, index)? else {
            return Ok(None);
        };
        Ok(Some(CompanyMemoryInspection {
            workspace,
            entry: snapshot.company_entry(&namespace, record)?,
            record_bytes,
        }))
    }
}
impl snapshot::MemorySnapshot<'_> {
    fn company_entry(&self, namespace: &str, record: MemoryRecord) -> Result<CompanyMemoryEntry> {
        let before = self.dependency_work();
        let mut eligibility = self.explain_eligibility(namespace, &record)?;
        // Entries report their own incremental dependency reads. The page reports
        // the aggregate budget; shared cached dependencies are not counted twice.
        eligibility.dependency_work.records_examined -= before.records_examined;
        eligibility.dependency_work.bytes_examined -= before.bytes_examined;
        Ok(CompanyMemoryEntry {
            record,
            eligibility,
        })
    }
}
