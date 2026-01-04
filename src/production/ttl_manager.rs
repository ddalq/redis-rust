use super::ShardedActorState;
use crate::observability::Metrics;
use std::sync::Arc;
use tokio::time::{interval, Duration};
use tracing::debug;

const TTL_CHECK_INTERVAL_MS: u64 = 100;

pub struct TtlManagerActor {
    state: ShardedActorState,
    interval_ms: u64,
    metrics: Arc<Metrics>,
}

impl TtlManagerActor {
    #[inline]
    pub fn new(state: ShardedActorState, metrics: Arc<Metrics>) -> Self {
        Self::with_interval(state, TTL_CHECK_INTERVAL_MS, metrics)
    }

    #[inline]
    pub fn with_interval(state: ShardedActorState, interval_ms: u64, metrics: Arc<Metrics>) -> Self {
        debug_assert!(interval_ms > 0, "TTL interval must be positive");
        TtlManagerActor { state, interval_ms, metrics }
    }

    pub async fn run(self) {
        let mut tick = interval(Duration::from_millis(self.interval_ms));

        loop {
            tick.tick().await;
            let evicted = self.state.evict_expired_all_shards().await;
            if evicted > 0 {
                debug!("TTL manager evicted {} expired keys", evicted);
                self.metrics.record_ttl_eviction(evicted);
            }
        }
    }
}
