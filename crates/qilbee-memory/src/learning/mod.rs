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
pub use store::tools::*;
