//! Cross-session memory learning: the consolidation pass that distills a
//! finished session's transcript into durable memory, and the retrieval
//! layer that reads it back. This crate owns the learning loop; it depends on
//! `aster-persist` for the store and journal, and `aster-ai` for the bounded
//! model calls that power consolidation. `aster-cli` only wires the triggers.
//!
//! The learning model is layered on the episodic/semantic split from
//! `docs/MEMORY.md`: the transcript is episodic raw history, memory blocks are
//! the semantic store. Consolidation is the periodic transfer between them.

pub mod consolidate;
pub mod digest;
