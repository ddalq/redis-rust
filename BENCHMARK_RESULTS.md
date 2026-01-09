# Redis Server Performance Benchmark Results

## Test Configuration

**Server:** Tiger Style Redis Server (Actor-per-Shard Architecture)
**Binary:** `redis-server-optimized`
**Port:** 3000
**Date:** January 8, 2026

### System Configuration

| Component | Specification |
|-----------|---------------|
| OS | macOS Darwin 24.4.0 |
| Platform | Docker Desktop |
| CPU Limit | 2 cores per container |
| Memory Limit | 1GB per container |
| Requests | 100,000 |
| Clients | 50 concurrent |
| Data Size | 64 bytes |

## Redis 8.0 Comparison

Three-way comparison: Redis 7.4 vs Redis 8.0 vs Rust implementation.

### Non-Pipelined Performance (P=1)

| Operation | Redis 7.4 | Redis 8.0 | Rust | Rust vs R8 |
|-----------|-----------|-----------|------|------------|
| SET | 170,068 req/s | 165,837 req/s | 168,350 req/s | **101.5%** |
| GET | 179,856 req/s | 183,824 req/s | 168,067 req/s | **91.4%** |

### Pipelined Performance (P=16)

| Operation | Redis 7.4 | Redis 8.0 | Rust | Rust vs R8 |
|-----------|-----------|-----------|------|------------|
| SET | 1,408,451 req/s | 1,369,863 req/s | 1,086,957 req/s | **79.3%** |
| GET | 1,250,000 req/s | 1,449,275 req/s | 1,250,000 req/s | **86.2%** |

### Summary

- **SET P=1: 101.5% of Redis 8.0** - Exceeds Redis 8.0 for single-operation writes!
- **GET P=1: 91.4% of Redis 8.0** - Competitive single-operation reads
- **SET P=16: 79.3% of Redis 8.0** - Pipelined write performance
- **GET P=16: 86.2% of Redis 8.0** - Pipelined read performance

### Performance Optimizations Applied

| Optimization | Description | Impact |
|-------------|-------------|--------|
| P0: Single Key Allocation | Reuse key string in `set_direct()` | +5-10% |
| P1: Static OK Response | Pre-allocated "OK" response | +1-2% |
| P2: Zero-Copy GET | Avoid data copy in `get_direct()` | +2-3% |
| P3: itoa Encoding | Fast integer-to-string conversion | +1-2% |
| P4: atoi Parsing | Fast string-to-integer parsing | +2-3% |

## Architecture

### Actor-per-Shard Design

```
Client Connection
       |
  [Connection Handler]
       |
  hash(key) % num_shards
       |
  [ShardActor 0..N]  <-- tokio::mpsc channels (lock-free)
       |
  [CommandExecutor]
```

### Performance Optimizations

| Optimization | Description |
|-------------|-------------|
| jemalloc | `tikv-jemallocator` custom allocator |
| Actor-per-Shard | Lock-free tokio channels (no RwLock) |
| Buffer Pooling | `crossbeam::ArrayQueue` buffer reuse |
| Zero-copy Parser | `bytes::Bytes` + `memchr` RESP parsing |
| Connection Pooling | Semaphore-limited with shared buffers |

### Zero-Copy RESP Parser

```
[RespCodec::parse]
       |
  [memchr] for CRLF scanning
       |
  [bytes::Bytes] zero-copy slicing
       |
  [RespValueZeroCopy] borrowed references
```

## Correctness Testing

### Test Suite (500+ tests)

| Category | Tests | Coverage |
|----------|-------|----------|
| Unit Tests | 400+ | RESP parsing, commands, data structures |
| Lua Scripting | 37 | EVAL/EVALSHA execution |
| Redis Equivalence | 30+ | Differential testing vs real Redis |
| CRDT/Consistency | 34 | Convergence, vector clocks, partition healing |
| DST/Simulation | 16 | Multi-seed chaos testing with fault injection |
| Streaming Persistence | 20 | Object store, recovery, compaction |

### Convergence Tests (January 6, 2026)

| Test Category | Tests | Result |
|---------------|-------|--------|
| CRDT Convergence | 16 | **PASS** |
| Multi-Node Replication | 9 | **PASS** |
| Partition Tolerance | 14 | **PASS** |
| **Total** | **39** | **100% PASS** |

### Maelstrom/Jepsen Results

| Test | Nodes | Result | Notes |
|------|-------|--------|-------|
| Linearizability (lin-kv) | 1 | **PASS** | Single-node is linearizable |
| Linearizability (lin-kv) | 3 | **FAIL** | Expected: eventual consistency |

**Note:** Multi-node linearizability tests FAIL by design. We use Anna-style eventual consistency, not Raft/Paxos consensus.

## Running Benchmarks

### Docker Benchmark (Recommended)

```bash
cd docker-benchmark

# Redis 8.0 three-way comparison
./run-redis8-comparison.sh

# In-memory comparison (Redis 7.4 vs Rust)
./run-benchmarks.sh

# Persistent comparison (Redis AOF vs Rust S3/MinIO)
./run-persistent-benchmarks.sh
```

### Benchmark Commands

```bash
# Non-pipelined (P=1)
redis-benchmark -p <port> -n 100000 -c 50 -P 1 -d 64 -r 10000 -t set,get --csv

# Pipelined (P=16)
redis-benchmark -p <port> -n 100000 -c 50 -P 16 -d 64 -r 10000 -t set,get --csv
```

## Known Limitations

1. **Streaming persistence**: Object store-based (S3/LocalFs), not traditional RDB/AOF
2. **No pub/sub or streams**: Not implemented
3. **Multi-node consistency**: Eventual, not linearizable (by design)
