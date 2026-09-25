//! Origin-aware company catalogs with request-wide verification limits.
use super::super::knowledge_origin::KnowledgeOriginDescriptor;
use super::*;

use super::super::knowledge_selection::{KnowledgeOriginKind, canonical_origin_kinds};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LearningCatalogCursorV2 {
    pub version: u32,
    pub company_id: String,
    pub filter_digest: String,
    pub position: String,
    pub accepted_origin_kinds: Vec<KnowledgeOriginKind>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LearningCatalogQueryV2 {
    pub kind: LearningResourceKind,
    #[serde(default)]
    pub filter: LearningCatalogFilter,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default = "default_scan")]
    pub max_scanned_records: usize,
    pub cursor: Option<LearningCatalogCursorV2>,
    pub accepted_origin_kinds: Vec<KnowledgeOriginKind>,
}
impl LearningCatalogQueryV2 {
    pub fn canonical_origins(&self) -> Result<Vec<KnowledgeOriginKind>> {
        canonical_origin_kinds(self.accepted_origin_kinds.clone())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearningCatalogEntryV2 {
    #[serde(flatten)]
    pub entry: LearningCatalogEntry,
    pub origin: Option<KnowledgeOriginDescriptor>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LearningCatalogStopV2 {
    Exhausted,
    EntryLimit,
    ScanLimit,
    ByteLimit,
    OriginRecordLimit,
    OriginByteLimit,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LearningCatalogPageV2 {
    pub entries: Vec<LearningCatalogEntryV2>,
    pub next_cursor: Option<LearningCatalogCursorV2>,
    pub stop_reason: LearningCatalogStopV2,
    pub scanned_records: usize,
    pub scanned_record_bytes: usize,
    pub skipped_non_platform_records: usize,
    pub observed_at_millis: i64,
    pub origin_records_examined: usize,
    pub origin_bytes_examined: usize,
    pub origin_lookahead_bytes: usize,
}

impl LearningMemory {
    pub fn company_learning_catalog_with_origin(
        &self,
        company: &str,
        query: &LearningCatalogQueryV2,
    ) -> Result<LearningCatalogPageV2> {
        validate_text(company, "company", 256)?;
        if !matches!(
            query.kind,
            LearningResourceKind::Knowledge | LearningResourceKind::Procedure
        ) {
            return Err(invalid(
                "Origin-aware catalogs require knowledge or procedure kind",
            ));
        }
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
        let kinds = query.canonical_origins()?;
        let filter_digest = super::super::knowledge_origin_hash::digest(
            super::super::knowledge_origin_hash::OriginHashDomain::CatalogBinding,
            &serde_json::json!({"version":2,"company_id":company,"kind":query.kind,"filter":query.filter,"accepted_origin_kinds":kinds}),
        )?;
        let prefix = query.kind.prefix(company);
        let after = query.cursor.as_ref().map(|cursor| {
            if cursor.version != 2 || cursor.company_id != company || cursor.filter_digest != filter_digest || cursor.accepted_origin_kinds != kinds {
                return Err(invalid("Catalog cursor does not match the company, resource kind, filters and origins"));
            }
            let key = unhex(&cursor.position)?;
            resource_at(query.kind, company, &prefix, &key).map_err(|_| invalid("Invalid catalog cursor position"))?;
            Ok(key)
        }).transpose()?;
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
        let mut budget = super::super::knowledge_origin::OriginReadBudget::new(1000, SCAN_BYTES);
        let mut page = LearningCatalogPageV2 {
            entries: vec![],
            next_cursor: None,
            stop_reason: LearningCatalogStopV2::Exhausted,
            scanned_records: 0,
            scanned_record_bytes: 0,
            skipped_non_platform_records: 0,
            observed_at_millis: chrono::Utc::now().timestamp_millis(),
            origin_records_examined: 0,
            origin_bytes_examined: 0,
            origin_lookahead_bytes: 0,
        };
        let text = query.filter.text.as_ref().map(|v| v.to_lowercase());
        let mut last = None;
        while let Some(key) = iterator.key() {
            if !key.starts_with(&prefix) {
                return Err(corrupt());
            }
            if page.entries.len() == query.limit {
                page.stop_reason = LearningCatalogStopV2::EntryLimit;
                break;
            }
            if page.scanned_records == query.max_scanned_records {
                page.stop_reason = LearningCatalogStopV2::ScanLimit;
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
                page.stop_reason = LearningCatalogStopV2::ByteLimit;
                break;
            }
            let resource = resource_at(query.kind, company, &prefix, key)?;
            page.scanned_records += 1;
            page.scanned_record_bytes += bytes;
            match resource {
                Some(resource) if query.filter.matches(&resource) => {
                    let verified =
                        self.catalog_origin_details_bounded(company, &resource, &mut budget)?;
                    if budget.lookahead_bytes > SCAN_BYTES {
                        return Err(Error::DataCorruption(
                            "Learning origin record exceeds its 4 MiB inspection ceiling".into(),
                        ));
                    }
                    if let Some(stop) = budget.stop {
                        if last.is_none() {
                            return Err(Error::DataCorruption(
                                "Catalog origin cannot be validated within a fresh page budget"
                                    .into(),
                            ));
                        }
                        page.stop_reason = match stop {
                            super::super::knowledge_origin::OriginBudgetStop::Records => {
                                LearningCatalogStopV2::OriginRecordLimit
                            }
                            super::super::knowledge_origin::OriginBudgetStop::Bytes => {
                                LearningCatalogStopV2::OriginByteLimit
                            }
                        };
                        break;
                    }
                    let (details, origin) = verified.ok_or_else(corrupt)?;
                    let entry = details.summary(resource);
                    if origin.as_ref().is_none_or(|o| {
                        kinds.iter().any(|kind| {
                            matches!(
                                (kind, o),
                                (
                                    KnowledgeOriginKind::MemoryOnly,
                                    KnowledgeOriginDescriptor::MemoryOnly
                                ) | (
                                    KnowledgeOriginKind::ExperienceMemory,
                                    KnowledgeOriginDescriptor::ExperienceMemory { .. }
                                )
                            )
                        })
                    }) && text.as_ref().is_none_or(|v| {
                        entry.title.to_lowercase().contains(v)
                            || entry.resource.id.to_lowercase().contains(v)
                    }) {
                        page.entries.push(LearningCatalogEntryV2 { entry, origin });
                    }
                }
                None => page.skipped_non_platform_records += 1,
                _ => (),
            }
            last = Some(key.to_vec());
            iterator.next();
        }
        iterator.status().map_err(storage_error)?;
        page.origin_records_examined = budget.records_examined;
        page.origin_bytes_examined = budget.bytes_examined;
        page.origin_lookahead_bytes = budget.lookahead_bytes;
        if page.stop_reason != LearningCatalogStopV2::Exhausted {
            page.next_cursor = Some(LearningCatalogCursorV2 {
                version: 2,
                company_id: company.into(),
                filter_digest,
                position: hex(&last.ok_or_else(corrupt)?),
                accepted_origin_kinds: kinds,
            });
        }
        Ok(page)
    }
}

impl LearningMemory {
    fn catalog_origin_details_bounded(
        &self,
        company: &str,
        resource: &LearningResourceRef,
        budget: &mut super::super::knowledge_origin::OriginReadBudget,
    ) -> Result<Option<(LearningResourceDetails, Option<KnowledgeOriginDescriptor>)>> {
        use super::super::{bound, knowledge, knowledge_origin, registry};
        let namespace = resource.namespace(company)?.ok_or_else(corrupt)?;
        let id = &resource.id;
        let Some(binding): Option<bound::ProposalReceipt> =
            budget.read_record(self, bound::binding_key(company, &namespace, id))?
        else {
            return Ok(None);
        };
        let Some(policy): Option<registry::RegistryEntry<registry::PolicyDefinition>> = budget
            .read_record(
                self,
                registry::registry_key(4, company, &binding.request.policy_id),
            )?
        else {
            return Ok(None);
        };
        let Some(context): Option<registry::RegistryEntry<registry::EvaluationContext>> = budget
            .read_record(
                self,
                registry::registry_key(5, company, &binding.request.context_id),
            )?
        else {
            return Ok(None);
        };
        if policy.schema_version != 1
            || policy.tenant != company
            || policy.id != binding.request.policy_id
            || policy.payload_digest != registry::digest(&policy.payload)?
            || context.schema_version != 1
            || context.tenant != company
            || context.id != binding.request.context_id
            || context.payload_digest != registry::digest(&context.payload)?
            || policy.payload.parameters.evaluation_contract != context.payload.evaluation_contract
        {
            return Err(corrupt());
        }
        let scope = Self::bound_scope(company, &namespace, &policy, &context)?;
        let Some(record): Option<ProcedureRecord> =
            budget.read_record(self, procedure_key(&scope, id))?
        else {
            return Ok(None);
        };
        Self::validate_registered_binding(
            company, &namespace, id, &binding, &record, &policy, &context,
        )?;
        let procedure = bound::RegisteredProcedure {
            receipt: binding,
            record,
        };
        let receipt: Option<knowledge::KnowledgeReceipt> =
            budget.read_optional(self, knowledge::key(company, &namespace, id)?)?;
        if budget.stop.is_some() {
            return Ok(None);
        }
        // Even an ordinary or generic procedure must prove there is no orphan
        // mandatory origin record; source markers alone are not authoritative.
        let stored_origin: Option<knowledge_origin::CombinedKnowledgeReceipt> =
            budget.read_optional(self, knowledge_origin::origin_key(company, &namespace, id)?)?;
        if budget.stop.is_some() {
            return Ok(None);
        }
        let Some(receipt) = receipt else {
            if stored_origin.is_some()
                || knowledge::has_knowledge_binding_marker(&procedure.record.proposal.source_refs)
                || knowledge_origin::has_origin_marker(&procedure.record.proposal.source_refs)
                || resource.kind == LearningResourceKind::Knowledge
            {
                return Err(corrupt());
            }
            return Ok(Some((LearningResourceDetails::Procedure(procedure), None)));
        };
        let origin = if stored_origin.is_some()
            || knowledge_origin::has_origin_marker(&receipt.record.proposal.source_refs)
        {
            let Some(origin) = self.validate_combined_origin_bounded(
                company, &namespace, id, &receipt, &procedure, &context, budget,
            )?
            else {
                return Ok(None);
            };
            // Catalog details describe retained provenance, not current reuse eligibility.
            origin.0
        } else {
            Self::validate_knowledge_binding(company, &namespace, id, &receipt, &procedure)?;
            KnowledgeOriginDescriptor::MemoryOnly
        };
        let details = if resource.kind == LearningResourceKind::Knowledge {
            LearningResourceDetails::Knowledge(KnowledgeDetails { receipt, procedure })
        } else {
            LearningResourceDetails::Procedure(procedure)
        };
        Ok(Some((details, Some(origin))))
    }
}
