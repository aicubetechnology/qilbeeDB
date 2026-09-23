//! Bounded seek pagination; each request rechecks current records and eligibility.
use super::*;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryDateOrder {
    #[default]
    CreatedDesc,
    CreatedAsc,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrderedMemoryQuery {
    #[serde(default)]
    pub order: MemoryDateOrder,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default = "default_scan")]
    pub scan_limit: usize,
    pub cursor: Option<String>,
    pub text_contains: Option<String>,
    pub tag: Option<String>,
    pub episode_type: Option<EpisodeType>,
}
fn default_limit() -> usize {
    40
}
fn default_scan() -> usize {
    1000
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyMemorySelection {
    pub workspace_id: Option<String>,
    pub project_id: Option<String>,
    pub agent_id: Option<String>,
    pub visibility: Option<MemoryVisibility>,
    pub private_subject_id: Option<String>,
}
impl CompanyMemorySelection {
    fn validate(&self) -> Result<()> {
        for value in [&self.project_id, &self.agent_id, &self.private_subject_id]
            .into_iter()
            .flatten()
        {
            if value.trim().is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
                return Err(invalid("Invalid company memory selection"));
            }
        }
        if self.workspace_id.as_ref().is_some_and(|id| {
            id.len() != 64
                || !id
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        }) {
            return Err(invalid("Invalid workspace ID"));
        }
        Ok(())
    }
    fn matches(&self, workspace: &CompanyMemoryWorkspace) -> bool {
        self.workspace_id
            .as_ref()
            .is_none_or(|x| x == &workspace.workspace_id)
            && self
                .project_id
                .as_ref()
                .is_none_or(|x| x == &workspace.scope.project_id)
            && self
                .agent_id
                .as_ref()
                .is_none_or(|x| x == &workspace.scope.agent_id)
            && self
                .visibility
                .is_none_or(|x| x == workspace.scope.visibility)
            && self
                .private_subject_id
                .as_ref()
                .is_none_or(|x| workspace.private_subject_id.as_ref() == Some(x))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrderedCompanyMemoryEntry {
    pub workspace: CompanyMemoryWorkspace,
    pub record: MemoryRecord,
    pub eligibility: MemoryEligibility,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrderedMemoryPage {
    pub records: Vec<MemoryRecord>,
    pub next_cursor: Option<String>,
    pub stop_reason: CompanyInventoryStopReason,
    pub evaluated_at_millis: i64,
    pub scanned_records: usize,
    pub record_bytes: usize,
    pub dependency_work: DependencyWork,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrderedCompanyMemoryPage {
    pub entries: Vec<OrderedCompanyMemoryEntry>,
    pub next_cursor: Option<String>,
    pub stop_reason: CompanyInventoryStopReason,
    pub evaluated_at_millis: i64,
    pub scanned_records: usize,
    pub record_bytes: usize,
    pub dependency_work: DependencyWork,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    version: u32,
    binding: String,
    position: String,
}
fn invalid(message: &str) -> Error {
    Error::ValidationError(message.into())
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn unhex(text: &str) -> Result<Vec<u8>> {
    if text.len() % 2 != 0
        || !text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(invalid(
            "Invalid ordered memory cursor; restart the listing",
        ));
    }
    text.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16)
                .map_err(|_| invalid("Invalid ordered memory cursor"))
        })
        .collect()
}

enum Boundary<'a> {
    Scope(&'a str),
    Company(&'a str, &'a CompanyMemorySelection),
}

impl RocksDbMemoryStorage {
    /// Caller provides the exact namespace authorized for MemoryRead.
    pub fn query_ordered_memory(
        &self,
        namespace: &str,
        query: &OrderedMemoryQuery,
    ) -> Result<OrderedMemoryPage> {
        Self::validate_agent(namespace)?;
        let page = self.ordered_inventory(
            Boundary::Scope(namespace),
            CompanyMemoryView::Current,
            query,
        )?;
        Ok(OrderedMemoryPage {
            records: page.entries.into_iter().map(|e| e.record).collect(),
            next_cursor: page.next_cursor,
            stop_reason: page.stop_reason,
            evaluated_at_millis: page.evaluated_at_millis,
            scanned_records: page.scanned_records,
            record_bytes: page.record_bytes,
            dependency_work: page.dependency_work,
        })
    }

    /// Caller authenticates a company administrator. Selection never grants access.
    pub fn query_ordered_company_memory(
        &self,
        company: &str,
        selection: &CompanyMemorySelection,
        view: CompanyMemoryView,
        query: &OrderedMemoryQuery,
    ) -> Result<OrderedCompanyMemoryPage> {
        if company.trim().is_empty() || company.len() > 256 || company.chars().any(char::is_control)
        {
            return Err(invalid("Invalid company ID"));
        }
        selection.validate()?;
        self.ordered_inventory(Boundary::Company(company, selection), view, query)
    }

    fn ordered_inventory(
        &self,
        boundary: Boundary<'_>,
        view: CompanyMemoryView,
        query: &OrderedMemoryQuery,
    ) -> Result<OrderedCompanyMemoryPage> {
        let filter = CompanyMemoryQuery {
            limit: query.limit,
            scan_limit: query.scan_limit,
            after: None,
            view,
            text_contains: query.text_contains.clone(),
            tag: query.tag.clone(),
            episode_type: query.episode_type.clone(),
        };
        filter.validate()?;
        let (prefix, suffix_len, authority) = match &boundary {
            Boundary::Scope(namespace) => (
                record_prefix(SCOPE_ROW, namespace),
                24,
                encode(&("scope", namespace))?,
            ),
            Boundary::Company(company, selection) => (
                record_prefix(COMPANY_ROW, company),
                88,
                encode(&("company", company, selection))?,
            ),
        };
        let binding = hex(&digest(&encode(&(
            authority,
            view,
            query.order,
            &query.text_contains,
            &query.tag,
            &query.episode_type,
        ))?));
        let after = query
            .cursor
            .as_ref()
            .map(|cursor| -> Result<Vec<u8>> {
                if cursor.len() > 1024 {
                    return Err(invalid("Ordered memory cursor exceeds 1024 bytes"));
                }
                let bytes = unhex(cursor)?;
                let cursor: Cursor = serde_json::from_slice(&bytes)
                    .map_err(|_| invalid("Invalid ordered memory cursor; restart the listing"))?;
                if cursor.version != 1 || cursor.binding != binding {
                    return Err(invalid(
                        "Ordered memory cursor does not match this listing; restart the listing",
                    ));
                }
                let position = unhex(&cursor.position)?;
                if position.len() != suffix_len {
                    return Err(invalid("Invalid ordered memory cursor position"));
                }
                let mut key = prefix.clone();
                key.extend(position);
                Ok(key)
            })
            .transpose()?;
        let descending = matches!(query.order, MemoryDateOrder::CreatedDesc);
        let start = after.clone().unwrap_or_else(|| {
            if descending {
                successor(prefix.clone())
            } else {
                prefix.clone()
            }
        });
        let direction = if descending {
            rocksdb::Direction::Reverse
        } else {
            rocksdb::Direction::Forward
        };
        let snapshot = self.memory_snapshot();
        let mut page = OrderedCompanyMemoryPage {
            entries: vec![],
            next_cursor: None,
            stop_reason: CompanyInventoryStopReason::Exhausted,
            evaluated_at_millis: snapshot.now,
            scanned_records: 0,
            record_bytes: 0,
            dependency_work: DependencyWork::default(),
        };
        let needle = query.text_contains.as_ref().map(|s| s.to_lowercase());
        let mut last: Option<Vec<u8>> = None;
        for row in snapshot.db.iterator_cf(
            self.cf(super::super::super::cf::EPISODE_INDEX)?,
            rocksdb::IteratorMode::From(&start, direction),
        ) {
            let (key, value) = row.map_err(storage_error)?;
            if !key.starts_with(&prefix) {
                break;
            }
            if after.as_ref().is_some_and(|after| {
                if descending {
                    key.as_ref() >= after.as_slice()
                } else {
                    key.as_ref() <= after.as_slice()
                }
            }) {
                continue;
            }
            if key.len() != prefix.len() + suffix_len {
                return Err(inconsistent());
            }
            let stop = if page.entries.len() == query.limit {
                Some(CompanyInventoryStopReason::RecordLimit)
            } else if page.scanned_records == query.scan_limit {
                Some(CompanyInventoryStopReason::ScanLimit)
            } else {
                None
            };
            if let Some(reason) = stop {
                page.stop_reason = reason;
                break;
            }
            let namespace = std::str::from_utf8(&value).map_err(|_| inconsistent())?;
            let address =
                CompanyMemoryAddress::from_namespace(namespace)?.ok_or_else(inconsistent)?;
            let workspace = CompanyMemoryWorkspace {
                workspace_id: address.workspace_id()?,
                company_id: address.company_id.clone(),
                scope: address.scope.clone(),
                private_subject_id: address.private_subject_id.clone(),
            };
            match &boundary {
                Boundary::Scope(expected) if namespace != *expected => return Err(inconsistent()),
                Boundary::Company(expected, _) if address.company_id != *expected => {
                    return Err(inconsistent());
                }
                _ => {}
            }
            // Count every traversed index position, but do not inspect payloads outside selection.
            if let Boundary::Company(_, selection) = &boundary {
                if !selection.matches(&workspace) {
                    page.scanned_records += 1;
                    last = Some(key.to_vec());
                    continue;
                }
            }
            let id = Uuid::from_slice(&key[key.len() - 16..]).map_err(|_| inconsistent())?;
            let (record, size, _) = snapshot
                .record_with_bytes(namespace, id)?
                .ok_or_else(inconsistent)?;
            let expected_key = match boundary {
                Boundary::Scope(_) => scope_key(namespace, &record),
                Boundary::Company(_, _) => company_key(&address, &record)?,
            };
            if key.as_ref() != expected_key.as_slice() {
                return Err(inconsistent());
            }
            if page.record_bytes.saturating_add(size) > MAX_MEMORY_READ_BYTES {
                if last.is_none() {
                    return Err(invalid("A memory exceeds the 8 MiB listing byte budget"));
                }
                page.stop_reason = CompanyInventoryStopReason::ByteLimit;
                break;
            }
            page.record_bytes += size;
            page.scanned_records += 1;
            last = Some(key.to_vec());
            if !filter.matches(&record, needle.as_deref()) {
                continue;
            }
            let entry = snapshot.company_entry(namespace, record)?;
            if view == CompanyMemoryView::Retained || entry.eligibility.eligible {
                page.entries.push(OrderedCompanyMemoryEntry {
                    workspace,
                    record: entry.record,
                    eligibility: entry.eligibility,
                });
            }
        }
        if page.stop_reason != CompanyInventoryStopReason::Exhausted {
            let last = last.ok_or_else(inconsistent)?;
            page.next_cursor = Some(hex(&encode(&Cursor {
                version: 1,
                binding,
                position: hex(&last[prefix.len()..]),
            })?));
        }
        page.dependency_work = snapshot.dependency_work();
        Ok(page)
    }
}
