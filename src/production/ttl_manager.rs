use super::ShardedActorState;
use tokio::time::{interval, Duration};
use tracing::debug;

const TTL_CHECK_INTERVAL_MS: u64 = 100;

pub struct TtlManagerActor {
    state: ShardedActorState,
    interval_ms: u64,
}

impl TtlManagerActor {
    #[inline]
    pub fn new(state: ShardedActorState) -> Self {
        Self::with_interval(state, TTL_CHECK_INTERVAL_MS)
    }

    #[inline]
    pub fn with_interval(state: ShardedActorState, interval_ms: u64) -> Self {
        debug_assert!(interval_ms > 0, "TTL interval must be positive");
        TtlManagerActor { state, interval_ms }
    }

    pub async fn run(self) {
        let mut tick = interval(Duration::from_millis(self.interval_ms));

        loop {
            tick.tick().await;
            let evicted = self.state.evict_expired_all_shards().await;
            if evicted > 0 {
                debug!("TTL manager evicted {} expired keys", evicted);
            }
        }
    }
}
