//! ReplicatedShardActor - Actor-based replicated shard operations
//!
//! This actor owns a ReplicatedShard exclusively, eliminating Arc<RwLock<>>.
//!
//! ## Design (TigerBeetle/FoundationDB inspired)
//!
//! ```text
//! ┌─────────────────────────┐        ┌─────────────────────────┐
//! │ ReplicatedShardedState  │──msg──▶│  ReplicatedShardActor   │
//! │  (coordinator)          │        │  (owns shard state)     │
//! └─────────────────────────┘        └─────────────────────────┘
//! ```

use crate::redis::{Command, CommandExecutor, RespValue};
use crate::replication::{
    ConsistencyLevel, ReplicaId, ReplicationDelta,
};
use crate::replication::state::ShardReplicaState;
use crate::simulator::VirtualTime;
use tokio::sync::{mpsc, oneshot};

/// Messages for controlling the ReplicatedShardActor
#[derive(Debug)]
pub enum ReplicatedShardMessage {
    /// Execute a command and return result with optional delta
    Execute {
        cmd: Command,
        response: oneshot::Sender<(RespValue, Option<ReplicationDelta>)>,
    },
    /// Execute a read-only command (no delta generation)
    ExecuteReadonly {
        cmd: Command,
        response: oneshot::Sender<RespValue>,
    },
    /// Apply a remote delta from another replica
    ApplyRemoteDelta {
        delta: ReplicationDelta,
    },
    /// Drain all pending deltas for replication
    DrainPendingDeltas {
        response: oneshot::Sender<Vec<ReplicationDelta>>,
    },
    /// Evict expired keys
    EvictExpired {
        current_time: VirtualTime,
        response: oneshot::Sender<usize>,
    },
    /// Get snapshot of replicated keys for checkpointing
    GetSnapshot {
        response: oneshot::Sender<std::collections::HashMap<String, crate::replication::state::ReplicatedValue>>,
    },
    /// Apply recovered state from persistence
    ApplyRecoveredState {
        key: String,
        value: crate::replication::state::ReplicatedValue,
    },
    /// Graceful shutdown
    Shutdown {
        response: oneshot::Sender<()>,
    },
}

/// Handle for communicating with the ReplicatedShardActor
#[derive(Clone)]
pub struct ReplicatedShardHandle {
    tx: mpsc::UnboundedSender<ReplicatedShardMessage>,
    shard_id: usize,
}

impl ReplicatedShardHandle {
    /// Execute a command and return result with optional delta
    #[inline]
    pub async fn execute(&self, cmd: Command) -> (RespValue, Option<ReplicationDelta>) {
        let (tx, rx) = oneshot::channel();
        if self.tx.send(ReplicatedShardMessage::Execute { cmd, response: tx }).is_err() {
            return (RespValue::Error("ERR shard unavailable".to_string()), None);
        }
        rx.await.unwrap_or_else(|_| {
            (RespValue::Error("ERR shard response failed".to_string()), None)
        })
    }

    /// Execute a read-only command
    #[inline]
    pub async fn execute_readonly(&self, cmd: Command) -> RespValue {
        let (tx, rx) = oneshot::channel();
        if self.tx.send(ReplicatedShardMessage::ExecuteReadonly { cmd, response: tx }).is_err() {
            return RespValue::Error("ERR shard unavailable".to_string());
        }
        rx.await.unwrap_or_else(|_| {
            RespValue::Error("ERR shard response failed".to_string())
        })
    }

    /// Apply a remote delta (fire-and-forget)
    #[inline]
    pub fn apply_remote_delta(&self, delta: ReplicationDelta) {
        let _ = self.tx.send(ReplicatedShardMessage::ApplyRemoteDelta { delta });
    }

    /// Drain pending deltas for replication
    pub async fn drain_pending_deltas(&self) -> Vec<ReplicationDelta> {
        let (tx, rx) = oneshot::channel();
        if self.tx.send(ReplicatedShardMessage::DrainPendingDeltas { response: tx }).is_err() {
            return Vec::new();
        }
        rx.await.unwrap_or_default()
    }

    /// Evict expired keys
    pub async fn evict_expired(&self, current_time: VirtualTime) -> usize {
        let (tx, rx) = oneshot::channel();
        if self.tx.send(ReplicatedShardMessage::EvictExpired { current_time, response: tx }).is_err() {
            return 0;
        }
        rx.await.unwrap_or(0)
    }

    /// Get snapshot for checkpointing
    pub async fn get_snapshot(&self) -> std::collections::HashMap<String, crate::replication::state::ReplicatedValue> {
        let (tx, rx) = oneshot::channel();
        if self.tx.send(ReplicatedShardMessage::GetSnapshot { response: tx }).is_err() {
            return std::collections::HashMap::new();
        }
        rx.await.unwrap_or_default()
    }

    /// Apply recovered state (fire-and-forget)
    pub fn apply_recovered_state(&self, key: String, value: crate::replication::state::ReplicatedValue) {
        let _ = self.tx.send(ReplicatedShardMessage::ApplyRecoveredState { key, value });
    }

    /// Graceful shutdown
    pub async fn shutdown(&self) {
        let (tx, rx) = oneshot::channel();
        if self.tx.send(ReplicatedShardMessage::Shutdown { response: tx }).is_ok() {
            let _ = rx.await;
        }
    }

    /// Get the shard ID
    pub fn shard_id(&self) -> usize {
        self.shard_id
    }

    /// Check if the actor is still running
    pub fn is_running(&self) -> bool {
        !self.tx.is_closed()
    }
}

/// The ReplicatedShardActor owns its state exclusively - no Arc<RwLock<>> needed!
pub struct ReplicatedShardActor {
    executor: CommandExecutor,
    replica_state: ShardReplicaState,
    rx: mpsc::UnboundedReceiver<ReplicatedShardMessage>,
    shard_id: usize,
}

impl ReplicatedShardActor {
    /// Spawn a new ReplicatedShardActor and return the handle
    pub fn spawn(
        replica_id: ReplicaId,
        consistency_level: ConsistencyLevel,
        shard_id: usize,
    ) -> ReplicatedShardHandle {
        let (tx, rx) = mpsc::unbounded_channel();
        let actor = ReplicatedShardActor {
            executor: CommandExecutor::new(),
            replica_state: ShardReplicaState::new(replica_id, consistency_level),
            rx,
            shard_id,
        };

        tokio::spawn(actor.run());

        ReplicatedShardHandle { tx, shard_id }
    }

    /// Run the actor's message loop
    async fn run(mut self) {
        while let Some(msg) = self.rx.recv().await {
            match msg {
                ReplicatedShardMessage::Execute { cmd, response } => {
                    let result = self.executor.execute(&cmd);
                    let delta = self.record_mutation_post_execute(&cmd);
                    let _ = response.send((result, delta));

                    #[cfg(debug_assertions)]
                    self.verify_invariants();
                }

                ReplicatedShardMessage::ExecuteReadonly { cmd, response } => {
                    let result = self.executor.execute_readonly(&cmd);
                    let _ = response.send(result);
                }

                ReplicatedShardMessage::ApplyRemoteDelta { delta } => {
                    self.apply_remote_delta_impl(delta);

                    #[cfg(debug_assertions)]
                    self.verify_invariants();
                }

                ReplicatedShardMessage::DrainPendingDeltas { response } => {
                    let deltas = self.replica_state.drain_pending_deltas();
                    let _ = response.send(deltas);
                }

                ReplicatedShardMessage::EvictExpired { current_time, response } => {
                    let evicted = self.executor.evict_expired_direct(current_time);
                    let _ = response.send(evicted);
                }

                ReplicatedShardMessage::GetSnapshot { response } => {
                    let snapshot = self.replica_state.replicated_keys.clone();
                    let _ = response.send(snapshot);
                }

                ReplicatedShardMessage::ApplyRecoveredState { key, value } => {
                    // Insert into replica state
                    self.replica_state.replicated_keys.insert(key.clone(), value.clone());

                    // Also apply to executor for command execution
                    if let Some(v) = value.get() {
                        if let Some(expiry_ms) = value.expiry_ms {
                            let seconds = (expiry_ms / 1000) as i64;
                            let cmd = Command::SetEx(key, seconds, v.clone());
                            self.executor.execute(&cmd);
                        } else {
                            let cmd = Command::Set(key, v.clone());
                            self.executor.execute(&cmd);
                        }
                    }
                }

                ReplicatedShardMessage::Shutdown { response } => {
                    let _ = response.send(());
                    break;
                }
            }
        }
    }

    /// Record mutation after command execution
    fn record_mutation_post_execute(&mut self, cmd: &Command) -> Option<ReplicationDelta> {
        match cmd {
            Command::Set(key, value) => {
                Some(self.replica_state.record_write(key.clone(), value.clone(), None))
            }
            Command::SetEx(key, seconds, value) => {
                let expiry_ms = (*seconds as u64) * 1000;
                Some(self.replica_state.record_write(key.clone(), value.clone(), Some(expiry_ms)))
            }
            Command::SetNx(key, value) => {
                if let Some(v) = self.executor.get_data().get(key) {
                    if v.as_string().is_some() {
                        return Some(self.replica_state.record_write(key.clone(), value.clone(), None));
                    }
                }
                None
            }
            Command::Del(key) => {
                self.replica_state.record_delete(key.clone())
            }
            Command::Incr(key) | Command::Decr(key) |
            Command::IncrBy(key, _) | Command::DecrBy(key, _) |
            Command::Append(key, _) | Command::GetSet(key, _) => {
                if let Some(value) = self.executor.get_data().get(key) {
                    if let Some(sds) = value.as_string() {
                        return Some(self.replica_state.record_write(key.clone(), sds.clone(), None));
                    }
                }
                None
            }
            Command::FlushDb | Command::FlushAll => {
                None
            }
            _ => None,
        }
    }

    /// Apply a remote delta from another replica
    fn apply_remote_delta_impl(&mut self, delta: ReplicationDelta) {
        self.replica_state.apply_remote_delta(delta.clone());

        if let Some(value) = delta.value.get() {
            if let Some(expiry_ms) = delta.value.expiry_ms {
                let seconds = (expiry_ms / 1000) as i64;
                let cmd = Command::SetEx(delta.key.clone(), seconds, value.clone());
                self.executor.execute(&cmd);
            } else {
                let cmd = Command::Set(delta.key.clone(), value.clone());
                self.executor.execute(&cmd);
            }
        } else if delta.value.is_tombstone() {
            let cmd = Command::Del(delta.key.clone());
            self.executor.execute(&cmd);
        }
    }

    /// Verify invariants (VOPR pattern)
    #[cfg(debug_assertions)]
    fn verify_invariants(&self) {
        // Invariant: Keys in executor data should be consistent with replica state
        // (Not strictly enforced as executor may have non-replicated keys)
        debug_assert!(
            self.shard_id < 256,
            "Invariant: shard_id {} seems unreasonably high",
            self.shard_id
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replication::ConsistencyLevel;

    #[tokio::test]
    async fn test_replicated_shard_actor_basic() {
        let handle = ReplicatedShardActor::spawn(
            ReplicaId::new(1),
            ConsistencyLevel::Eventual,
            0,
        );

        // Execute SET
        let (result, delta) = handle.execute(Command::Set(
            "key1".to_string(),
            crate::redis::SDS::from_str("value1"),
        )).await;

        assert!(matches!(result, RespValue::SimpleString(_)));
        assert!(delta.is_some());

        // Execute GET
        let result = handle.execute_readonly(Command::Get("key1".to_string())).await;
        assert!(matches!(result, RespValue::BulkString(Some(_))));

        handle.shutdown().await;
    }

    #[tokio::test]
    async fn test_replicated_shard_actor_drain_deltas() {
        let handle = ReplicatedShardActor::spawn(
            ReplicaId::new(1),
            ConsistencyLevel::Eventual,
            0,
        );

        // Execute some writes
        handle.execute(Command::Set("k1".to_string(), crate::redis::SDS::from_str("v1"))).await;
        handle.execute(Command::Set("k2".to_string(), crate::redis::SDS::from_str("v2"))).await;

        // Drain deltas
        let deltas = handle.drain_pending_deltas().await;
        assert_eq!(deltas.len(), 2);

        // Drain again should be empty
        let deltas = handle.drain_pending_deltas().await;
        assert!(deltas.is_empty());

        handle.shutdown().await;
    }

    #[tokio::test]
    async fn test_replicated_shard_actor_apply_remote_delta() {
        let handle = ReplicatedShardActor::spawn(
            ReplicaId::new(1),
            ConsistencyLevel::Eventual,
            0,
        );

        // Create a delta as if from another replica
        let remote_replica = ReplicaId::new(2);
        let mut clock = crate::replication::lattice::LamportClock::new(remote_replica);
        clock.time = 100; // Advance clock to simulate remote write
        let delta = ReplicationDelta::new(
            "remote_key".to_string(),
            crate::replication::state::ReplicatedValue::with_value(
                crate::redis::SDS::from_str("remote_value"),
                clock,
            ),
            remote_replica,
        );

        // Apply it
        handle.apply_remote_delta(delta);

        // Small delay for message processing
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        // Verify it's there
        let result = handle.execute_readonly(Command::Get("remote_key".to_string())).await;
        assert!(matches!(result, RespValue::BulkString(Some(_))));

        handle.shutdown().await;
    }

    #[tokio::test]
    async fn test_replicated_shard_actor_snapshot() {
        let handle = ReplicatedShardActor::spawn(
            ReplicaId::new(1),
            ConsistencyLevel::Eventual,
            0,
        );

        // Execute some writes
        handle.execute(Command::Set("k1".to_string(), crate::redis::SDS::from_str("v1"))).await;
        handle.execute(Command::Set("k2".to_string(), crate::redis::SDS::from_str("v2"))).await;

        // Get snapshot
        let snapshot = handle.get_snapshot().await;
        assert_eq!(snapshot.len(), 2);
        assert!(snapshot.contains_key("k1"));
        assert!(snapshot.contains_key("k2"));

        handle.shutdown().await;
    }

    #[tokio::test]
    async fn test_replicated_shard_actor_shutdown() {
        let handle = ReplicatedShardActor::spawn(
            ReplicaId::new(1),
            ConsistencyLevel::Eventual,
            0,
        );

        assert!(handle.is_running());
        handle.shutdown().await;

        // Give it a moment
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        assert!(!handle.is_running());
    }
}
