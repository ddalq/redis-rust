use crate::io::{TimeSource, ProductionTimeSource};
use crate::redis::{Command, CommandExecutor, RespValue};
use crate::replication::{
    ReplicaId, ReplicationConfig, ConsistencyLevel,
    ReplicationDelta,
};
use crate::replication::state::ShardReplicaState;
use crate::replication::gossip::GossipState;
use crate::simulator::VirtualTime;
use crate::streaming::DeltaSinkSender;
use super::gossip_actor::GossipActorHandle;
use parking_lot::RwLock;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// Gossip backend for replication
///
/// Supports both legacy lock-based and new actor-based approaches.
/// The actor-based approach eliminates lock contention and follows
/// the actor model for better testability.
#[derive(Clone)]
pub enum GossipBackend {
    /// Legacy: Arc<RwLock<GossipState>> - shared mutable state with locks
    Locked(Arc<RwLock<GossipState>>),
    /// Actor-based: message passing, no locks (preferred)
    Actor(GossipActorHandle),
}

const NUM_SHARDS: usize = 16;

fn hash_key(key: &str) -> usize {
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    (hasher.finish() as usize) % NUM_SHARDS
}

pub struct ReplicatedShard {
    executor: CommandExecutor,
    replica_state: ShardReplicaState,
}

impl ReplicatedShard {
    pub fn new(replica_id: ReplicaId, consistency_level: ConsistencyLevel) -> Self {
        ReplicatedShard {
            executor: CommandExecutor::new(),
            replica_state: ShardReplicaState::new(replica_id, consistency_level),
        }
    }

    pub fn execute(&mut self, cmd: &Command) -> (RespValue, Option<ReplicationDelta>) {
        let result = self.executor.execute(cmd);
        let delta = self.record_mutation_post_execute(cmd);
        (result, delta)
    }

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

    pub fn apply_remote_delta(&mut self, delta: ReplicationDelta) {
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

    pub fn drain_pending_deltas(&mut self) -> Vec<ReplicationDelta> {
        self.replica_state.drain_pending_deltas()
    }

    pub fn evict_expired(&mut self, current_time: VirtualTime) -> usize {
        self.executor.evict_expired_direct(current_time)
    }
}

/// Replicated sharded state with configurable time source
///
/// Generic over `T: TimeSource` for zero-cost abstraction:
/// - Production: `ProductionTimeSource` (ZST, compiles to syscall)
/// - Simulation: `SimulatedTimeSource` (virtual clock)
///
/// ## Gossip Backend
///
/// Supports both legacy lock-based and actor-based gossip:
/// - `GossipBackend::Locked`: Arc<RwLock<GossipState>> - shared mutable state
/// - `GossipBackend::Actor`: GossipActorHandle - message passing (preferred)
///
/// Use `with_gossip_actor()` or `with_gossip_actor_and_time()` for actor-based.
pub struct ReplicatedShardedState<T: TimeSource = ProductionTimeSource> {
    shards: Vec<Arc<RwLock<ReplicatedShard>>>,
    config: ReplicationConfig,
    /// Gossip backend - supports both locked and actor-based modes
    gossip_backend: GossipBackend,
    /// Optional delta sink for streaming persistence
    delta_sink: Option<DeltaSinkSender>,
    /// Time source for getting current time
    time_source: T,
}

/// Production-specific constructors
impl ReplicatedShardedState<ProductionTimeSource> {
    /// Create with lock-based gossip (legacy)
    pub fn new(config: ReplicationConfig) -> Self {
        Self::with_time_source(config, ProductionTimeSource::new())
    }

    /// Create with actor-based gossip (preferred)
    ///
    /// This is the recommended constructor for production as it eliminates
    /// lock contention and follows the actor model.
    pub fn with_gossip_actor(config: ReplicationConfig, gossip_handle: GossipActorHandle) -> Self {
        Self::with_gossip_actor_and_time(config, gossip_handle, ProductionTimeSource::new())
    }
}

/// Generic implementation that works with any TimeSource
impl<T: TimeSource> ReplicatedShardedState<T> {
    /// Create with configuration and custom time source (lock-based gossip)
    pub fn with_time_source(config: ReplicationConfig, time_source: T) -> Self {
        let replica_id = ReplicaId::new(config.replica_id);
        let consistency_level = config.consistency_level;

        let shards = (0..NUM_SHARDS)
            .map(|_| Arc::new(RwLock::new(ReplicatedShard::new(replica_id, consistency_level))))
            .collect();

        let gossip_state = Arc::new(RwLock::new(GossipState::new(config.clone())));

        ReplicatedShardedState {
            shards,
            config,
            gossip_backend: GossipBackend::Locked(gossip_state),
            delta_sink: None,
            time_source,
        }
    }

    /// Create with configuration, custom time source, and actor-based gossip (preferred)
    ///
    /// This is the recommended constructor as it eliminates lock contention
    /// and follows the actor model for better testability.
    pub fn with_gossip_actor_and_time(
        config: ReplicationConfig,
        gossip_handle: GossipActorHandle,
        time_source: T,
    ) -> Self {
        let replica_id = ReplicaId::new(config.replica_id);
        let consistency_level = config.consistency_level;

        let shards = (0..NUM_SHARDS)
            .map(|_| Arc::new(RwLock::new(ReplicatedShard::new(replica_id, consistency_level))))
            .collect();

        ReplicatedShardedState {
            shards,
            config,
            gossip_backend: GossipBackend::Actor(gossip_handle),
            delta_sink: None,
            time_source,
        }
    }

    /// Set the delta sink for streaming persistence
    pub fn set_delta_sink(&mut self, sink: DeltaSinkSender) {
        self.delta_sink = Some(sink);
    }

    /// Clear the delta sink (for shutdown)
    pub fn clear_delta_sink(&mut self) {
        self.delta_sink = None;
    }

    /// Check if streaming persistence is enabled
    pub fn has_streaming_persistence(&self) -> bool {
        self.delta_sink.is_some()
    }

    pub fn execute(&self, cmd: Command) -> RespValue {
        if let Some(key) = cmd.get_primary_key() {
            let shard_idx = hash_key(&key);
            let mut shard = self.shards[shard_idx].write();
            let (result, delta) = shard.execute(&cmd);

            if let Some(delta) = delta {
                // Send to gossip for replication
                if self.config.enabled {
                    match &self.gossip_backend {
                        GossipBackend::Locked(gossip_state) => {
                            let mut gossip = gossip_state.write();
                            gossip.queue_deltas(vec![delta.clone()]);
                        }
                        GossipBackend::Actor(handle) => {
                            // Actor-based: fire-and-forget, no locks!
                            handle.queue_deltas(vec![delta.clone()]);
                        }
                    }
                }

                // Send to streaming persistence if enabled
                if let Some(ref sink) = self.delta_sink {
                    // Best-effort send - don't block or error on persistence failures
                    let _ = sink.send(delta);
                }
            }

            result
        } else {
            self.execute_global(cmd)
        }
    }

    fn execute_global(&self, cmd: Command) -> RespValue {
        match &cmd {
            Command::Ping => RespValue::SimpleString("PONG".to_string()),
            Command::FlushDb | Command::FlushAll => {
                for shard in &self.shards {
                    let mut s = shard.write();
                    s.executor.execute(&cmd);
                }
                RespValue::SimpleString("OK".to_string())
            }
            Command::MSet(pairs) => {
                for (key, value) in pairs {
                    let shard_idx = hash_key(key);
                    let mut shard = self.shards[shard_idx].write();
                    let set_cmd = Command::Set(key.clone(), value.clone());
                    shard.execute(&set_cmd);
                }
                RespValue::SimpleString("OK".to_string())
            }
            Command::MGet(keys) => {
                let mut results = Vec::new();
                for key in keys {
                    let shard_idx = hash_key(key);
                    let shard = self.shards[shard_idx].read();
                    let get_cmd = Command::Get(key.clone());
                    results.push(shard.executor.execute_readonly(&get_cmd));
                }
                RespValue::Array(Some(results))
            }
            Command::Exists(keys) => {
                let mut count = 0i64;
                for key in keys {
                    let shard_idx = hash_key(key);
                    let shard = self.shards[shard_idx].read();
                    let exists_cmd = Command::Exists(vec![key.clone()]);
                    if let RespValue::Integer(n) = shard.executor.execute_readonly(&exists_cmd) {
                        count += n;
                    }
                }
                RespValue::Integer(count)
            }
            Command::Keys(_pattern) => {
                let mut all_keys = Vec::new();
                for shard in &self.shards {
                    let s = shard.read();
                    if let RespValue::Array(Some(keys)) = s.executor.execute_readonly(&cmd) {
                        all_keys.extend(keys);
                    }
                }
                RespValue::Array(Some(all_keys))
            }
            Command::Info => {
                let info = format!(
                    "# Replication\r\nrole:master\r\nreplica_id:{}\r\nconsistency_level:{:?}\r\nreplication_enabled:{}\r\nnum_shards:{}\r\n",
                    self.config.replica_id,
                    self.config.consistency_level,
                    self.config.enabled,
                    NUM_SHARDS
                );
                RespValue::BulkString(Some(info.into_bytes()))
            }
            _ => RespValue::Error("ERR unknown command".to_string()),
        }
    }

    pub fn apply_remote_deltas(&self, deltas: Vec<ReplicationDelta>) {
        for delta in deltas {
            let shard_idx = hash_key(&delta.key);
            let mut shard = self.shards[shard_idx].write();
            shard.apply_remote_delta(delta);
        }
    }

    pub fn collect_pending_deltas(&self) -> Vec<ReplicationDelta> {
        let mut all_deltas = Vec::new();
        for shard in &self.shards {
            let mut s = shard.write();
            all_deltas.extend(s.drain_pending_deltas());
        }
        all_deltas
    }

    /// Evict expired keys from all shards
    ///
    /// Uses the configured TimeSource for zero-cost abstraction.
    pub fn evict_expired_all_shards(&self) -> usize {
        let current_time_ms = self.time_source.now_millis();
        let current_time = VirtualTime::from_millis(current_time_ms);

        let mut total_evicted = 0;
        for shard in &self.shards {
            let mut s = shard.write();
            total_evicted += s.evict_expired(current_time);
        }
        total_evicted
    }

    /// Get the time source
    pub fn time_source(&self) -> &T {
        &self.time_source
    }

    /// Get the gossip backend
    ///
    /// Returns the configured gossip backend (either locked or actor-based).
    pub fn gossip_backend(&self) -> &GossipBackend {
        &self.gossip_backend
    }

    /// Get the gossip state (legacy lock-based)
    ///
    /// Returns Some if using lock-based gossip, None if using actor-based.
    /// Prefer using `gossip_backend()` or `gossip_actor_handle()` instead.
    pub fn get_gossip_state(&self) -> Option<Arc<RwLock<GossipState>>> {
        match &self.gossip_backend {
            GossipBackend::Locked(state) => Some(state.clone()),
            GossipBackend::Actor(_) => None,
        }
    }

    /// Get the gossip actor handle (actor-based)
    ///
    /// Returns Some if using actor-based gossip, None if using lock-based.
    pub fn gossip_actor_handle(&self) -> Option<GossipActorHandle> {
        match &self.gossip_backend {
            GossipBackend::Locked(_) => None,
            GossipBackend::Actor(handle) => Some(handle.clone()),
        }
    }

    /// Check if using actor-based gossip
    pub fn is_actor_based(&self) -> bool {
        matches!(&self.gossip_backend, GossipBackend::Actor(_))
    }

    pub fn config(&self) -> &ReplicationConfig {
        &self.config
    }

    pub fn num_shards(&self) -> usize {
        NUM_SHARDS
    }

    /// Create a snapshot of all replicated state for checkpointing
    ///
    /// Returns a HashMap of all keys to their ReplicatedValue across all shards.
    /// This is used by the CheckpointManager to create full state snapshots.
    pub fn snapshot_state(&self) -> HashMap<String, crate::replication::state::ReplicatedValue> {
        let mut snapshot = HashMap::new();
        for shard in &self.shards {
            let s = shard.read();
            for (key, value) in &s.replica_state.replicated_keys {
                snapshot.insert(key.clone(), value.clone());
            }
        }
        snapshot
    }

    /// Apply recovered state from persistence
    ///
    /// This is called during server startup to restore state from object store.
    /// If checkpoint_state is provided, it replaces the current state.
    /// Then all deltas are applied in order (CRDT merge is idempotent).
    pub fn apply_recovered_state(
        &self,
        checkpoint_state: Option<HashMap<String, crate::replication::state::ReplicatedValue>>,
        deltas: Vec<ReplicationDelta>,
    ) {
        // Step 1: Apply checkpoint state if present
        if let Some(state) = checkpoint_state {
            for (key, value) in state {
                let shard_idx = hash_key(&key);
                let mut shard = self.shards[shard_idx].write();
                shard.replica_state.replicated_keys.insert(key.clone(), value.clone());

                // Also apply to executor for command execution
                if let Some(v) = value.get() {
                    if let Some(expiry_ms) = value.expiry_ms {
                        let seconds = (expiry_ms / 1000) as i64;
                        let cmd = crate::redis::Command::SetEx(key, seconds, v.clone());
                        shard.executor.execute(&cmd);
                    } else {
                        let cmd = crate::redis::Command::Set(key, v.clone());
                        shard.executor.execute(&cmd);
                    }
                }
            }
        }

        // Step 2: Apply all deltas (CRDT merge is idempotent)
        self.apply_remote_deltas(deltas);
    }

    /// Get the total number of keys across all shards
    pub fn key_count(&self) -> usize {
        let mut count = 0;
        for shard in &self.shards {
            let s = shard.read();
            count += s.replica_state.replicated_keys.len();
        }
        count
    }
}

impl<T: TimeSource> Clone for ReplicatedShardedState<T> {
    fn clone(&self) -> Self {
        ReplicatedShardedState {
            shards: self.shards.clone(),
            config: self.config.clone(),
            gossip_backend: self.gossip_backend.clone(),
            delta_sink: self.delta_sink.clone(),
            time_source: self.time_source.clone(),
        }
    }
}
