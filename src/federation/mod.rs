pub mod client;
pub mod conflict;
pub mod partition;
pub mod propagation;
pub mod server;
pub mod state_sync;
pub mod trust;
pub mod types;

pub use client::FederationClient;
pub use conflict::{ConflictMediator, ConflictRecord, ConflictResolution};
pub use partition::{MergeStrategy, PartitionDetector, PartitionMerger, PartitionState, PeerState};
pub use propagation::{
    PropagationEngine, PropagationEnvelope, PropagationPayload, PropagationStrategy,
};
pub use server::FederationServer;
pub use state_sync::{StateDigest, StateSyncEngine, SyncMessage, VectorClock};
pub use trust::{TrustEdge, TrustGraph, TrustPolicy};
pub use types::*;
