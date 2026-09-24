//! Evidence-driven procedural memory. See `docs/agent-memory/learning.md`.

mod store;
mod types;
pub use store::LearningMemory;
pub use types::*;

#[cfg(test)]
mod tests;

pub use store::admission::*;
pub use store::bound::*;
pub use store::registry::*;

#[cfg(test)]
mod tool_tests;
pub use store::development::*;
pub use store::executors::*;
pub use store::experience::*;
pub use store::tools::*;

#[cfg(test)]
mod experience_tests;

pub use store::experience_history::*;

pub use store::experience_artifacts::*;

pub use store::experience_lineage::*;

pub use store::catalog::*;
pub use store::experience_export::*;
pub use store::strategies::*;

pub use store::knowledge::*;

pub use store::knowledge_selection::*;

pub use store::knowledge_origin::{CombinedKnowledgeInput, CombinedKnowledgeProposal, CombinedKnowledgeReceipt, KnowledgeOriginDescriptor};

pub use store::verification::KnowledgeIndexVerification;
