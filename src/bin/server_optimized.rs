//! Optimized Redis Server (Drop-in Replacement)
//!
//! High-performance Redis-compatible server without persistence.
//! Uses Redis standard port 6379 by default for drop-in replacement.
//!
//! ## Environment Variables
//!
//! | Variable | Default | Description |
//! |----------|---------|-------------|
//! | REDIS_PORT | 6379 | Server port (Redis default) |
//!
//! ## Datadog (when built with --features datadog)
//!
//! | Variable | Default | Description |
//! |----------|---------|-------------|
//! | DD_SERVICE | redis-rust | Service name |
//! | DD_ENV | development | Environment |
//! | DD_DOGSTATSD_URL | 127.0.0.1:8125 | DogStatsD address |
//! | DD_TRACE_AGENT_URL | http://127.0.0.1:8126 | APM agent URL |

#[cfg(not(target_env = "msvc"))]
use tikv_jemallocator::Jemalloc;

#[cfg(not(target_env = "msvc"))]
#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;

use redis_sim::production::OptimizedRedisServer;
use redis_sim::observability::{DatadogConfig, init_tracing, shutdown};

const DEFAULT_PORT: u16 = 6379;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Initialize observability (Datadog when feature enabled, basic tracing otherwise)
    let dd_config = DatadogConfig::from_env();
    init_tracing(&dd_config)?;

    let port = std::env::var("REDIS_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_PORT);

    let addr = format!("0.0.0.0:{}", port);
    let server = OptimizedRedisServer::new(addr.clone());

    println!("Redis Rust Server (Drop-in Replacement)");
    println!("========================================");
    println!();
    println!("Listening on {}", addr);
    println!();
    println!("Performance optimizations:");
    println!("  - jemalloc custom allocator");
    println!("  - Actor-based shards (lock-free)");
    println!("  - Connection pooling");
    println!("  - Buffer pooling");
    #[cfg(feature = "datadog")]
    println!("  - Datadog observability enabled");
    println!();

    server.run().await?;

    // Graceful shutdown
    shutdown();

    Ok(())
}
