//! Durable observations of externally identified agents, independent of clients.
use super::*;

const AGENT: u8 = 0x40;

/// Trusted input supplied after authentication, scope authorization and success.
/// This library does not authenticate callers or generate business identifiers.
#[derive(Debug, Clone)]
pub struct AgentObservation {
    pub company_id: String,
    pub agent_id: String,
    pub namespace: String,
    pub author: RecordAuthor,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentRegistrationTrigger {
    SuccessfulResourceRequest {},
    MemoryCommand {
        record_id: Uuid,
        revision: u64,
        action: String,
    },
}

impl AgentRegistrationTrigger {
    fn validate(&self) -> Result<()> {
        if let Self::MemoryCommand {
            revision, action, ..
        } = self
        {
            if *revision == 0
                || !matches!(
                    action.as_str(),
                    "created" | "derived" | "updated" | "deleted"
                )
            {
                return Err(inconsistent());
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedAgent {
    pub schema_version: u32,
    pub company_id: String,
    pub agent_id: String,
    pub registered_at_millis: i64,
    pub registered_by: RecordAuthor,
    pub first_namespace: String,
    pub trigger: AgentRegistrationTrigger,
}

#[derive(Debug, Serialize)]
pub struct ObservedAgentPage {
    pub agents: Vec<ObservedAgent>,
    pub next_after_agent_id: Option<String>,
}

fn valid_id(id: &str) -> Result<()> {
    if id.trim().is_empty() || id.len() > 256 || id.chars().any(char::is_control) {
        return Err(Error::ValidationError(
            "Agent observation IDs require 1–256 UTF-8 bytes without control characters".into(),
        ));
    }
    Ok(())
}
fn key(company: &str, agent: &str) -> Vec<u8> {
    let mut key = record_prefix(AGENT, company);
    key.extend_from_slice(agent.as_bytes());
    key
}

impl AgentObservation {
    pub(super) fn validate(&self) -> Result<()> {
        valid_id(&self.company_id)?;
        valid_id(&self.agent_id)?;
        valid_id(&self.author.subject_id)?;
        RocksDbMemoryStorage::validate_agent(&self.namespace)
    }
}

impl RocksDbMemoryStorage {
    pub fn observed_agent(&self, company: &str, agent: &str) -> Result<Option<ObservedAgent>> {
        valid_id(company)?;
        valid_id(agent)?;
        self.db
            .get_cf(self.cf(super::super::cf::AGENT_META)?, key(company, agent))
            .map_err(storage_error)?
            .map(|bytes| {
                let record: ObservedAgent = decode(&bytes)?;
                if record.schema_version != 1
                    || record.company_id != company
                    || record.agent_id != agent
                {
                    return Err(inconsistent());
                }
                AgentObservation {
                    company_id: record.company_id.clone(),
                    agent_id: record.agent_id.clone(),
                    namespace: record.first_namespace.clone(),
                    author: record.registered_by.clone(),
                }
                .validate()
                .map_err(|_| inconsistent())?;
                record.trigger.validate()?;
                Ok(record)
            })
            .transpose()
    }

    /// Called only while holding the memory mutation lock. The first observation
    /// is immutable; seeing an existing ID never transfers ownership or authority.
    pub(super) fn append_agent_observation(
        &self,
        observation: &AgentObservation,
        trigger: AgentRegistrationTrigger,
        at_millis: i64,
        batch: &mut rocksdb::WriteBatch,
    ) -> Result<()> {
        observation.validate()?;
        trigger.validate()?;
        if self
            .observed_agent(&observation.company_id, &observation.agent_id)?
            .is_some()
        {
            return Ok(());
        }
        let record = ObservedAgent {
            schema_version: 1,
            company_id: observation.company_id.clone(),
            agent_id: observation.agent_id.clone(),
            registered_at_millis: at_millis,
            registered_by: observation.author.clone(),
            first_namespace: observation.namespace.clone(),
            trigger,
        };
        batch.put_cf(
            self.cf(super::super::cf::AGENT_META)?,
            key(&record.company_id, &record.agent_id),
            encode(&record)?,
        );
        Ok(())
    }

    /// Register a successful resource observation before acknowledging its HTTP
    /// response. A read may therefore durably register a previously unseen agent.
    pub fn observe_agent(&self, observation: &AgentObservation) -> Result<()> {
        observation.validate()?;
        // Registrations are immutable. Existing agents do not need the global
        // mutation lock on every resource read; new agents recheck under it.
        if self
            .observed_agent(&observation.company_id, &observation.agent_id)?
            .is_some()
        {
            return Ok(());
        }
        let _guard = self
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Memory mutation lock poisoned".into()))?;
        let mut batch = rocksdb::WriteBatch::default();
        self.append_agent_observation(
            observation,
            AgentRegistrationTrigger::SuccessfulResourceRequest {},
            chrono::Utc::now().timestamp_millis(),
            &mut batch,
        )?;
        if batch.len() == 0 {
            return Ok(());
        }
        let mut options = rocksdb::WriteOptions::default();
        options.disable_wal(false);
        options.set_sync(true);
        self.db.write_opt(batch, &options).map_err(storage_error)
    }

    /// Ordered, bounded company-prefix traversal; the caller authenticates the
    /// company and administrative authority before invoking this trusted API.
    pub fn list_observed_agents(
        &self,
        company: &str,
        after: Option<&str>,
        limit: usize,
    ) -> Result<ObservedAgentPage> {
        valid_id(company)?;
        if let Some(after) = after {
            valid_id(after)?;
        }
        if !(1..=100).contains(&limit) {
            return Err(Error::ValidationError(
                "Agent directory limit must be 1–100".into(),
            ));
        }
        let prefix = record_prefix(AGENT, company);
        let start = after
            .map(|agent| key(company, agent))
            .unwrap_or_else(|| prefix.clone());
        let mut agents = vec![];
        let mut more = false;
        for row in self.db.iterator_cf(
            self.cf(super::super::cf::AGENT_META)?,
            rocksdb::IteratorMode::From(&start, rocksdb::Direction::Forward),
        ) {
            let (key, _) = row.map_err(storage_error)?;
            if !key.starts_with(&prefix) {
                break;
            }
            let agent = std::str::from_utf8(&key[prefix.len()..]).map_err(|_| inconsistent())?;
            if after.is_some_and(|after| agent.as_bytes() <= after.as_bytes()) {
                continue;
            }
            if agents.len() == limit {
                more = true;
                break;
            }
            agents.push(
                self.observed_agent(company, agent)?
                    .ok_or_else(inconsistent)?,
            );
        }
        Ok(ObservedAgentPage {
            next_after_agent_id: if more {
                agents.last().map(|agent| agent.agent_id.clone())
            } else {
                None
            },
            agents,
        })
    }
}

#[cfg(test)]
mod tests;

mod profiles;
pub use profiles::*;
