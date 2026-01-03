use super::ShardedRedisState;
use tokio::time::{interval, Duration};
use tracing::debug;

pub struct TtlManager {
    state: ShardedRedisState,
}

impl TtlManager {
    pub fn new(state: ShardedRedisState) -> Self {
        TtlManager { state }
    }
    
    pub async fn run(self) {
        let mut tick = interval(Duration::from_millis(100));
        
        loop {
            tick.tick().await;
            let evicted = self.state.evict_expired_all_shards();
            if evicted > 0 {
                debug!("TTL manager evicted {} expired keys", evicted);
            }
        }
    }
}
