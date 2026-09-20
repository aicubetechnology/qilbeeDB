//! Evidence-driven procedural memory. See `docs/agent-memory/learning.md`.

mod store;
mod types;
pub use store::LearningMemory;
pub use types::*;

#[cfg(test)]
mod tests;

pub use store::registry::*;
pub use store::bound::*;
