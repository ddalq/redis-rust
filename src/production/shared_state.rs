use crate::redis::{Command, CommandExecutor, RespValue};
use crate::simulator::VirtualTime;
use parking_lot::RwLock;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone)]
pub struct SharedRedisState {
    executor: Arc<RwLock<CommandExecutor>>,
    start_time: SystemTime,
}

impl SharedRedisState {
    pub fn new() -> Self {
        let mut executor = CommandExecutor::new();
        
        let epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        executor.set_simulation_start_epoch(epoch);
        
        SharedRedisState {
            executor: Arc::new(RwLock::new(executor)),
            start_time: SystemTime::now(),
        }
    }
    
    fn get_current_virtual_time(&self) -> VirtualTime {
        let elapsed = self.start_time.elapsed().unwrap();
        VirtualTime::from_millis(elapsed.as_millis() as u64)
    }
    
    pub fn execute(&self, cmd: &Command) -> RespValue {
        let virtual_time = self.get_current_virtual_time();
        let mut executor = self.executor.write();
        executor.set_time(virtual_time);
        executor.execute(cmd)
    }
    
    pub fn with_lock<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut CommandExecutor) -> R,
    {
        let mut executor = self.executor.write();
        
        let elapsed = self.start_time.elapsed().unwrap();
        let virtual_time = VirtualTime::from_millis(elapsed.as_millis() as u64);
        executor.set_time(virtual_time);
        
        f(&mut executor)
    }
    
    pub fn evict_expired(&self) -> usize {
        let virtual_time = self.get_current_virtual_time();
        let mut executor = self.executor.write();
        executor.evict_expired_direct(virtual_time)
    }
}
