//! Exact pre-accounting bincode field order; never deserialize old bytes as v2.
use crate::episode::{Episode, EpisodeContent, EpisodeId, EpisodeType};
use crate::types::Relevance;
use qilbee_core::{
    Property,
    temporal::{EventTime, TransactionTime},
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(super) struct LegacyRelevance {
    score: f64,
    access_count: u32,
    last_accessed: i64,
}

#[derive(Serialize, Deserialize)]
pub(super) struct LegacyEpisode {
    id: EpisodeId,
    agent_id: String,
    episode_type: EpisodeType,
    event_time: EventTime,
    transaction_time: TransactionTime,
    content: EpisodeContent,
    metadata: Property,
    relevance: LegacyRelevance,
    consolidated: bool,
    invalidated_at: Option<TransactionTime>,
}

impl From<LegacyEpisode> for Episode {
    fn from(old: LegacyEpisode) -> Self {
        Self {
            id: old.id,
            agent_id: old.agent_id,
            episode_type: old.episode_type,
            event_time: old.event_time,
            transaction_time: old.transaction_time,
            content: old.content,
            metadata: old.metadata,
            relevance: Relevance::from_legacy(
                old.relevance.score,
                old.relevance.access_count,
                old.relevance.last_accessed,
            ),
            consolidated: old.consolidated,
            invalidated_at: old.invalidated_at,
        }
    }
}
