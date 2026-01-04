use super::{ShardedActorState, ConnectionPool};
use super::connection_optimized::OptimizedConnectionHandler;
use super::ttl_manager::TtlManagerActor;
use crate::observability::{DatadogConfig, Metrics};
use tokio::net::TcpListener;
use tracing::{info, error};
use std::sync::Arc;

const NUM_SHARDS: usize = 16;

pub struct OptimizedRedisServer {
    addr: String,
}

impl OptimizedRedisServer {
    #[inline]
    pub fn new(addr: String) -> Self {
        debug_assert!(!addr.is_empty(), "Server address cannot be empty");
        OptimizedRedisServer { addr }
    }

    pub async fn run(self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let state = ShardedActorState::new();
        let connection_pool = Arc::new(ConnectionPool::new(10000, 512));

        // Initialize metrics
        let dd_config = DatadogConfig::from_env();
        let metrics = Arc::new(Metrics::new(&dd_config));

        info!("Initialized Tiger Style Redis with {} shards (lock-free)", NUM_SHARDS);

        let ttl_manager = TtlManagerActor::new(state.clone(), metrics.clone());
        tokio::spawn(async move {
            ttl_manager.run().await;
        });
        info!("TTL manager started (100ms interval)");

        let listener = TcpListener::bind(&self.addr).await?;
        info!("Redis server listening on {}", self.addr);

        loop {
            match listener.accept().await {
                Ok((stream, addr)) => {
                    let client_addr = addr.to_string();
                    let state_clone = state.clone();
                    let pool = connection_pool.clone();
                    let metrics_clone = metrics.clone();

                    tokio::spawn(async move {
                        let _permit = pool.acquire_permit().await;

                        let handler = OptimizedConnectionHandler::new(
                            stream,
                            state_clone,
                            client_addr,
                            pool.buffer_pool(),
                            metrics_clone,
                        );
                        handler.run().await;
                    });
                }
                Err(e) => {
                    error!("Failed to accept connection: {}", e);
                }
            }
        }
    }
}
