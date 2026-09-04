//! Cross-session memory learning: the consolidation pass that distills a
//! finished session into durable memory blocks, on the episodic/semantic split
//! from `docs/MEMORY.md`. `aster-cli` only wires the triggers.

pub mod consolidate;
pub mod digest;
