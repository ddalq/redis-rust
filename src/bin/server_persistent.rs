//! Redis Server with Streaming Persistence
//!
//! A Redis-compatible server that persists data to object store using
//! streaming delta writes. Supports recovery on startup.

#[cfg(not(target_env = "msvc"))]
use tikv_jemallocator::Jemalloc;

#[cfg(not(target_env = "msvc"))]
#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;

use redis_sim::production::ReplicatedShardedState;
use redis_sim::replication::{ReplicationConfig, ConsistencyLevel};
use redis_sim::streaming::{StreamingConfig, StreamingIntegration, ObjectStoreType, WorkerHandles};
use redis_sim::redis::{RespCodec, RespValue, Command};
use bytes::{BytesMut, BufMut};
use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::signal;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, error};

const DEFAULT_PORT: u16 = 3000;
const DEFAULT_REPLICA_ID: u64 = 1;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    // Parse command line arguments
    let args: Vec<String> = std::env::args().collect();
    let port = args.get(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_PORT);
    let persistence_path = args.get(2)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp/redis-persistent"));
    let use_memory = args.iter().any(|s| s == "--memory");

    println!("Redis Server with Streaming Persistence");
    println!("========================================");
    println!();
    println!("Configuration:");
    println!("  Port: {}", port);
    if use_memory {
        println!("  Store: InMemory (no durability)");
    } else {
        println!("  Store: LocalFs ({})", persistence_path.display());
    }
    println!();

    // Create replication config
    let repl_config = ReplicationConfig {
        enabled: true,
        replica_id: DEFAULT_REPLICA_ID,
        consistency_level: ConsistencyLevel::Eventual,
        gossip_interval_ms: 100,
        peers: vec![],
        replication_factor: 1,
        partitioned_mode: false,
        selective_gossip: false,
        virtual_nodes_per_physical: 150,
    };

    // Create state
    let mut state = ReplicatedShardedState::new(repl_config);

    // Configure streaming persistence
    let streaming_config = if use_memory {
        StreamingConfig::test()
    } else {
        StreamingConfig {
            enabled: true,
            store_type: ObjectStoreType::LocalFs,
            prefix: "redis-stream".to_string(),
            local_path: Some(persistence_path.clone()),
            #[cfg(feature = "s3")]
            s3: None,
            write_buffer: redis_sim::streaming::WriteBufferConfig::default(),
            checkpoint: redis_sim::streaming::config::CheckpointConfig::default(),
            compaction: redis_sim::streaming::config::CompactionConfig::default(),
        }
    };

    // Create integration and perform recovery
    let worker_handles: WorkerHandles = if use_memory {
        let integration = StreamingIntegration::new_in_memory(streaming_config, DEFAULT_REPLICA_ID);

        info!("Checking for existing data to recover...");
        let stats = integration.recover(&state).await?;
        if stats.segments_loaded > 0 {
            info!(
                "Recovered {} segments, {} deltas",
                stats.segments_loaded, stats.deltas_replayed
            );
        } else {
            info!("Starting fresh (no existing data)");
        }

        let (handles, sender) = integration.start_workers().await?;
        state.set_delta_sink(sender);
        handles
    } else {
        // Ensure persistence directory exists
        std::fs::create_dir_all(&persistence_path)?;

        let integration = StreamingIntegration::new_local_fs(streaming_config, DEFAULT_REPLICA_ID)?;

        info!("Checking for existing data to recover...");
        let stats = integration.recover(&state).await?;
        if stats.segments_loaded > 0 {
            info!(
                "Recovered {} segments, {} deltas, {} keys",
                stats.segments_loaded,
                stats.deltas_replayed,
                state.key_count()
            );
        } else {
            info!("Starting fresh (no existing data)");
        }

        let (handles, sender) = integration.start_workers().await?;
        state.set_delta_sink(sender);
        handles
    };

    let state = Arc::new(state);

    println!("Starting server...");
    println!();

    // Start TCP listener
    let addr = format!("0.0.0.0:{}", port);
    let listener = TcpListener::bind(&addr).await?;

    info!("Server listening on {}", addr);
    println!("Server listening on {}", addr);
    println!("Press Ctrl+C to shutdown gracefully");
    println!();

    // Accept connections until shutdown
    loop {
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, addr)) => {
                        let state = state.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_connection(stream, state).await {
                                error!("Connection error from {}: {}", addr, e);
                            }
                        });
                    }
                    Err(e) => {
                        error!("Accept error: {}", e);
                    }
                }
            }
            _ = signal::ctrl_c() => {
                info!("Shutdown signal received");
                println!("\nShutdown signal received, flushing data...");
                break;
            }
        }
    }

    // Graceful shutdown
    info!("Shutting down persistence workers...");
    worker_handles.shutdown().await;

    println!("Server shutdown complete");
    info!("Server shutdown complete");

    Ok(())
}

async fn handle_connection(
    mut stream: TcpStream,
    state: Arc<ReplicatedShardedState>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Enable TCP_NODELAY for lower latency
    let _ = stream.set_nodelay(true);

    let mut read_buf = [0u8; 8192];
    let mut buffer = BytesMut::with_capacity(4096);
    let mut write_buffer = BytesMut::with_capacity(4096);

    loop {
        let n = stream.read(&mut read_buf).await?;
        if n == 0 {
            break;
        }

        buffer.extend_from_slice(&read_buf[..n]);

        // Process all available commands (pipelining support)
        loop {
            match RespCodec::parse(&mut buffer) {
                Ok(Some(resp_value)) => {
                    match Command::from_resp_zero_copy(&resp_value) {
                        Ok(cmd) => {
                            let response = state.execute(cmd);
                            encode_resp_into(&response, &mut write_buffer);
                        }
                        Err(e) => {
                            encode_error_into(&e, &mut write_buffer);
                        }
                    }
                }
                Ok(None) => break, // Need more data
                Err(e) => {
                    encode_error_into(&format!("protocol error: {}", e), &mut write_buffer);
                    buffer.clear();
                    break;
                }
            }
        }

        // Flush all responses
        if !write_buffer.is_empty() {
            stream.write_all(&write_buffer).await?;
            stream.flush().await?;
            write_buffer.clear();
        }
    }

    Ok(())
}

fn encode_resp_into(value: &RespValue, buf: &mut BytesMut) {
    match value {
        RespValue::SimpleString(s) => {
            buf.put_u8(b'+');
            buf.extend_from_slice(s.as_bytes());
            buf.extend_from_slice(b"\r\n");
        }
        RespValue::Error(s) => {
            buf.put_u8(b'-');
            buf.extend_from_slice(s.as_bytes());
            buf.extend_from_slice(b"\r\n");
        }
        RespValue::Integer(n) => {
            buf.put_u8(b':');
            buf.extend_from_slice(n.to_string().as_bytes());
            buf.extend_from_slice(b"\r\n");
        }
        RespValue::BulkString(None) => {
            buf.extend_from_slice(b"$-1\r\n");
        }
        RespValue::BulkString(Some(data)) => {
            buf.put_u8(b'$');
            buf.extend_from_slice(data.len().to_string().as_bytes());
            buf.extend_from_slice(b"\r\n");
            buf.extend_from_slice(data);
            buf.extend_from_slice(b"\r\n");
        }
        RespValue::Array(None) => {
            buf.extend_from_slice(b"*-1\r\n");
        }
        RespValue::Array(Some(elements)) => {
            buf.put_u8(b'*');
            buf.extend_from_slice(elements.len().to_string().as_bytes());
            buf.extend_from_slice(b"\r\n");
            for elem in elements {
                encode_resp_into(elem, buf);
            }
        }
    }
}

fn encode_error_into(msg: &str, buf: &mut BytesMut) {
    buf.put_u8(b'-');
    buf.extend_from_slice(b"ERR ");
    buf.extend_from_slice(msg.as_bytes());
    buf.extend_from_slice(b"\r\n");
}
