//! Company-bounded discovery from retained checkpoint primary keys, without a new index.
use super::snapshot::MemorySnapshot;
use super::*;
const MAX_BYTES: usize = 4 * 1024 * 1024;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompanyConsumerKind {
    MemoryV1,
    MemoryV2,
    RelationsV1,
}
impl CompanyConsumerKind {
    fn tag(self) -> u8 {
        match self {
            Self::MemoryV1 => 0x24,
            Self::MemoryV2 => 0x28,
            Self::RelationsV1 => 0x5a,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyConsumerRef {
    pub kind: CompanyConsumerKind,
    pub scope: MemoryResourceScope,
    pub private_subject_id: Option<String>,
    pub subject_id: String,
    pub consumer_id: String,
}
impl CompanyConsumerRef {
    fn namespace(&self, company: &str) -> Result<String> {
        valid(&self.subject_id, 256)?;
        valid(&self.consumer_id, 128)?;
        CompanyMemoryAddress::new(company, &self.scope, self.private_subject_id.as_deref())?
            .namespace()
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyConsumerCursor {
    pub version: u32,
    pub company_id: String,
    pub filter_digest: String,
    pub position: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyConsumerQuery {
    pub kind: CompanyConsumerKind,
    pub text: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default = "default_scan")]
    pub max_scanned_records: usize,
    pub cursor: Option<CompanyConsumerCursor>,
}
fn default_limit() -> usize {
    25
}
fn default_scan() -> usize {
    100
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyConsumerEntry {
    pub consumer: CompanyConsumerRef,
    pub revision: u64,
    pub sequence: u64,
    pub updated_at_millis: i64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompanyConsumerStop {
    Exhausted,
    EntryLimit,
    ScanLimit,
    ByteLimit,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyConsumerPage {
    pub entries: Vec<CompanyConsumerEntry>,
    pub next_cursor: Option<CompanyConsumerCursor>,
    pub stop_reason: CompanyConsumerStop,
    pub scanned_records: usize,
    pub scanned_record_bytes: usize,
    pub observed_at_millis: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "cursor",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum CompanyConsumerWitness {
    MemoryV2(VerifiedMemoryCursor),
    RelationsV1(RelationChangeCursor),
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyConsumerObservation {
    pub checkpoint: MemoryCheckpoint,
    pub high_watermark: Option<MemoryChangeCursor>,
    /// Matching journal identity and sequence bounds cannot verify a legacy history branch.
    pub position_in_range: bool,
    pub sequence_distance_estimate: Option<u64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "observation",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum CompanyConsumerDetails {
    MemoryV1(LegacyConsumerObservation),
    MemoryV2(MemoryConsumerDiagnostics),
    RelationsV1(RelationConsumerDiagnostics),
}
fn valid(s: &str, max: usize) -> Result<()> {
    if s.trim().is_empty() || s.len() > max || s.chars().any(char::is_control) {
        Err(Error::ValidationError(
            "Invalid consumer identity or filter".into(),
        ))
    } else {
        Ok(())
    }
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn unhex(s: &str) -> Result<Vec<u8>> {
    if s.is_empty()
        || s.len() > 32768
        || s.len() % 2 != 0
        || !s
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(Error::ValidationError(
            "Invalid consumer continuation".into(),
        ));
    }
    s.as_bytes()
        .chunks_exact(2)
        .map(|v| {
            u8::from_str_radix(std::str::from_utf8(v).unwrap(), 16).map_err(|_| inconsistent())
        })
        .collect()
}
fn key(
    kind: CompanyConsumerKind,
    namespace: &str,
    subject: &str,
    consumer: &str,
) -> Result<Vec<u8>> {
    let mut key = vec![kind.tag()];
    key.extend(encode(&(namespace, subject, consumer))?);
    Ok(key)
}
fn company_prefix(company: &str, kind: CompanyConsumerKind) -> Result<Vec<u8>> {
    valid(company, 256)?;
    // The outer tuple encodes the canonical namespace as a JSON string. Preserve
    // both escaping layers and the inner company delimiter; "a" must not match "aa".
    let namespace_prefix = format!(
        "qdb:scope:v1:[{},",
        serde_json::to_string(company).map_err(|_| inconsistent())?
    );
    let mut encoded = encode(&namespace_prefix)?;
    if encoded.pop() != Some(b'"') {
        return Err(inconsistent());
    }
    let mut prefix = vec![kind.tag(), b'['];
    prefix.extend(encoded);
    Ok(prefix)
}
fn decode_selection(
    company: &str,
    kind: CompanyConsumerKind,
    bytes: &[u8],
) -> Result<(CompanyConsumerRef, String)> {
    if bytes.first() != Some(&kind.tag()) || bytes.len() > 16384 {
        return Err(inconsistent());
    }
    let (namespace, subject, consumer): (String, String, String) = decode(&bytes[1..])?;
    if key(kind, &namespace, &subject, &consumer)? != bytes {
        return Err(inconsistent());
    }
    let address = CompanyMemoryAddress::from_namespace(&namespace)?.ok_or_else(inconsistent)?;
    if address.company_id != company {
        return Err(inconsistent());
    }
    let selected = CompanyConsumerRef {
        kind,
        scope: address.scope,
        private_subject_id: address.private_subject_id,
        subject_id: subject,
        consumer_id: consumer,
    };
    if selected.namespace(company).map_err(|_| inconsistent())? != namespace {
        return Err(inconsistent());
    }
    Ok((selected, namespace))
}
impl MemorySnapshot<'_> {
    fn consumer_entry(
        &self,
        company: &str,
        kind: CompanyConsumerKind,
        bytes: &[u8],
    ) -> Result<CompanyConsumerEntry> {
        let (consumer, namespace) = decode_selection(company, kind, bytes)?;
        let subject = &consumer.subject_id;
        let id = &consumer.consumer_id;
        let (revision, sequence, updated_at_millis) = match kind {
            CompanyConsumerKind::MemoryV1 => {
                let c = self
                    .legacy_consumer_checkpoint(&namespace, subject, id)?
                    .ok_or_else(inconsistent)?;
                (c.revision, c.cursor.sequence, c.updated_at_millis)
            }
            CompanyConsumerKind::MemoryV2 => {
                let c = self
                    .stored_verified_checkpoint(&namespace, subject, id)?
                    .ok_or_else(inconsistent)?;
                (c.revision, c.cursor.sequence, c.updated_at_millis)
            }
            CompanyConsumerKind::RelationsV1 => {
                let c = self
                    .stored_relation_checkpoint(&namespace, subject, id)?
                    .ok_or_else(inconsistent)?;
                (c.revision, c.cursor.sequence, c.updated_at_millis)
            }
        };
        Ok(CompanyConsumerEntry {
            consumer,
            revision,
            sequence,
            updated_at_millis,
        })
    }
    fn legacy_consumer_checkpoint(
        &self,
        namespace: &str,
        subject: &str,
        consumer: &str,
    ) -> Result<Option<MemoryCheckpoint>> {
        let bytes = self
            .db
            .get_cf(
                self.storage.cf(crate::storage::cf::AGENT_META)?,
                key(CompanyConsumerKind::MemoryV1, namespace, subject, consumer)?,
            )
            .map_err(storage_error)?;
        bytes
            .map(|bytes| {
                let c: MemoryCheckpoint = decode(&bytes)?;
                c.validate(namespace, subject, consumer)?;
                Ok(c)
            })
            .transpose()
    }
}
impl RocksDbMemoryStorage {
    /// Trusted company administration; HTTP must derive the company from current authority.
    /// Reads canonical checkpoint keys directly, including checkpoint-only namespaces.
    pub fn company_consumers(
        &self,
        company: &str,
        query: &CompanyConsumerQuery,
    ) -> Result<CompanyConsumerPage> {
        if !(1..=50).contains(&query.limit) || !(1..=1000).contains(&query.max_scanned_records) {
            return Err(Error::ValidationError(
                "Consumer limits require 1–50 results and 1–1000 primary records".into(),
            ));
        }
        if let Some(text) = &query.text {
            valid(text, 256)?;
        }
        let prefix = company_prefix(company, query.kind)?;
        let filter_digest = hex(&digest(&encode(&(
            "company-consumers-v1",
            query.kind,
            &query.text,
        ))?));
        let after = query
            .cursor
            .as_ref()
            .map(|c| {
                if c.version != 1 || c.company_id != company || c.filter_digest != filter_digest {
                    return Err(Error::ValidationError(
                        "Consumer continuation does not match company, feed kind and filter".into(),
                    ));
                }
                let k = unhex(&c.position)?;
                if !k.starts_with(&prefix) {
                    return Err(Error::ValidationError(
                        "Consumer continuation is outside this company".into(),
                    ));
                }
                decode_selection(company, query.kind, &k).map_err(|_| {
                    Error::ValidationError("Invalid consumer continuation identity".into())
                })?;
                Ok(k)
            })
            .transpose()?;
        let snapshot = self.memory_snapshot();
        let cf = self.cf(crate::storage::cf::AGENT_META)?;
        let mut options = rocksdb::ReadOptions::default();
        options.set_iterate_lower_bound(prefix.clone());
        let mut upper = prefix.clone();
        while upper.last() == Some(&255) {
            upper.pop();
        }
        *upper.last_mut().ok_or_else(inconsistent)? += 1;
        options.set_iterate_upper_bound(upper);
        let mut iter = snapshot.db.raw_iterator_cf_opt(cf, options);
        iter.seek(after.as_deref().unwrap_or(&prefix));
        if iter.key().is_some_and(|k| Some(k) == after.as_deref()) {
            iter.next();
        }
        let mut page = CompanyConsumerPage {
            entries: vec![],
            next_cursor: None,
            stop_reason: CompanyConsumerStop::Exhausted,
            scanned_records: 0,
            scanned_record_bytes: 0,
            observed_at_millis: snapshot.now,
        };
        let text = query.text.as_ref().map(|s| s.to_lowercase());
        let mut last = None;
        while let Some(k) = iter.key() {
            if !k.starts_with(&prefix) {
                return Err(inconsistent());
            }
            if page.entries.len() == query.limit {
                page.stop_reason = CompanyConsumerStop::EntryLimit;
                break;
            }
            if page.scanned_records == query.max_scanned_records {
                page.stop_reason = CompanyConsumerStop::ScanLimit;
                break;
            }
            let bytes = k
                .len()
                .checked_add(iter.value().ok_or_else(inconsistent)?.len())
                .ok_or_else(inconsistent)?;
            if bytes > MAX_BYTES {
                return Err(inconsistent());
            }
            if page.scanned_record_bytes + bytes > MAX_BYTES {
                page.stop_reason = CompanyConsumerStop::ByteLimit;
                break;
            }
            let entry = snapshot.consumer_entry(company, query.kind, k)?;
            page.scanned_records += 1;
            page.scanned_record_bytes += bytes;
            let c = &entry.consumer;
            if text.as_ref().is_none_or(|t| {
                [
                    &c.consumer_id,
                    &c.subject_id,
                    &c.scope.project_id,
                    &c.scope.agent_id,
                ]
                .into_iter()
                .chain(c.scope.mission_id.iter())
                .chain(c.private_subject_id.iter())
                .any(|s| s.to_lowercase().contains(t))
            }) {
                page.entries.push(entry);
            }
            last = Some(k.to_vec());
            iter.next();
        }
        iter.status().map_err(storage_error)?;
        if page.stop_reason != CompanyConsumerStop::Exhausted {
            page.next_cursor = Some(CompanyConsumerCursor {
                version: 1,
                company_id: company.into(),
                filter_digest,
                position: hex(&last.ok_or_else(inconsistent)?),
            });
        }
        Ok(page)
    }
    /// Observe stored progress without acknowledging events, changing checkpoints or issuing credentials.
    pub fn inspect_company_consumer(
        &self,
        company: &str,
        consumer: &CompanyConsumerRef,
        witness: Option<&CompanyConsumerWitness>,
    ) -> Result<Option<CompanyConsumerDetails>> {
        let namespace = consumer.namespace(company)?;
        let subject = &consumer.subject_id;
        let id = &consumer.consumer_id;
        match (consumer.kind, witness) {
            (CompanyConsumerKind::MemoryV1, None) => {
                let view = self.memory_snapshot();
                let Some(checkpoint) = view.legacy_consumer_checkpoint(&namespace, subject, id)?
                else {
                    return Ok(None);
                };
                let high_watermark = view.journal(&namespace)?.map(|s| s.cursor);
                let position_in_range = high_watermark.as_ref().is_some_and(|tip| {
                    tip.journal_id == checkpoint.cursor.journal_id
                        && checkpoint.cursor.sequence <= tip.sequence
                });
                let sequence_distance_estimate = if position_in_range {
                    Some(high_watermark.as_ref().unwrap().sequence - checkpoint.cursor.sequence)
                } else {
                    None
                };
                Ok(Some(CompanyConsumerDetails::MemoryV1(
                    LegacyConsumerObservation {
                        checkpoint,
                        high_watermark,
                        position_in_range,
                        sequence_distance_estimate,
                    },
                )))
            }
            (CompanyConsumerKind::MemoryV2, None | Some(CompanyConsumerWitness::MemoryV2(_))) => {
                let witness = match witness {
                    Some(CompanyConsumerWitness::MemoryV2(c)) => Some(c),
                    _ => None,
                };
                let details = self.diagnose_memory_consumer(&namespace, subject, id, witness)?;
                Ok(details
                    .checkpoint
                    .is_some()
                    .then_some(CompanyConsumerDetails::MemoryV2(details)))
            }
            (
                CompanyConsumerKind::RelationsV1,
                None | Some(CompanyConsumerWitness::RelationsV1(_)),
            ) => {
                let witness = match witness {
                    Some(CompanyConsumerWitness::RelationsV1(c)) => Some(c),
                    _ => None,
                };
                let details = self.diagnose_relation_consumer(&namespace, subject, id, witness)?;
                Ok(details
                    .checkpoint
                    .is_some()
                    .then_some(CompanyConsumerDetails::RelationsV1(details)))
            }
            _ => Err(Error::ValidationError(
                "Witness kind does not match the selected verified feed".into(),
            )),
        }
    }
}
#[cfg(test)]
mod tests;
