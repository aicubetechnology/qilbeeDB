//! Snapshot-local consumer observations; sequence distance is not a journal audit.
use super::*;
use crate::storage::platform::snapshot::MemorySnapshot;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsumerCheckpointStatus {
    Missing,
    Compatible,
    HistoryIncompatible,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsumerWitnessStatus {
    NotProvided,
    Compatible,
    HistoryIncompatible,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsumerCursorOrder {
    Before,
    Equal,
    After,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryConsumerDiagnostics {
    pub consumer_id: String,
    pub active: bool,
    pub baseline: Option<VerifiedMemoryCursor>,
    pub high_watermark: Option<VerifiedMemoryCursor>,
    pub checkpoint: Option<VerifiedMemoryCheckpoint>,
    pub checkpoint_status: ConsumerCheckpointStatus,
    /// Sequence distance only; null when progress cannot be compared to this history.
    pub pending_positions: Option<u64>,
    pub witness_status: ConsumerWitnessStatus,
    pub checkpoint_relative_to_witness: Option<ConsumerCursorOrder>,
}
impl RocksDbMemoryStorage {
    /// Caller authorizes both checkpoint ownership and memory reading in this namespace.
    pub fn diagnose_memory_consumer(
        &self,
        namespace: &str,
        subject: &str,
        consumer: &str,
        witness: Option<&VerifiedMemoryCursor>,
    ) -> Result<MemoryConsumerDiagnostics> {
        Self::validate_agent(namespace)?;
        if !valid_checkpoint_identity(subject, 256) || !valid_checkpoint_identity(consumer, 128) {
            return Err(Error::ValidationError(
                "Invalid checkpoint subject or consumer identifier".into(),
            ));
        }
        if let Some(cursor) = witness {
            cursor.validate()?;
        }
        self.memory_snapshot()
            .diagnose_consumer(namespace, subject, consumer, witness)
    }
}
impl MemorySnapshot<'_> {
    fn diagnose_consumer(
        &self,
        namespace: &str,
        subject: &str,
        consumer: &str,
        witness: Option<&VerifiedMemoryCursor>,
    ) -> Result<MemoryConsumerDiagnostics> {
        let checkpoint = self.stored_verified_checkpoint(namespace, subject, consumer)?;
        let journal = self.verified_journal(namespace)?;
        let compatible = |cursor: &VerifiedMemoryCursor| -> Result<bool> {
            let Some(journal) = &journal else {
                return Ok(false);
            };
            match self.verify_memory_cursor(namespace, journal, cursor) {
                Ok(()) => Ok(true),
                Err(Error::JournalHistoryConflict(_)) => Ok(false),
                Err(error) => Err(error),
            }
        };
        let checkpoint_status = match &checkpoint {
            None => ConsumerCheckpointStatus::Missing,
            Some(c) if compatible(&c.cursor)? => ConsumerCheckpointStatus::Compatible,
            Some(_) => ConsumerCheckpointStatus::HistoryIncompatible,
        };
        let witness_status = match witness {
            None => ConsumerWitnessStatus::NotProvided,
            Some(c) if compatible(c)? => ConsumerWitnessStatus::Compatible,
            Some(_) => ConsumerWitnessStatus::HistoryIncompatible,
        };
        let mut pending_positions = None;
        let mut order = None;
        if checkpoint_status == ConsumerCheckpointStatus::Compatible {
            let cursor = &checkpoint.as_ref().ok_or_else(inconsistent)?.cursor;
            let tip = &journal.as_ref().ok_or_else(inconsistent)?.tip;
            pending_positions = Some(
                tip.sequence
                    .checked_sub(cursor.sequence)
                    .ok_or_else(inconsistent)?,
            );
            if witness_status == ConsumerWitnessStatus::Compatible {
                order = Some(
                    match cursor
                        .sequence
                        .cmp(&witness.ok_or_else(inconsistent)?.sequence)
                    {
                        std::cmp::Ordering::Less => ConsumerCursorOrder::Before,
                        std::cmp::Ordering::Equal => ConsumerCursorOrder::Equal,
                        std::cmp::Ordering::Greater => ConsumerCursorOrder::After,
                    },
                );
            }
        }
        Ok(MemoryConsumerDiagnostics {
            consumer_id: consumer.into(),
            active: journal.is_some(),
            baseline: journal.as_ref().map(|j| j.baseline.clone()),
            high_watermark: journal.map(|j| j.tip),
            checkpoint,
            checkpoint_status,
            pending_positions,
            witness_status,
            checkpoint_relative_to_witness: order,
        })
    }
}

#[cfg(test)]
mod tests;
