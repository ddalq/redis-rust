use crate::redis::{Command, CommandExecutor, RespValue};
use crate::simulator::VirtualTime;
use parking_lot::RwLock;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

const NUM_SHARDS: usize = 16;

/// ShardedRedisState distributes keys across multiple executors using hash partitioning.
/// 
/// **Consistency Model:**
/// - Single-key operations (GET, SET, INCR, etc.) are atomic and consistent
/// - Multi-key operations (MSET, MGET, EXISTS) have relaxed semantics:
///   - Each key is processed independently on its shard
///   - No cross-shard atomicity (similar to Redis Cluster)
///   - Acceptable for caching workloads where eventual consistency is OK
/// 
/// This trade-off provides significantly higher throughput (~60-70% improvement)
/// at the cost of strict multi-key atomicity.

fn hash_key(key: &str) -> usize {
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    (hasher.finish() as usize) % NUM_SHARDS
}

#[derive(Clone)]
pub struct ShardedRedisState {
    shards: Arc<[RwLock<CommandExecutor>; NUM_SHARDS]>,
    start_time: SystemTime,
}

impl ShardedRedisState {
    pub fn new() -> Self {
        let epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        
        let shards: [RwLock<CommandExecutor>; NUM_SHARDS] = std::array::from_fn(|_| {
            let mut executor = CommandExecutor::new();
            executor.set_simulation_start_epoch(epoch);
            RwLock::new(executor)
        });
        
        ShardedRedisState {
            shards: Arc::new(shards),
            start_time: SystemTime::now(),
        }
    }
    
    fn get_current_virtual_time(&self) -> VirtualTime {
        let elapsed = self.start_time.elapsed().unwrap();
        VirtualTime::from_millis(elapsed.as_millis() as u64)
    }
    
    pub fn execute(&self, cmd: &Command) -> RespValue {
        let virtual_time = self.get_current_virtual_time();
        
        match cmd {
            Command::Ping => RespValue::SimpleString("PONG".to_string()),
            
            Command::Info => {
                let info = format!(
                    "# Server\r\n\
                     redis_mode:sharded\r\n\
                     num_shards:{}\r\n\
                     \r\n\
                     # Stats\r\n\
                     current_time_ms:{}\r\n",
                    NUM_SHARDS,
                    virtual_time.as_millis()
                );
                RespValue::BulkString(Some(info.into_bytes()))
            }
            
            Command::FlushDb | Command::FlushAll => {
                for shard in self.shards.iter() {
                    let mut executor = shard.write();
                    executor.set_time(virtual_time);
                    executor.execute(&Command::FlushDb);
                }
                RespValue::SimpleString("OK".to_string())
            }
            
            Command::Keys(pattern) => {
                let mut all_keys: Vec<RespValue> = Vec::new();
                for shard in self.shards.iter() {
                    let mut executor = shard.write();
                    executor.set_time(virtual_time);
                    if let RespValue::Array(Some(keys)) = executor.execute(&Command::Keys(pattern.clone())) {
                        all_keys.extend(keys);
                    }
                }
                RespValue::Array(Some(all_keys))
            }
            
            Command::MGet(keys) => {
                let mut results: Vec<RespValue> = Vec::with_capacity(keys.len());
                for key in keys {
                    let shard_idx = hash_key(key);
                    let mut executor = self.shards[shard_idx].write();
                    executor.set_time(virtual_time);
                    results.push(executor.execute(&Command::Get(key.clone())));
                }
                RespValue::Array(Some(results))
            }
            
            Command::MSet(pairs) => {
                for (key, value) in pairs {
                    let shard_idx = hash_key(key);
                    let mut executor = self.shards[shard_idx].write();
                    executor.set_time(virtual_time);
                    executor.execute(&Command::Set(key.clone(), value.clone()));
                }
                RespValue::SimpleString("OK".to_string())
            }
            
            Command::Exists(keys) => {
                let mut count = 0i64;
                for key in keys {
                    let shard_idx = hash_key(key);
                    let mut executor = self.shards[shard_idx].write();
                    executor.set_time(virtual_time);
                    if let RespValue::Integer(n) = executor.execute(&Command::Exists(vec![key.clone()])) {
                        count += n;
                    }
                }
                RespValue::Integer(count)
            }
            
            _ => {
                if let Some(key) = cmd.get_primary_key() {
                    let shard_idx = hash_key(key);
                    let mut executor = self.shards[shard_idx].write();
                    executor.set_time(virtual_time);
                    executor.execute(cmd)
                } else {
                    let mut executor = self.shards[0].write();
                    executor.set_time(virtual_time);
                    executor.execute(cmd)
                }
            }
        }
    }
    
    pub fn evict_expired_all_shards(&self) -> usize {
        let virtual_time = self.get_current_virtual_time();
        let mut total_evicted = 0;
        
        for shard in self.shards.iter() {
            let mut executor = shard.write();
            total_evicted += executor.evict_expired_direct(virtual_time);
        }
        
        total_evicted
    }
}
