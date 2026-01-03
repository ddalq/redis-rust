use super::{ShardedActorState, ConnectionPool};
use super::connection_optimized::OptimizedConnectionHandler;
use tokio::net::TcpListener;
use tracing::{info, error};
use std::sync::Arc;

const NUM_SHARDS: usize = 16;

pub struct OptimizedRedisServer {
    addr: String,
}

impl OptimizedRedisServer {
    pub fn new(addr: String) -> Self {
        OptimizedRedisServer { addr }
    }
    
    pub async fn run(self) -> Result<(), Box<dyn std::error::Error>> {
        let state = ShardedActorState::new();
        let connection_pool = Arc::new(ConnectionPool::new(10000, 512));
        
        info!("Initialized actor-based Redis with {} shards (lock-free)", NUM_SHARDS);
        
        let listener = TcpListener::bind(&self.addr).await?;
        info!("Optimized Redis server listening on {}", self.addr);
        
        loop {
            match listener.accept().await {
                Ok((stream, addr)) => {
                    let client_addr = addr.to_string();
                    let state_clone = state.clone();
                    let pool = connection_pool.clone();
                    
                    tokio::spawn(async move {
                        let _permit = pool.acquire_permit().await;
                        
                        let handler = OptimizedConnectionHandler::new(
                            stream,
                            state_clone,
                            client_addr,
                            pool.buffer_pool(),
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
