use super::*;
const MAX_LIST_BYTES: usize = 4 * 1024 * 1024;
const MAX_CONTEXT_BYTES: usize = 8 * 1024 * 1024;

impl RocksDbMemoryStorage {
    /// Ordered owner-scoped current jobs. A cursor resumes scanning, not a durable snapshot.
    pub fn query_consolidation_jobs(
        &self,
        namespace: &str,
        owner: &str,
        query: &ConsolidationQuery,
    ) -> Result<ConsolidationPage> {
        check_identity(namespace, owner)?;
        if !(1..=100).contains(&query.limit) || !(1..=1000).contains(&query.scan_limit) {
            return Err(Error::ValidationError(
                "Consolidation list requires a limit of 1-100 and scan limit of 1-1000".into(),
            ));
        }
        let snapshot = self.memory_snapshot();
        let prefix = owner_prefix(JOB, namespace, owner)?;
        let mut end = prefix.clone();
        while let Some(last) = end.pop() {
            if last != u8::MAX {
                end.push(last + 1);
                break;
            }
        }
        let mut options = rocksdb::ReadOptions::default();
        options.set_iterate_lower_bound(prefix.clone());
        options.set_iterate_upper_bound(end);
        let mut iter = snapshot
            .db
            .raw_iterator_cf_opt(self.cf(crate::storage::cf::AGENT_META)?, options);
        let mut seek = prefix.clone();
        if let Some(id) = query.after {
            seek.extend(id.as_bytes());
        }
        iter.seek(&seek);
        iter.status().map_err(storage_error)?;
        if query.after.is_some() && iter.key() == Some(seek.as_slice()) {
            iter.next();
        }
        let mut jobs = Vec::new();
        let mut count = 0;
        let mut bytes = 0;
        let mut last = None;
        while iter.valid() && jobs.len() < query.limit && count < query.scan_limit {
            let key = iter.key().ok_or_else(inconsistent)?;
            if !key.starts_with(&prefix) || key.len() != prefix.len() + 16 {
                return Err(inconsistent());
            }
            let id = Uuid::from_slice(&key[prefix.len()..]).map_err(|_| inconsistent())?;
            let len = iter.value().ok_or_else(inconsistent)?.len();
            if len > MAX_JOB_BYTES {
                return Err(inconsistent());
            }
            if bytes + len > MAX_LIST_BYTES {
                break;
            }
            let job = snapshot
                .consolidation_job(namespace, owner, id)?
                .ok_or_else(inconsistent)?;
            count += 1;
            bytes += len;
            last = Some(id);
            if query.status.is_none_or(|status| status == job.status) {
                let active = lease_active(&job, self.consolidation_incarnation, snapshot.now);
                jobs.push(ConsolidationSummary {
                    job_id: id,
                    revision: job.revision,
                    status: job.status,
                    created_at_millis: job.created_at_millis,
                    modified_at_millis: job.modified_at_millis,
                    attempts: job.attempts.len(),
                    lease_active: active,
                    recoverable: job.status == ConsolidationStatus::Running && !active,
                });
            }
            iter.next();
            iter.status().map_err(storage_error)?;
        }
        iter.status().map_err(storage_error)?;
        Ok(ConsolidationPage {
            jobs,
            next_after: if iter.valid() { last } else { None },
            records_examined: count,
            record_bytes: bytes,
            evaluated_at_millis: snapshot.now,
        })
    }
    /// Caller authorizes company administration of this workspace namespace.
    /// Cursor ordering follows encoded owner and UUID bytes, not locale collation.
    pub fn query_admin_consolidation_jobs(
        &self,
        namespace: &str,
        query: &AdminConsolidationQuery,
    ) -> Result<AdminConsolidationPage> {
        Self::validate_agent(namespace)?;
        if let Some(cursor) = &query.after {
            check_identity(namespace, &cursor.owner_id)?;
        }
        if !(1..=100).contains(&query.limit) || !(1..=1000).contains(&query.scan_limit) {
            return Err(Error::ValidationError(
                "Consolidation list requires a limit of 1-100 and scan limit of 1-1000".into(),
            ));
        }
        let snapshot = self.memory_snapshot();
        let prefix = record_prefix(JOB, namespace);
        let mut end = prefix.clone();
        while let Some(last) = end.pop() {
            if last != u8::MAX {
                end.push(last + 1);
                break;
            }
        }
        let mut options = rocksdb::ReadOptions::default();
        options.set_iterate_lower_bound(prefix.clone());
        options.set_iterate_upper_bound(end);
        let mut iter = snapshot
            .db
            .raw_iterator_cf_opt(self.cf(crate::storage::cf::AGENT_META)?, options);
        let mut seek = prefix.clone();
        if let Some(cursor) = &query.after {
            seek = job_key(JOB, namespace, &cursor.owner_id, cursor.job_id)?;
        }
        iter.seek(&seek);
        iter.status().map_err(storage_error)?;
        if query.after.is_some() && iter.key() == Some(seek.as_slice()) {
            iter.next();
        }
        let mut jobs = Vec::new();
        let mut count = 0;
        let mut bytes = 0;
        let mut last = None;
        while iter.valid() && jobs.len() < query.limit && count < query.scan_limit {
            let key = iter.key().ok_or_else(inconsistent)?;
            if !key.starts_with(&prefix) {
                return Err(inconsistent());
            }
            let len = iter.value().ok_or_else(inconsistent)?.len();
            if len > MAX_JOB_BYTES {
                return Err(inconsistent());
            }
            if bytes + len > MAX_LIST_BYTES {
                break;
            }
            let candidate: ConsolidationJob = decode(iter.value().ok_or_else(inconsistent)?)?;
            let owner = &candidate.created_by.subject_id;
            check_identity(namespace, owner).map_err(|_| inconsistent())?;
            let id = candidate.job_id;
            if key != job_key(JOB, namespace, owner, id)?.as_slice() {
                return Err(inconsistent());
            }
            let job = snapshot
                .consolidation_job(namespace, owner, id)?
                .ok_or_else(inconsistent)?;
            count += 1;
            bytes += len;
            last = Some(AdminConsolidationCursor {
                owner_id: owner.clone(),
                job_id: id,
            });
            if query.status.is_none_or(|status| status == job.status) {
                let active = lease_active(&job, self.consolidation_incarnation, snapshot.now);
                jobs.push(AdminConsolidationEntry {
                    owner_id: owner.clone(),
                    summary: ConsolidationSummary {
                        job_id: id,
                        revision: job.revision,
                        status: job.status,
                        created_at_millis: job.created_at_millis,
                        modified_at_millis: job.modified_at_millis,
                        attempts: job.attempts.len(),
                        lease_active: active,
                        recoverable: job.status == ConsolidationStatus::Running && !active,
                    },
                });
            }
            iter.next();
            iter.status().map_err(storage_error)?;
        }
        iter.status().map_err(storage_error)?;
        Ok(AdminConsolidationPage {
            jobs,
            next_after: if iter.valid() { last } else { None },
            records_examined: count,
            record_bytes: bytes,
            evaluated_at_millis: snapshot.now,
        })
    }
    /// Exact source payloads for the current lease, in one snapshot. This read is not a
    /// freshness guarantee at model completion; publication rechecks the entire manifest.
    pub fn read_consolidation_context(
        &self,
        namespace: &str,
        author: &RecordAuthor,
        id: Uuid,
        revision: u64,
        fence: Uuid,
    ) -> Result<ConsolidationContext> {
        check_identity(namespace, &author.subject_id)?;
        let snapshot = self.memory_snapshot();
        let job = snapshot
            .consolidation_job(namespace, &author.subject_id, id)?
            .ok_or_else(|| {
                Error::KeyNotFound(
                    "Consolidation job is not available in the authorized scope and subject".into(),
                )
            })?;
        if revision == 0 || revision != job.revision {
            return Err(conflict("Consolidation job revision changed"));
        }
        super::execution::require_lease(
            &job,
            author,
            fence,
            self.consolidation_incarnation,
            snapshot.now,
        )?;
        super::execution::check_sources(&snapshot, namespace, &job.spec)?;
        let mut records = Vec::new();
        let mut bytes = 0;
        for source in &job.spec.sources {
            let (record, len, _) = snapshot
                .record_with_bytes(namespace, source.record_id)?
                .ok_or_else(inconsistent)?;
            bytes += len;
            if bytes > MAX_CONTEXT_BYTES {
                return Err(Error::ValidationError(
                    "Consolidation context exceeds 8 MiB of canonical records".into(),
                ));
            }
            if record.revision != source.revision {
                return Err(inconsistent());
            }
            records.push(record);
        }
        Ok(ConsolidationContext {
            job_id: id,
            job_revision: revision,
            fence,
            records,
            evaluated_at_millis: snapshot.now,
            record_bytes: bytes,
            dependency_work: snapshot.dependency_work(),
        })
    }
}
