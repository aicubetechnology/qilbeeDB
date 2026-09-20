//! Bounded history scans over an immutable revision fence.
use super::{experience::*, *};
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperienceHistoryCursor {
    pub receipt_digest: String,
    pub through_event_id: Option<String>,
    pub after_event_id: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperienceHistoryQuery {
    pub limit: usize,
    pub cursor: Option<ExperienceHistoryCursor>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperienceHistoryPage {
    pub receipt_digest: String,
    pub through_revision: u64,
    pub events: Vec<ExperienceEvent>,
    pub scanned_events: usize,
    pub next_cursor: Option<ExperienceHistoryCursor>,
    pub complete: bool,
}
impl LearningMemory {
    /// A logical revision snapshot, in storage-key order, with bounded candidate scans.
    pub fn experience_history(
        &self,
        tenant: &str,
        namespace: &str,
        id: &str,
        query: ExperienceHistoryQuery,
    ) -> Result<ExperienceHistoryPage> {
        if !(1..=64).contains(&query.limit) {
            return Err(Error::ValidationError(
                "History limit must be 1 to 64".into(),
            ));
        }
        let record = self
            .experience(tenant, namespace, id)?
            .ok_or_else(|| Error::KeyNotFound("Experience attempt".into()))?;
        let through = match &query.cursor {
            Some(cursor) => {
                if cursor.receipt_digest != record.receipt.receipt_digest {
                    return Err(Error::ConstraintViolation(
                        "History cursor belongs to another receipt".into(),
                    ));
                }
                self.experience_event(tenant, namespace, id, &cursor.after_event_id)?
                    .ok_or_else(|| Error::ValidationError("Invalid history continuation".into()))?;
                cursor.through_event_id.clone()
            }
            None => record.last_event_id.clone(),
        };
        let revision = match &through {
            Some(event) => {
                self.experience_event(tenant, namespace, id, event)?
                    .ok_or_else(|| Error::ValidationError("Invalid history fence".into()))?
                    .record
                    .revision
            }
            None => 1,
        };
        let prefix = experience::key(13, tenant, namespace, id)?;
        let start = match &query.cursor {
            Some(cursor) => experience::event_key(tenant, namespace, id, &cursor.after_event_id)?,
            None => prefix.clone(),
        };
        let mut page = ExperienceHistoryPage {
            receipt_digest: record.receipt.receipt_digest.clone(),
            through_revision: revision,
            events: Vec::new(),
            scanned_events: 0,
            next_cursor: None,
            complete: true,
        };
        if revision == 1 {
            return Ok(page);
        }
        for entry in self
            .inner
            .db
            .iterator(IteratorMode::From(&start, Direction::Forward))
        {
            let (key, bytes) = entry.map_err(storage_error)?;
            if !key.starts_with(&prefix) {
                break;
            }
            if query.cursor.is_some() && key.as_ref() == start.as_slice() {
                continue;
            }
            let decoded: ExperienceEvent = decode(&bytes)?;
            let event_id = &decoded.command.event_id;
            if experience::event_key(tenant, namespace, id, event_id)?.as_slice() != key.as_ref() {
                return Err(Error::DataCorruption("History event key mismatch".into()));
            }
            let event = self
                .experience_event(tenant, namespace, id, event_id)?
                .ok_or_else(|| Error::DataCorruption("History event disappeared".into()))?;
            page.scanned_events += 1;
            page.next_cursor = Some(ExperienceHistoryCursor {
                receipt_digest: record.receipt.receipt_digest.clone(),
                through_event_id: through.clone(),
                after_event_id: event_id.clone(),
            });
            if event.record.revision <= revision {
                page.events.push(event);
            }
            if page.scanned_events == query.limit {
                page.complete = false;
                break;
            }
        }
        if page.complete {
            page.next_cursor = None;
        }
        Ok(page)
    }
}
