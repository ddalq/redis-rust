use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::Instant;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔥 Redis Server Performance Benchmark\n");
    println!("Connecting to 127.0.0.1:3000...\n");
    
    let num_requests = 10_000;
    let num_clients = 50;
    
    println!("Configuration:");
    println!("  Total requests per test: {}", num_requests);
    println!("  Concurrent clients: {}\n", num_clients);
    
    println!("====================================\n");
    
    benchmark_ping(num_requests, num_clients).await?;
    benchmark_set(num_requests, num_clients).await?;
    benchmark_get_existing(num_requests, num_clients).await?;
    benchmark_incr(num_requests, num_clients).await?;
    benchmark_mixed(num_requests, num_clients).await?;
    
    println!("====================================\n");
    println!("✅ Benchmark complete!\n");
    
    Ok(())
}

async fn benchmark_ping(num_requests: usize, num_clients: usize) -> Result<(), Box<dyn std::error::Error>> {
    let start = Instant::now();
    let completed = Arc::new(AtomicU64::new(0));
    
    let mut handles = vec![];
    let requests_per_client = num_requests / num_clients;
    
    for _ in 0..num_clients {
        let completed = completed.clone();
        let handle = tokio::spawn(async move {
            let mut stream = TcpStream::connect("127.0.0.1:3000").await.unwrap();
            let cmd = b"*1\r\n$4\r\nPING\r\n";
            let mut buf = vec![0u8; 64];
            
            for _ in 0..requests_per_client {
                stream.write_all(cmd).await.unwrap();
                stream.read(&mut buf).await.unwrap();
                completed.fetch_add(1, Ordering::Relaxed);
            }
        });
        handles.push(handle);
    }
    
    for handle in handles {
        handle.await?;
    }
    
    let elapsed = start.elapsed();
    let total = completed.load(Ordering::Relaxed);
    print_results("PING", total, elapsed);
    
    Ok(())
}

async fn benchmark_set(num_requests: usize, num_clients: usize) -> Result<(), Box<dyn std::error::Error>> {
    let start = Instant::now();
    let completed = Arc::new(AtomicU64::new(0));
    
    let mut handles = vec![];
    let requests_per_client = num_requests / num_clients;
    
    for client_id in 0..num_clients {
        let completed = completed.clone();
        let handle = tokio::spawn(async move {
            let mut stream = TcpStream::connect("127.0.0.1:3000").await.unwrap();
            let mut buf = vec![0u8; 64];
            
            for i in 0..requests_per_client {
                let cmd = format!("*3\r\n$3\r\nSET\r\n$6\r\nbm:{}:{}\r\n$5\r\nvalue\r\n", client_id, i);
                stream.write_all(cmd.as_bytes()).await.unwrap();
                stream.read(&mut buf).await.unwrap();
                completed.fetch_add(1, Ordering::Relaxed);
            }
        });
        handles.push(handle);
    }
    
    for handle in handles {
        handle.await?;
    }
    
    let elapsed = start.elapsed();
    let total = completed.load(Ordering::Relaxed);
    print_results("SET", total, elapsed);
    
    Ok(())
}

async fn benchmark_get_existing(num_requests: usize, num_clients: usize) -> Result<(), Box<dyn std::error::Error>> {
    let start = Instant::now();
    let completed = Arc::new(AtomicU64::new(0));
    
    let mut handles = vec![];
    let requests_per_client = num_requests / num_clients;
    
    for client_id in 0..num_clients {
        let completed = completed.clone();
        let handle = tokio::spawn(async move {
            let mut stream = TcpStream::connect("127.0.0.1:3000").await.unwrap();
            let mut buf = vec![0u8; 128];
            
            for i in 0..requests_per_client {
                let key_id = i % 200;
                let cmd = format!("*2\r\n$3\r\nGET\r\n$6\r\nbm:{}:{}\r\n", client_id, key_id);
                stream.write_all(cmd.as_bytes()).await.unwrap();
                stream.read(&mut buf).await.unwrap();
                completed.fetch_add(1, Ordering::Relaxed);
            }
        });
        handles.push(handle);
    }
    
    for handle in handles {
        handle.await?;
    }
    
    let elapsed = start.elapsed();
    let total = completed.load(Ordering::Relaxed);
    print_results("GET", total, elapsed);
    
    Ok(())
}

async fn benchmark_incr(num_requests: usize, num_clients: usize) -> Result<(), Box<dyn std::error::Error>> {
    let start = Instant::now();
    let completed = Arc::new(AtomicU64::new(0));
    
    let mut handles = vec![];
    let requests_per_client = num_requests / num_clients;
    
    for client_id in 0..num_clients {
        let completed = completed.clone();
        let handle = tokio::spawn(async move {
            let mut stream = TcpStream::connect("127.0.0.1:3000").await.unwrap();
            let cmd = format!("*2\r\n$4\r\nINCR\r\n$6\r\nctr:{}\r\n", client_id);
            let mut buf = vec![0u8; 64];
            
            for _ in 0..requests_per_client {
                stream.write_all(cmd.as_bytes()).await.unwrap();
                stream.read(&mut buf).await.unwrap();
                completed.fetch_add(1, Ordering::Relaxed);
            }
        });
        handles.push(handle);
    }
    
    for handle in handles {
        handle.await?;
    }
    
    let elapsed = start.elapsed();
    let total = completed.load(Ordering::Relaxed);
    print_results("INCR", total, elapsed);
    
    Ok(())
}

async fn benchmark_mixed(num_requests: usize, num_clients: usize) -> Result<(), Box<dyn std::error::Error>> {
    let start = Instant::now();
    let completed = Arc::new(AtomicU64::new(0));
    
    let mut handles = vec![];
    let requests_per_client = num_requests / num_clients;
    
    for client_id in 0..num_clients {
        let completed = completed.clone();
        let handle = tokio::spawn(async move {
            let mut stream = TcpStream::connect("127.0.0.1:3000").await.unwrap();
            let mut buf = vec![0u8; 256];
            
            for i in 0..requests_per_client {
                let cmd = match i % 4 {
                    0 => format!("*3\r\n$3\r\nSET\r\n$6\r\nmx:{}:{}\r\n$5\r\nvalue\r\n", client_id, i),
                    1 => format!("*2\r\n$3\r\nGET\r\n$6\r\nmx:{}:{}\r\n", client_id, i.saturating_sub(1)),
                    2 => format!("*2\r\n$4\r\nINCR\r\n$7\r\nmxc:{}\r\n", client_id),
                    _ => "*1\r\n$4\r\nPING\r\n".to_string(),
                };
                
                stream.write_all(cmd.as_bytes()).await.unwrap();
                stream.read(&mut buf).await.unwrap();
                completed.fetch_add(1, Ordering::Relaxed);
            }
        });
        handles.push(handle);
    }
    
    for handle in handles {
        handle.await?;
    }
    
    let elapsed = start.elapsed();
    let total = completed.load(Ordering::Relaxed);
    print_results("MIXED (SET/GET/INCR/PING)", total, elapsed);
    
    Ok(())
}

fn print_results(test_name: &str, total: u64, elapsed: std::time::Duration) {
    let ops_per_sec = total as f64 / elapsed.as_secs_f64();
    let latency_ms = elapsed.as_secs_f64() * 1000.0 / total as f64;
    
    println!("{}", test_name);
    println!("  {} requests in {:.2}s", total, elapsed.as_secs_f64());
    println!("  {:.0} req/sec", ops_per_sec);
    println!("  {:.3} ms avg latency\n", latency_ms);
}
