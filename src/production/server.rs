use super::{ShardedRedisState, connection::ConnectionHandler, ttl_manager::TtlManager};
use tokio::net::TcpListener;
use tracing::{info, error};

const NUM_SHARDS: usize = 16;

pub struct ProductionRedisServer {
    addr: String,
}

impl ProductionRedisServer {
    pub fn new(addr: String) -> Self {
        ProductionRedisServer { addr }
    }
    
    pub async fn run(self) -> Result<(), Box<dyn std::error::Error>> {
        let state = ShardedRedisState::new();
        
        info!("Initialized sharded Redis with {} shards", NUM_SHARDS);
        
        let ttl_manager = TtlManager::new(state.clone());
        tokio::spawn(async move {
            ttl_manager.run().await;
        });
        
        let listener = TcpListener::bind(&self.addr).await?;
        info!("Redis server listening on {}", self.addr);
        
        loop {
            match listener.accept().await {
                Ok((stream, addr)) => {
                    let client_addr = addr.to_string();
                    let state_clone = state.clone();
                    
                    tokio::spawn(async move {
                        let handler = ConnectionHandler::new(
                            stream,
                            state_clone,
                            client_addr,
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
