//! Company-owned human labels. Labels never create agents or grant authority.
use super::*;
const PROFILE: u8 = 0x44;
const HISTORY: u8 = 0x45;
const RECEIPT: u8 = 0x46;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentDisplayProfile {
    pub schema_version: u32,
    pub company_id: String,
    pub agent_id: String,
    pub revision: u64,
    pub display_name: Option<String>,
    pub updated_at_millis: i64,
    pub updated_by: RecordAuthor,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentProfileCommand {
    pub agent_id: String,
    pub expected_revision: u64,
    #[serde(deserialize_with = "required_nullable_name")]
    pub display_name: Option<String>,
    pub idempotency_key: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentProfileReceipt {
    pub command: AgentProfileCommand,
    pub profile: AgentDisplayProfile,
}
#[derive(Debug, Serialize)]
pub struct AgentProfileOutcome {
    pub receipt: AgentProfileReceipt,
    pub replayed: bool,
}
#[derive(Debug, Serialize)]
pub struct NamedAgent {
    pub registration: ObservedAgent,
    pub profile: Option<AgentDisplayProfile>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentDirectoryCursor {
    pub version: u32,
    pub company_id: String,
    pub filter_digest: String,
    pub after_agent_id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentDirectoryQuery {
    pub text: Option<String>,
    #[serde(default = "page_limit")]
    pub limit: usize,
    #[serde(default = "scan_limit")]
    pub max_scanned_agents: usize,
    pub cursor: Option<AgentDirectoryCursor>,
}
fn page_limit() -> usize {
    25
}
fn scan_limit() -> usize {
    100
}
#[derive(Debug, Serialize)]
pub struct NamedAgentPage {
    pub agents: Vec<NamedAgent>,
    pub next_cursor: Option<AgentDirectoryCursor>,
    pub scanned_agents: usize,
    pub observed_at_millis: i64,
}
#[derive(Debug, Serialize)]
pub struct AgentProfileHistory {
    pub receipts: Vec<AgentProfileReceipt>,
    pub next_after_revision: Option<u64>,
    pub current_revision: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Seal {
    receipt: AgentProfileReceipt,
    digest: [u8; 32],
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Tip {
    revision: u64,
    digest: [u8; 32],
}
fn profile_key(kind: u8, company: &str, id: &str) -> Vec<u8> {
    let mut value = record_prefix(kind, company);
    value.extend_from_slice(&(id.len() as u16).to_be_bytes());
    value.extend_from_slice(id.as_bytes());
    value
}
fn history_key(company: &str, agent: &str, revision: u64) -> Vec<u8> {
    let mut value = profile_key(HISTORY, company, agent);
    value.extend_from_slice(&revision.to_be_bytes());
    value
}
fn name_valid(name: &Option<String>) -> Result<()> {
    if let Some(name) = name {
        valid_id(name)?;
        if name.trim() != name {
            return Err(Error::ValidationError(
                "Display names must not have surrounding whitespace".into(),
            ));
        }
    }
    Ok(())
}
fn required_nullable_name<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<String>, D::Error> {
    Option::<String>::deserialize(deserializer)
}
impl AgentProfileCommand {
    fn validate(&self) -> Result<()> {
        valid_id(&self.agent_id)?;
        valid_id(&self.idempotency_key)?;
        name_valid(&self.display_name)
    }
}
impl Seal {
    fn new(receipt: AgentProfileReceipt) -> Result<Self> {
        Ok(Self {
            digest: digest(&encode(&receipt)?),
            receipt,
        })
    }
    fn validate(&self, company: &str, agent: &str) -> Result<()> {
        let r = &self.receipt;
        let p = &r.profile;
        r.command.validate().map_err(|_| inconsistent())?;
        valid_id(&p.updated_by.subject_id).map_err(|_| inconsistent())?;
        if p.schema_version != 1
            || p.company_id != company
            || p.agent_id != agent
            || r.command.agent_id != agent
            || p.display_name != r.command.display_name
            || r.command.expected_revision.checked_add(1) != Some(p.revision)
            || self.digest != digest(&encode(r)?)
            || chrono::DateTime::from_timestamp_millis(p.updated_at_millis).is_none()
        {
            return Err(inconsistent());
        }
        Ok(())
    }
}
impl RocksDbMemoryStorage {
    fn profile_bytes(&self, key: Vec<u8>) -> Result<Option<Vec<u8>>> {
        self.db
            .get_cf(self.cf(super::super::super::cf::AGENT_META)?, key)
            .map_err(storage_error)
    }
    fn profile_seal(&self, company: &str, agent: &str, revision: u64) -> Result<Seal> {
        let bytes = self
            .profile_bytes(history_key(company, agent, revision))?
            .ok_or_else(inconsistent)?;
        let seal: Seal = decode(&bytes)?;
        seal.validate(company, agent)?;
        if seal.receipt.profile.revision != revision
            || self
                .profile_bytes(profile_key(
                    RECEIPT,
                    company,
                    &seal.receipt.command.idempotency_key,
                ))?
                .as_deref()
                != Some(bytes.as_slice())
        {
            return Err(inconsistent());
        }
        Ok(seal)
    }
    fn agent_profile_current(
        &self,
        company: &str,
        agent: &str,
    ) -> Result<Option<AgentDisplayProfile>> {
        let Some(bytes) = self.profile_bytes(profile_key(PROFILE, company, agent))? else {
            if self
                .profile_bytes(history_key(company, agent, 1))?
                .is_some()
            {
                return Err(inconsistent());
            }
            return Ok(None);
        };
        let tip: Tip = decode(&bytes)?;
        let seal = self.profile_seal(company, agent, tip.revision)?;
        if tip.digest != seal.digest {
            return Err(inconsistent());
        }
        if let Some(next) = tip.revision.checked_add(1) {
            if self
                .profile_bytes(history_key(company, agent, next))?
                .is_some()
            {
                return Err(inconsistent());
            }
        }
        Ok(Some(seal.receipt.profile))
    }
    /// Trusted administrative read; HTTP derives authority and company first.
    pub fn named_agent(&self, company: &str, agent: &str) -> Result<Option<NamedAgent>> {
        valid_id(company)?;
        valid_id(agent)?;
        let _guard = self
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Memory mutation lock poisoned".into()))?;
        self.named_agent_locked(company, agent)
    }
    fn named_agent_locked(&self, company: &str, agent: &str) -> Result<Option<NamedAgent>> {
        let Some(registration) = self.observed_agent(company, agent)? else {
            return Ok(None);
        };
        Ok(Some(NamedAgent {
            registration,
            profile: self.agent_profile_current(company, agent)?,
        }))
    }
    /// Atomic compare-and-set, history and replay receipt. This does not register agents.
    pub fn apply_agent_profile_command(
        &self,
        company: &str,
        author: &RecordAuthor,
        command: &AgentProfileCommand,
    ) -> Result<AgentProfileOutcome> {
        valid_id(company)?;
        valid_id(&author.subject_id)?;
        command.validate()?;
        let _guard = self
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Memory mutation lock poisoned".into()))?;
        let observed = self
            .named_agent_locked(company, &command.agent_id)?
            .ok_or_else(|| {
                Error::KeyNotFound("Agent has no successful authorized registration".into())
            })?;
        let receipt_key = profile_key(RECEIPT, company, &command.idempotency_key);
        if let Some(bytes) = self.profile_bytes(receipt_key.clone())? {
            let previous: Seal = decode(&bytes)?;
            previous.validate(company, &previous.receipt.command.agent_id)?;
            if previous.receipt.command != *command
                || previous.receipt.profile.updated_by != *author
            {
                return Err(Error::ConstraintViolation(
                    "Agent profile command identity was reused".into(),
                ));
            }
            let verified = self.profile_seal(
                company,
                &command.agent_id,
                previous.receipt.profile.revision,
            )?;
            if verified.digest != previous.digest
                || observed
                    .profile
                    .as_ref()
                    .is_none_or(|p| p.revision < previous.receipt.profile.revision)
            {
                return Err(inconsistent());
            }
            return Ok(AgentProfileOutcome {
                receipt: previous.receipt,
                replayed: true,
            });
        }
        let current = observed.profile.as_ref().map_or(0, |p| p.revision);
        if current != command.expected_revision {
            return Err(Error::TransactionConflict(
                "Agent profile revision changed".into(),
            ));
        }
        let revision = current.checked_add(1).ok_or_else(inconsistent)?;
        let profile = AgentDisplayProfile {
            schema_version: 1,
            company_id: company.into(),
            agent_id: command.agent_id.clone(),
            revision,
            display_name: command.display_name.clone(),
            updated_at_millis: chrono::Utc::now().timestamp_millis(),
            updated_by: author.clone(),
        };
        let receipt = AgentProfileReceipt {
            command: command.clone(),
            profile,
        };
        let seal = Seal::new(receipt.clone())?;
        let bytes = encode(&seal)?;
        let cf = self.cf(super::super::super::cf::AGENT_META)?;
        let mut batch = rocksdb::WriteBatch::default();
        batch.put_cf(
            cf,
            profile_key(PROFILE, company, &command.agent_id),
            encode(&Tip {
                revision,
                digest: seal.digest,
            })?,
        );
        batch.put_cf(
            cf,
            history_key(company, &command.agent_id, revision),
            &bytes,
        );
        batch.put_cf(cf, receipt_key, &bytes);
        let mut options = rocksdb::WriteOptions::default();
        options.disable_wal(false);
        options.set_sync(true);
        self.db.write_opt(batch, &options).map_err(storage_error)?;
        Ok(AgentProfileOutcome {
            receipt,
            replayed: false,
        })
    }
    pub fn agent_profile_history(
        &self,
        company: &str,
        agent: &str,
        after: u64,
        limit: usize,
    ) -> Result<AgentProfileHistory> {
        valid_id(company)?;
        valid_id(agent)?;
        if !(1..=100).contains(&limit) {
            return Err(Error::ValidationError("History limit must be 1–100".into()));
        }
        let _guard = self
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Memory mutation lock poisoned".into()))?;
        let observed = self
            .named_agent_locked(company, agent)?
            .ok_or_else(|| Error::KeyNotFound("Unknown registered agent".into()))?;
        let current_revision = observed.profile.map_or(0, |p| p.revision);
        if after > current_revision {
            return Err(Error::TransactionConflict(
                "History continuation is ahead of the current profile".into(),
            ));
        }
        if after > 0 {
            self.profile_seal(company, agent, after)?;
        }
        let end = current_revision.min(after.saturating_add(limit as u64));
        let mut receipts = vec![];
        for revision in after.saturating_add(1)..=end {
            if revision <= after {
                break;
            }
            receipts.push(self.profile_seal(company, agent, revision)?.receipt);
        }
        Ok(AgentProfileHistory {
            receipts,
            next_after_revision: (end < current_revision).then_some(end),
            current_revision,
        })
    }
    pub fn query_named_agents(
        &self,
        company: &str,
        query: &AgentDirectoryQuery,
    ) -> Result<NamedAgentPage> {
        valid_id(company)?;
        if !(1..=100).contains(&query.limit) || !(1..=1000).contains(&query.max_scanned_agents) {
            return Err(Error::ValidationError(
                "Agent page limit must be 1–100 and scan budget 1–1000".into(),
            ));
        }
        if let Some(text) = &query.text {
            valid_id(text)?;
        }
        let filter_digest: String = digest(&encode(&query.text)?)
            .iter()
            .map(|v| format!("{v:02x}"))
            .collect();
        if let Some(cursor) = &query.cursor {
            valid_id(&cursor.after_agent_id)?;
            if cursor.version != 1
                || cursor.company_id != company
                || cursor.filter_digest != filter_digest
            {
                return Err(Error::ValidationError(
                    "Agent cursor does not match company and filter".into(),
                ));
            }
        }
        let _guard = self
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Memory mutation lock poisoned".into()))?;
        let after = query.cursor.as_ref().map(|c| c.after_agent_id.as_str());
        let prefix = record_prefix(AGENT, company);
        let start = after
            .map(|a| key(company, a))
            .unwrap_or_else(|| prefix.clone());
        let mut page = NamedAgentPage {
            agents: vec![],
            next_cursor: None,
            scanned_agents: 0,
            observed_at_millis: chrono::Utc::now().timestamp_millis(),
        };
        let text = query.text.as_ref().map(|s| s.to_lowercase());
        let mut last: Option<String> = None;
        for row in self.db.iterator_cf(
            self.cf(super::super::super::cf::AGENT_META)?,
            rocksdb::IteratorMode::From(&start, rocksdb::Direction::Forward),
        ) {
            let (bytes, _) = row.map_err(storage_error)?;
            if !bytes.starts_with(&prefix) {
                break;
            }
            let agent = std::str::from_utf8(&bytes[prefix.len()..]).map_err(|_| inconsistent())?;
            if after.is_some_and(|a| agent.as_bytes() <= a.as_bytes()) {
                continue;
            }
            if page.agents.len() == query.limit || page.scanned_agents == query.max_scanned_agents {
                page.next_cursor = Some(AgentDirectoryCursor {
                    version: 1,
                    company_id: company.into(),
                    filter_digest,
                    after_agent_id: last.ok_or_else(inconsistent)?,
                });
                break;
            }
            let record = self
                .named_agent_locked(company, agent)?
                .ok_or_else(inconsistent)?;
            page.scanned_agents += 1;
            last = Some(agent.into());
            if text.as_ref().is_none_or(|text| {
                agent.to_lowercase().contains(text)
                    || record
                        .profile
                        .as_ref()
                        .and_then(|p| p.display_name.as_ref())
                        .is_some_and(|n| n.to_lowercase().contains(text))
            }) {
                page.agents.push(record);
            }
        }
        Ok(page)
    }
}
#[cfg(test)]
mod tests;
