//! Rebuildable active-knowledge index; authoritative ledger formats stay unchanged.
use super::knowledge::{KnowledgeReceipt, has_knowledge_binding_marker};
use super::*;

pub(super) const INDEX_ROOT: &[u8] = b"\0knowledge-selection-v3/";
pub(super) const GENERATION_KEY: &[u8] = b"\0knowledge-selection-v3-generation";
const CHUNK_ENTRIES: usize = 128;
// Flush after crossing this threshold: one encoded entry may exceed it.
// This bounds batching overhead, not total ledger size or a single record.
const CHUNK_BYTES: usize = 1_048_576;

#[derive(Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct KnowledgeLocator {
    pub tenant: String,
    pub namespace: String,
    pub id: String,
}

impl LearningMemory {
    fn generation_prefix(&self, kind: u8) -> Vec<u8> {
        let mut key = INDEX_ROOT.to_vec();
        key.extend_from_slice(self.inner.knowledge_generation.as_bytes());
        key.push(kind);
        key
    }

    pub(super) fn locator_key(&self, record: &ProcedureRecord) -> Vec<u8> {
        let mut key = self.generation_prefix(0);
        key.extend_from_slice(&procedure_key(&record.scope, &record.proposal.id));
        key
    }

    pub(super) fn knowledge_index_prefix(&self, receipt: &KnowledgeReceipt) -> Result<Vec<u8>> {
        let mut prefix = self.selection_index_prefix(
            &receipt.tenant,
            &receipt.namespace,
            &receipt.request.policy_id,
            &receipt.request.context_id,
            &receipt.request.external_identities()?,
        )?;
        if super::knowledge_origin::has_origin_marker(&receipt.record.proposal.source_refs) {
            // Keep ordinary-only readers on projection 1; mixed readers merge
            // projection 2 under one shared request budget and ordering key.
            prefix[INDEX_ROOT.len() + 16] = 2;
        }
        Ok(prefix)
    }

    pub(super) fn selection_index_prefix(
        &self,
        tenant: &str,
        namespace: &str,
        policy: &str,
        context: &str,
        identities: &[super::knowledge::ExternalToolIdentity],
    ) -> Result<Vec<u8>> {
        let mut key = self.generation_prefix(1);
        for value in [tenant, namespace, policy, context] {
            append_component(&mut key, value);
        }
        let identities = encode(&identities)?;
        key.extend_from_slice(&(identities.len() as u32).to_be_bytes());
        key.extend_from_slice(&identities);
        Ok(key)
    }

    pub(super) fn origin_selection_index_prefix(
        &self,
        mut prefix: Vec<u8>,
        kind: super::knowledge_selection::KnowledgeOriginKind,
    ) -> Vec<u8> {
        if kind == super::knowledge_selection::KnowledgeOriginKind::ExperienceMemory {
            prefix[INDEX_ROOT.len() + 16] = 2;
        }
        prefix
    }

    pub(super) fn active_key(
        &self,
        receipt: &KnowledgeReceipt,
        record: &ProcedureRecord,
    ) -> Result<Vec<u8>> {
        let bound = record
            .lower_improvement_bound
            .filter(|n| n.is_finite())
            .ok_or_else(|| {
                Error::DataCorruption("Active knowledge has no finite ranking bound".into())
            })?;
        // Ordered IEEE-754 transform, reversed for descending numeric order.
        // Normalize signed zero, then append the raw ID for UTF-8 lexical ties.
        let bits = if bound == 0.0 {
            0.0f64.to_bits()
        } else {
            bound.to_bits()
        };
        let ascending = if bits >> 63 == 1 {
            !bits
        } else {
            bits ^ (1 << 63)
        };
        let mut key = self.knowledge_index_prefix(receipt)?;
        key.extend_from_slice(&(!ascending).to_be_bytes());
        key.extend_from_slice(record.proposal.id.as_bytes());
        Ok(key)
    }

    fn locator_entry(&self, receipt: &KnowledgeReceipt) -> Result<(Vec<u8>, Vec<u8>)> {
        Ok((
            self.locator_key(&receipt.record),
            encode(&KnowledgeLocator {
                tenant: receipt.tenant.clone(),
                namespace: receipt.namespace.clone(),
                id: receipt.request.id.clone(),
            })?,
        ))
    }

    pub(super) fn put_knowledge_locator(
        &self,
        batch: &mut WriteBatch,
        receipt: &KnowledgeReceipt,
    ) -> Result<()> {
        let (key, value) = self.locator_entry(receipt)?;
        batch.put(key, value);
        Ok(())
    }

    pub(super) fn update_knowledge_index(
        &self,
        batch: &mut WriteBatch,
        before: &ProcedureRecord,
        after: &ProcedureRecord,
    ) -> Result<()> {
        if !has_knowledge_binding_marker(&before.proposal.source_refs) {
            return Ok(());
        }
        let bytes = self
            .inner
            .db
            .get(self.locator_key(before))
            .map_err(storage_error)?
            .ok_or_else(|| Error::DataCorruption("Knowledge index locator is missing".into()))?;
        let locator: KnowledgeLocator = decode(&bytes)?;
        let receipt = self
            .knowledge_receipt_any_origin(&locator.tenant, &locator.namespace, &locator.id)?
            .ok_or_else(|| Error::DataCorruption("Knowledge index receipt is missing".into()))?;
        if receipt.record.scope != before.scope
            || receipt.request.id != before.proposal.id
            || after.scope != before.scope
            || after.proposal != before.proposal
        {
            return Err(Error::DataCorruption(
                "Knowledge index transition binding mismatch".into(),
            ));
        }
        if before.state == ProcedureState::Active {
            batch.delete(self.active_key(&receipt, before)?);
        }
        if after.state == ProcedureState::Active {
            batch.put(self.active_key(&receipt, after)?, encode(after)?);
        }
        Ok(())
    }

    /// Enumerate every derived entry implied by the authoritative ledgers while
    /// auditing receipts, origins and bound procedures. Rebuilding persists the
    /// emitted entries; offline verification compares them with the persisted
    /// generation. Both paths share this single reading of the key layout.
    pub(super) fn audited_knowledge_entries(
        &self,
        sink: &mut dyn KnowledgeIndexSink,
    ) -> Result<KnowledgeIndexSummary> {
        let mut summary = KnowledgeIndexSummary::default();
        for item in self
            .inner
            .db
            .iterator(IteratorMode::From(&[16], Direction::Forward))
        {
            let (key, bytes) = item.map_err(storage_error)?;
            if key.first() != Some(&16) {
                break;
            }
            let raw: KnowledgeReceipt = decode(&bytes)?;
            if key.as_ref()
                != super::knowledge::key(&raw.tenant, &raw.namespace, &raw.request.id)?.as_slice()
            {
                return Err(Error::DataCorruption(
                    "Knowledge receipt storage key mismatch".into(),
                ));
            }
            let receipt = self
                .knowledge_receipt_any_origin(&raw.tenant, &raw.namespace, &raw.request.id)?
                .ok_or_else(|| {
                    Error::DataCorruption("Knowledge receipt disappeared during audit".into())
                })?;
            let current = self
                .registered_procedure(&raw.tenant, &raw.namespace, &raw.request.id)?
                .ok_or_else(|| {
                    Error::DataCorruption("Knowledge procedure missing during audit".into())
                })?;
            let (key, value) = self.locator_entry(&receipt)?;
            sink.entry(key, value)?;
            if current.record.state == ProcedureState::Active {
                sink.entry(
                    self.active_key(&receipt, &current.record)?,
                    encode(&current.record)?,
                )?;
                summary.active_entries += 1;
            }
            summary.knowledge_receipts += 1;
        }
        sink.receipts_complete()?;
        // Audit the reverse origin link, including orphaned origins whose
        // ordinary receipt was removed. Never publish an incomplete generation.
        for item in self
            .inner
            .db
            .iterator(IteratorMode::From(&[17], Direction::Forward))
        {
            let (key, bytes) = item.map_err(storage_error)?;
            if key.first() != Some(&17) {
                break;
            }
            let raw: super::knowledge_origin::CombinedKnowledgeReceipt = decode(&bytes)?;
            if key.as_ref()
                != super::knowledge_origin::origin_key(
                    &raw.tenant,
                    &raw.namespace,
                    &raw.request.knowledge.id,
                )?
                .as_slice()
            {
                return Err(Error::DataCorruption(
                    "Knowledge origin storage key mismatch".into(),
                ));
            }
            self.combined_knowledge_receipt(
                &raw.tenant,
                &raw.namespace,
                &raw.request.knowledge.id,
            )?
            .ok_or_else(|| {
                Error::DataCorruption("Knowledge origin disappeared during audit".into())
            })?;
            summary.combined_origins += 1;
        }
        // Also inspect authoritative procedures: missing receipts must not silently
        // turn active knowledge into an apparently empty derived index.
        for item in self
            .inner
            .db
            .iterator(IteratorMode::From(&[1], Direction::Forward))
        {
            let (key, bytes) = item.map_err(storage_error)?;
            if key.first() != Some(&1) {
                break;
            }
            let record: ProcedureRecord = decode(&bytes)?;
            if key.as_ref() != procedure_key(&record.scope, &record.proposal.id).as_slice() {
                return Err(Error::DataCorruption(
                    "Procedure storage key mismatch".into(),
                ));
            }
            if (has_knowledge_binding_marker(&record.proposal.source_refs)
                || super::knowledge_origin::has_origin_marker(&record.proposal.source_refs))
                && self
                    .inner
                    .db
                    .get(self.locator_key(&record))
                    .map_err(storage_error)?
                    .is_none()
            {
                return Err(Error::DataCorruption(
                    "Knowledge procedure has no validated receipt".into(),
                ));
            }
        }
        Ok(summary)
    }

    /// Called before publishing the opened handle; no writer can observe a partial generation.
    pub(super) fn rebuild_knowledge_index(&self) -> Result<()> {
        // Older binaries ignore this prefix. Every open discards all derived generations,
        // including interrupted builds, with bounded batches; authoritative data is untouched.
        let mut batch = WriteBatch::default();
        let mut count = 0;
        for item in self
            .inner
            .db
            .iterator(IteratorMode::From(INDEX_ROOT, Direction::Forward))
        {
            let (key, _) = item.map_err(storage_error)?;
            if !key.starts_with(INDEX_ROOT) {
                break;
            }
            batch.delete(&key);
            count += 1;
            if count == CHUNK_ENTRIES || batch.size_in_bytes() >= CHUNK_BYTES {
                self.inner
                    .db
                    .write_opt(batch, &write_options())
                    .map_err(storage_error)?;
                batch = WriteBatch::default();
                count = 0;
            }
        }
        self.inner
            .db
            .write_opt(batch, &write_options())
            .map_err(storage_error)?;
        let mut writer = ChunkedIndexWriter {
            store: self,
            batch: WriteBatch::default(),
            count: 0,
            receipts_rebuilt: 0,
        };
        let summary = self.audited_knowledge_entries(&mut writer)?;
        writer.flush()?;
        self.inner
            .db
            .put_opt(
                GENERATION_KEY,
                self.inner.knowledge_generation.as_bytes(),
                &write_options(),
            )
            .map_err(storage_error)?;
        tracing::debug!(generation = %self.inner.knowledge_generation, receipts_rebuilt = summary.knowledge_receipts, "knowledge_index_rebuild_published");
        Ok(())
    }
}

/// Counts observed while enumerating the derived knowledge index.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct KnowledgeIndexSummary {
    pub knowledge_receipts: u64,
    pub combined_origins: u64,
    pub active_entries: u64,
}

/// Receives derived index entries in enumeration order.
pub(super) trait KnowledgeIndexSink {
    fn entry(&mut self, key: Vec<u8>, value: Vec<u8>) -> Result<()>;
    /// Every receipt-derived entry has been emitted. Locators must be readable
    /// through the store before the procedure audit that follows.
    fn receipts_complete(&mut self) -> Result<()>;
}

struct ChunkedIndexWriter<'a> {
    store: &'a LearningMemory,
    batch: WriteBatch,
    count: usize,
    receipts_rebuilt: usize,
}

impl ChunkedIndexWriter<'_> {
    fn flush(&mut self) -> Result<()> {
        let batch = std::mem::take(&mut self.batch);
        self.store
            .inner
            .db
            .write_opt(batch, &write_options())
            .map_err(storage_error)?;
        self.count = 0;
        Ok(())
    }
}

impl KnowledgeIndexSink for ChunkedIndexWriter<'_> {
    fn entry(&mut self, key: Vec<u8>, value: Vec<u8>) -> Result<()> {
        self.batch.put(key, value);
        self.count += 1;
        self.receipts_rebuilt += 1;
        if self.count == CHUNK_ENTRIES || self.batch.size_in_bytes() >= CHUNK_BYTES {
            self.flush()?;
            tracing::debug!(generation = %self.store.inner.knowledge_generation, entries_rebuilt = self.receipts_rebuilt, "knowledge_index_rebuild_chunk_committed");
        }
        Ok(())
    }

    fn receipts_complete(&mut self) -> Result<()> {
        self.flush()
    }
}
