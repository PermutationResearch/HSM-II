//! Honcho-inspired cross-session user inference and memory system for HSM-II.
//!
//! Implements five capabilities missing from the base personal agent:
//!
//! 1. **Async cross-session user inference** — a background worker reads every session
//!    transcript and continuously builds a psychological profile (communication style,
//!    goals, frustrations, preferences) without the agent explicitly asking.
//!
//! 2. **UserRepresentation / metamemory** — pre-synthesized insight documents about a
//!    specific peer, stored in the `EntitySummary` network of `HybridMemory` and ready
//!    for instant retrieval at session start.
//!
//! 3. **Token-budget-aware context endpoint** — packs messages + conclusions + peer
//!    summaries to fit exactly N tokens, suitable for constrained LLM context windows.
//!
//! 4. **Unified Peer abstraction** — `Peer` treats humans and AI agents as identical
//!    participants, bridging the gap between the `UserMd` model and agent reputation.
//!
//! 5. **Configurable session visibility** — per-session control of which participants
//!    see which messages, enabling private sub-conversations inside a session.
//!
//! ## Storage layout
//! ```text
//! ~/.hsmii/honcho/
//!   hybrid_memory.json      ← serialized HybridMemory with EntitySummary entries
//!   peers/
//!     <peer_id>.json        ← latest UserRepresentation per peer
//!   sessions/
//!     <session_id>.json     ← SessionVisibility config
//! ```

pub mod context_packer;
pub mod inference_worker;
pub mod peer;
pub mod session_visibility;
pub mod user_representation;

pub use context_packer::{ContextBudget, PackedContext, PackedContextBuilder};
pub use inference_worker::HonchoInferenceWorker;
pub use peer::{Peer, PeerKind};
pub use session_visibility::{SessionVisibility, VisibilityMatrix};
pub use user_representation::{
    InferredGoal, InferredPreference, InferredTrait, UserRepresentation,
};
