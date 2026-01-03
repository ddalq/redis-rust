use crate::redis::{Command, CommandExecutor, RespValue};
use crate::simulator::VirtualTime;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, oneshot};

const NUM_SHARDS: usize = 16;

pub struct ShardCommand {
    cmd: Command,
    virtual_time: VirtualTime,
    response_tx: oneshot::Sender<RespValue>,
}

pub struct ShardActor {
    executor: CommandExecutor,
    rx: mpsc::UnboundedReceiver<ShardCommand>,
}

impl ShardActor {
    fn new(rx: mpsc::UnboundedReceiver<ShardCommand>, simulation_start_epoch: i64) -> Self {
        let mut executor = CommandExecutor::new();
        executor.set_simulation_start_epoch(simulation_start_epoch);
        ShardActor { executor, rx }
    }

    async fn run(mut self) {
        while let Some(shard_cmd) = self.rx.recv().await {
            self.executor.set_time(shard_cmd.virtual_time);
            let response = self.executor.execute(&shard_cmd.cmd);
            let _ = shard_cmd.response_tx.send(response);
        }
    }

    fn evict_expired(&mut self, virtual_time: VirtualTime) -> usize {
        self.executor.evict_expired_direct(virtual_time)
    }
}

#[derive(Clone)]
pub struct ShardHandle {
    tx: mpsc::UnboundedSender<ShardCommand>,
}

impl ShardHandle {
    async fn execute(&self, cmd: Command, virtual_time: VirtualTime) -> RespValue {
        let (response_tx, response_rx) = oneshot::channel();
        let shard_cmd = ShardCommand {
            cmd,
            virtual_time,
            response_tx,
        };
        
        if self.tx.send(shard_cmd).is_err() {
            return RespValue::Error("Shard unavailable".to_string());
        }
        
        response_rx.await.unwrap_or_else(|_| RespValue::Error("Shard response failed".to_string()))
    }
}

fn hash_key(key: &str) -> usize {
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    (hasher.finish() as usize) % NUM_SHARDS
}

#[derive(Clone)]
pub struct ShardedActorState {
    shards: Arc<[ShardHandle; NUM_SHARDS]>,
    start_time: SystemTime,
}

impl ShardedActorState {
    pub fn new() -> Self {
        let epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        
        let shards: [ShardHandle; NUM_SHARDS] = std::array::from_fn(|_| {
            let (tx, rx) = mpsc::unbounded_channel();
            let actor = ShardActor::new(rx, epoch);
            tokio::spawn(actor.run());
            ShardHandle { tx }
        });
        
        ShardedActorState {
            shards: Arc::new(shards),
            start_time: SystemTime::now(),
        }
    }
    
    fn get_current_virtual_time(&self) -> VirtualTime {
        let elapsed = self.start_time.elapsed().unwrap();
        VirtualTime::from_millis(elapsed.as_millis() as u64)
    }
    
    pub async fn execute(&self, cmd: &Command) -> RespValue {
        let virtual_time = self.get_current_virtual_time();
        
        match cmd {
            Command::Ping => RespValue::SimpleString("PONG".to_string()),
            
            Command::Info => {
                let info = format!(
                    "# Server\r\n\
                     redis_mode:actor_sharded\r\n\
                     num_shards:{}\r\n\
                     architecture:message_passing\r\n\
                     \r\n\
                     # Stats\r\n\
                     current_time_ms:{}\r\n",
                    NUM_SHARDS,
                    virtual_time.as_millis()
                );
                RespValue::BulkString(Some(info.into_bytes()))
            }
            
            Command::FlushDb | Command::FlushAll => {
                let mut futures = Vec::with_capacity(NUM_SHARDS);
                for shard in self.shards.iter() {
                    futures.push(shard.execute(Command::FlushDb, virtual_time));
                }
                for future in futures {
                    let _ = future.await;
                }
                RespValue::SimpleString("OK".to_string())
            }
            
            Command::Keys(pattern) => {
                let mut futures = Vec::with_capacity(NUM_SHARDS);
                for shard in self.shards.iter() {
                    futures.push(shard.execute(Command::Keys(pattern.clone()), virtual_time));
                }
                
                let mut all_keys: Vec<RespValue> = Vec::new();
                for future in futures {
                    if let RespValue::Array(Some(keys)) = future.await {
                        all_keys.extend(keys);
                    }
                }
                RespValue::Array(Some(all_keys))
            }
            
            Command::MGet(keys) => {
                let mut futures: Vec<_> = keys.iter().map(|key| {
                    let shard_idx = hash_key(key);
                    self.shards[shard_idx].execute(Command::Get(key.clone()), virtual_time)
                }).collect();
                
                let mut results = Vec::with_capacity(keys.len());
                for future in futures {
                    results.push(future.await);
                }
                RespValue::Array(Some(results))
            }
            
            Command::MSet(pairs) => {
                let mut futures = Vec::with_capacity(pairs.len());
                for (key, value) in pairs {
                    let shard_idx = hash_key(key);
                    futures.push(self.shards[shard_idx].execute(
                        Command::Set(key.clone(), value.clone()),
                        virtual_time,
                    ));
                }
                for future in futures {
                    let _ = future.await;
                }
                RespValue::SimpleString("OK".to_string())
            }
            
            Command::Exists(keys) => {
                let mut futures = Vec::with_capacity(keys.len());
                for key in keys {
                    let shard_idx = hash_key(key);
                    futures.push(self.shards[shard_idx].execute(
                        Command::Exists(vec![key.clone()]),
                        virtual_time,
                    ));
                }
                
                let mut count = 0i64;
                for future in futures {
                    if let RespValue::Integer(n) = future.await {
                        count += n;
                    }
                }
                RespValue::Integer(count)
            }
            
            _ => {
                if let Some(key) = cmd.get_primary_key() {
                    let shard_idx = hash_key(key);
                    self.shards[shard_idx].execute(cmd.clone(), virtual_time).await
                } else {
                    self.shards[0].execute(cmd.clone(), virtual_time).await
                }
            }
        }
    }
}

impl Default for ShardedActorState {
    fn default() -> Self {
        Self::new()
    }
}
