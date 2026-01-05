//! GossipActor - Actor-based gossip state management
//!
//! This actor owns the GossipState exclusively, eliminating the need for
//! Arc<RwLock<GossipState>>. All interactions happen through message passing.
//!
//! ## Design (TigerBeetle/FoundationDB inspired)
//!
//! ```text
//! ┌─────────────────┐        ┌─────────────────┐
//! │  ReplicatedState │──msg──▶│   GossipActor   │
//! └─────────────────┘        │ (owns GossipState)│
//!                            └─────────────────┘
//!                                     │
//!                                     ▼
//!                            ┌─────────────────┐
//!                            │  GossipManager  │──network──▶ peers
//!                            └─────────────────┘
//! ```

use crate::replication::config::ReplicationConfig;
use crate::replication::gossip::{GossipState, RoutedMessage};
use crate::replication::gossip_router::GossipRouter;
use crate::replication::state::ReplicationDelta;
use tokio::sync::{mpsc, oneshot};

/// Messages that can be sent to the GossipActor
#[derive(Debug)]
pub enum GossipMessage {
    /// Queue deltas for gossip to peers
    QueueDeltas(Vec<ReplicationDelta>),

    /// Queue deltas using broadcast mode (ignore router)
    QueueDeltasBroadcast(Vec<ReplicationDelta>),

    /// Queue a heartbeat message
    QueueHeartbeat,

    /// Advance the epoch counter
    AdvanceEpoch,

    /// Drain all outbound messages
    DrainOutbound {
        response: oneshot::Sender<Vec<RoutedMessage>>,
    },

    /// Set or update the gossip router
    SetRouter(GossipRouter),

    /// Check if selective gossip is active
    IsSelective {
        response: oneshot::Sender<bool>,
    },

    /// Get current epoch
    GetEpoch {
        response: oneshot::Sender<u64>,
    },

    /// Graceful shutdown
    Shutdown {
        response: oneshot::Sender<()>,
    },
}

/// Handle for communicating with the GossipActor
#[derive(Clone)]
pub struct GossipActorHandle {
    tx: mpsc::UnboundedSender<GossipMessage>,
}

impl GossipActorHandle {
    /// Create a new handle from a sender
    pub fn new(tx: mpsc::UnboundedSender<GossipMessage>) -> Self {
        GossipActorHandle { tx }
    }

    /// Queue deltas for gossip
    #[inline]
    pub fn queue_deltas(&self, deltas: Vec<ReplicationDelta>) {
        if !deltas.is_empty() {
            let _ = self.tx.send(GossipMessage::QueueDeltas(deltas));
        }
    }

    /// Queue deltas using broadcast mode
    #[inline]
    pub fn queue_deltas_broadcast(&self, deltas: Vec<ReplicationDelta>) {
        if !deltas.is_empty() {
            let _ = self.tx.send(GossipMessage::QueueDeltasBroadcast(deltas));
        }
    }

    /// Queue a heartbeat message
    #[inline]
    pub fn queue_heartbeat(&self) {
        let _ = self.tx.send(GossipMessage::QueueHeartbeat);
    }

    /// Advance the epoch counter
    #[inline]
    pub fn advance_epoch(&self) {
        let _ = self.tx.send(GossipMessage::AdvanceEpoch);
    }

    /// Drain all outbound messages (blocking)
    pub async fn drain_outbound(&self) -> Vec<RoutedMessage> {
        let (tx, rx) = oneshot::channel();
        if self.tx.send(GossipMessage::DrainOutbound { response: tx }).is_err() {
            return Vec::new();
        }
        rx.await.unwrap_or_default()
    }

    /// Set or update the gossip router
    pub fn set_router(&self, router: GossipRouter) {
        let _ = self.tx.send(GossipMessage::SetRouter(router));
    }

    /// Check if selective gossip is active
    pub async fn is_selective(&self) -> bool {
        let (tx, rx) = oneshot::channel();
        if self.tx.send(GossipMessage::IsSelective { response: tx }).is_err() {
            return false;
        }
        rx.await.unwrap_or(false)
    }

    /// Get current epoch
    pub async fn get_epoch(&self) -> u64 {
        let (tx, rx) = oneshot::channel();
        if self.tx.send(GossipMessage::GetEpoch { response: tx }).is_err() {
            return 0;
        }
        rx.await.unwrap_or(0)
    }

    /// Graceful shutdown
    pub async fn shutdown(&self) {
        let (tx, rx) = oneshot::channel();
        if self.tx.send(GossipMessage::Shutdown { response: tx }).is_ok() {
            let _ = rx.await;
        }
    }
}

/// The GossipActor owns GossipState exclusively
pub struct GossipActor {
    /// The owned state - no Arc<RwLock<>> needed!
    state: GossipState,
    /// Message receiver
    rx: mpsc::UnboundedReceiver<GossipMessage>,
}

impl GossipActor {
    /// Create a new GossipActor and return the handle
    pub fn spawn(config: ReplicationConfig) -> GossipActorHandle {
        let (tx, rx) = mpsc::unbounded_channel();
        let state = GossipState::new(config);

        let actor = GossipActor { state, rx };

        tokio::spawn(async move {
            actor.run().await;
        });

        GossipActorHandle::new(tx)
    }

    /// Create a new GossipActor with a router
    pub fn spawn_with_router(config: ReplicationConfig, router: GossipRouter) -> GossipActorHandle {
        let (tx, rx) = mpsc::unbounded_channel();
        let state = GossipState::with_router(config, router);

        let actor = GossipActor { state, rx };

        tokio::spawn(async move {
            actor.run().await;
        });

        GossipActorHandle::new(tx)
    }

    /// Run the actor's message loop
    async fn run(mut self) {
        while let Some(msg) = self.rx.recv().await {
            match msg {
                GossipMessage::QueueDeltas(deltas) => {
                    self.state.queue_deltas(deltas);
                }

                GossipMessage::QueueDeltasBroadcast(deltas) => {
                    self.state.queue_deltas_broadcast(deltas);
                }

                GossipMessage::QueueHeartbeat => {
                    self.state.queue_heartbeat();
                }

                GossipMessage::AdvanceEpoch => {
                    self.state.advance_epoch();
                }

                GossipMessage::DrainOutbound { response } => {
                    let messages = self.state.drain_outbound();
                    let _ = response.send(messages);
                }

                GossipMessage::SetRouter(router) => {
                    self.state.set_router(router);
                }

                GossipMessage::IsSelective { response } => {
                    let _ = response.send(self.state.is_selective());
                }

                GossipMessage::GetEpoch { response } => {
                    let _ = response.send(self.state.epoch);
                }

                GossipMessage::Shutdown { response } => {
                    let _ = response.send(());
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replication::ReplicaId;

    fn test_config() -> ReplicationConfig {
        ReplicationConfig {
            replica_id: 1,
            peers: vec![],
            enabled: true,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn test_gossip_actor_basic() {
        let handle = GossipActor::spawn(test_config());

        // Advance epoch and check
        handle.advance_epoch();
        let epoch = handle.get_epoch().await;
        assert!(epoch >= 1);

        // Graceful shutdown
        handle.shutdown().await;
    }

    #[tokio::test]
    async fn test_gossip_actor_queue_deltas() {
        let handle = GossipActor::spawn(test_config());

        // Create a test delta
        use crate::replication::state::{ReplicatedValue, ReplicationDelta};
        use crate::replication::lattice::LamportClock;
        use crate::redis::SDS;

        let replica_id = ReplicaId::new(1);
        let clock = LamportClock::new(replica_id);
        let value = ReplicatedValue::with_value(SDS::from_str("test"), clock);
        let delta = ReplicationDelta::new("key1".to_string(), value, replica_id);

        // Queue deltas
        handle.queue_deltas(vec![delta]);

        // Drain and verify
        let messages = handle.drain_outbound().await;
        assert!(!messages.is_empty());

        handle.shutdown().await;
    }

    #[tokio::test]
    async fn test_gossip_actor_heartbeat() {
        let handle = GossipActor::spawn(test_config());

        handle.queue_heartbeat();

        let messages = handle.drain_outbound().await;
        assert!(!messages.is_empty());

        handle.shutdown().await;
    }

    #[tokio::test]
    async fn test_gossip_actor_is_selective() {
        let handle = GossipActor::spawn(test_config());

        // Without router, should not be selective
        let selective = handle.is_selective().await;
        assert!(!selective);

        handle.shutdown().await;
    }

    #[tokio::test]
    async fn test_gossip_actor_multiple_handles() {
        let handle1 = GossipActor::spawn(test_config());
        let handle2 = handle1.clone();

        // Both handles should work
        handle1.advance_epoch();
        handle2.advance_epoch();

        let epoch = handle1.get_epoch().await;
        assert!(epoch >= 2);

        handle1.shutdown().await;
    }
}
