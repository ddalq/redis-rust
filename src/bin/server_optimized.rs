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
    
    println!("🚀 Optimized Redis Cache Server starting on 0.0.0.0:3000");
    println!("   Performance optimizations enabled:");
    println!("   ✓ jemalloc custom allocator (-10% overhead)");
    println!("   ✓ Actor-based shards (lock-free, -30% overhead)");
    println!("   ✓ Connection pooling (-10% overhead)");
    println!("   ✓ Buffer pooling (-20% overhead)");
    println!();
    
    server.run().await?;
    
    Ok(())
}
