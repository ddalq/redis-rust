#[cfg(not(target_env = "msvc"))]
use tikv_jemallocator::Jemalloc;

#[cfg(not(target_env = "msvc"))]
#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;

use redis_sim::production::OptimizedRedisServer;
use tracing_subscriber;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let server = OptimizedRedisServer::new("0.0.0.0:3000".to_string());

    println!("🚀 Redis Cache Server starting on 0.0.0.0:3000");
    println!("   Tiger Style: Explicit, deterministic, assertion-heavy");
    println!("   Actor-based shards with lock-free message passing");
    println!("   jemalloc + buffer pooling + zero-copy parsing");
    println!();

    server.run().await?;

    Ok(())
}
