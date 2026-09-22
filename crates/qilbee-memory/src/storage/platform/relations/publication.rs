//! Prepare bounded assertion batches inside the caller's mutation lock and WAL batch.
use super::*;

impl RocksDbMemoryStorage {
    /// The consolidation ledger owns the mutation lock, lease fence and final commit.
    /// A failed preparation requires dropping the entire batch; nothing is written here.
    pub(in crate::storage::platform) fn prepare_relation_publication(
        &self,
        snapshot: &MemorySnapshot<'_>,
        namespace: &str,
        author: &RecordAuthor,
        publication_id: Uuid,
        inputs: &[MemoryRelationInput],
        batch: &mut rocksdb::WriteBatch,
    ) -> Result<Vec<MemoryRelationReceipt>> {
        if inputs.len() > 16 {
            return Err(Error::ValidationError(
                "A publication allows at most 16 assertions".into(),
            ));
        }
        let cf = self.cf(crate::storage::cf::AGENT_META)?;
        let mut prepared = Vec::new();
        let mut ids = std::collections::BTreeSet::new();
        for (index, input) in inputs.iter().enumerate() {
            input.validate()?;
            snapshot.validate_relation_endpoints(namespace, input)?;
            let command = MemoryRelationCommand {
                contract_version: 1,
                idempotency_key: format!("consolidation:{publication_id}:{index}"),
                operation: MemoryRelationOperation::Assert {
                    relation: input.clone(),
                },
            };
            let mut receipt_key = record_prefix(RECEIPT, namespace);
            receipt_key.extend(encode(&(&author.subject_id, &command.idempotency_key))?);
            if snapshot
                .db
                .get_cf(cf, &receipt_key)
                .map_err(storage_error)?
                .is_some()
            {
                return Err(Error::TransactionConflict(
                    "Consolidation publication key collision".into(),
                ));
            }
            let id = Uuid::new_v4();
            if !ids.insert(id) || snapshot.relation(namespace, id)?.is_some() {
                return Err(Error::TransactionConflict(
                    "Relation identifier collision".into(),
                ));
            }
            let relation = MemoryRelation {
                schema_version: 1,
                relation_id: id,
                revision: 1,
                input: input.clone(),
                reported_by: author.clone(),
                created_at_millis: snapshot.now,
                modified_at_millis: snapshot.now,
                state: RelationState::Active,
                review: None,
            };
            let mut receipt = MemoryRelationReceipt {
                contract_version: 1,
                idempotency_key: command.idempotency_key.clone(),
                relation_id: id,
                revision: 1,
                action: RelationAction::Asserted,
                author: author.clone(),
                committed_at_millis: snapshot.now,
                evidence_ref: input.provenance.evidence_ref.clone(),
                relation_digest: hex_digest(&encode(&relation)?),
                command_digest: hex_digest(&encode(&command)?),
                receipt_digest: String::new(),
            };
            receipt.receipt_digest = receipt.calculate_digest(namespace)?;
            let history_key = history_key(namespace, id, 1);
            if snapshot
                .db
                .get_cf(cf, &history_key)
                .map_err(storage_error)?
                .is_some()
            {
                return Err(inconsistent());
            }
            let bytes = encode(&relation)?;
            let history = encode(&MemoryRelationRevision {
                relation: relation.clone(),
                receipt: receipt.clone(),
            })?;
            if bytes.len() > MAX_RELATION_RECORD_BYTES || history.len() > MAX_RELATION_RECORD_BYTES
            {
                return Err(Error::ValidationError(
                    "Relation metadata exceeds the 16 KiB bound".into(),
                ));
            }
            let integrity = encode(&RelationIntegrity {
                schema_version: 1,
                revision: 1,
                record_digest: digest(&bytes),
                history_digest: digest(&history),
            })?;
            batch.put_cf(cf, record_key(RELATION, namespace, id), bytes);
            batch.put_cf(cf, record_key(INTEGRITY, namespace, id), &integrity);
            batch.put_cf(cf, history_key, history);
            batch.put_cf(
                cf,
                receipt_key,
                encode(&StoredRelationReceipt {
                    schema_version: 1,
                    namespace: namespace.into(),
                    request_digest: digest(&encode(&command)?),
                    receipt: receipt.clone(),
                })?,
            );
            for (kind, endpoint) in [(OUTGOING, &input.source), (INCOMING, &input.target)] {
                batch.put_cf(cf, adjacency_key(kind, namespace, endpoint, id), &integrity);
            }
            prepared.push((relation, receipt, integrity));
        }
        self.append_relation_change_batch(
            snapshot,
            namespace,
            &prepared
                .iter()
                .map(|(r, receipt, _)| (r, receipt))
                .collect::<Vec<_>>(),
            batch,
        )?;
        self.append_relation_head_batch(
            snapshot,
            namespace,
            &prepared
                .iter()
                .map(|(r, _, integrity)| (r, integrity.as_slice()))
                .collect::<Vec<_>>(),
            batch,
        )?;
        Ok(prepared
            .into_iter()
            .map(|(_, receipt, _)| receipt)
            .collect())
    }
}
