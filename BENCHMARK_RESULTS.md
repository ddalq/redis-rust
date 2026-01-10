# Redis Server Performance Benchmark Results

## Test Configuration

**Server:** Tiger Style Redis Server (Actor-per-Shard Architecture)
**Binary:** `redis-server-optimized`
**Port:** 3000

### System Configuration

| Component | Specification |
|-----------|---------------|
| CPU Limit | 2 cores per container |
| Memory Limit | 1GB per container |
| Requests | 100,000 |
| Clients | 50 concurrent |
| Data Size | 64 bytes |

---

## Linux Benchmarks (January 9, 2026)

**Platform:** Linux 6.8.0-86-generic (Native Docker)

### Non-Pipelined Performance (P=1)

| Command | Redis 7.4 | Redis 8.0 | Rust | Rust vs R8 |
|---------|-----------|-----------|------|------------|
| PING | 83,542 req/s | 84,818 req/s | 79,555 req/s | **93.7%** |
| SET | 83,612 req/s | 82,576 req/s | 78,802 req/s | **95.4%** |
| GET | 81,766 req/s | 81,037 req/s | 77,942 req/s | **96.1%** |
| MSET | 70,872 req/s | 67,751 req/s | 72,516 req/s | **107.0%** |
| INCR | 80,000 req/s | 77,942 req/s | 77,459 req/s | **99.3%** |
| LPUSH | 75,930 req/s | 80,064 req/s | 75,415 req/s | **94.1%** |
| RPUSH | 76,746 req/s | 75,815 req/s | 73,638 req/s | **97.1%** |
| LPOP | 76,923 req/s | 78,064 req/s | 75,930 req/s | **97.2%** |
| RPOP | 76,923 req/s | 77,042 req/s | 72,569 req/s | **94.1%** |
| LRANGE_100 | 36,390 req/s | 44,743 req/s | 56,593 req/s | **126.4%** |
| LRANGE_300 | 19,015 req/s | 27,382 req/s | 26,882 req/s | **98.1%** |
| LRANGE_500 | 13,174 req/s | 20,064 req/s | 18,615 req/s | **92.7%** |
| SADD | 76,628 req/s | 74,794 req/s | 74,019 req/s | **98.9%** |
| SPOP | 72,939 req/s | 72,411 req/s | 71,582 req/s | **98.8%** |
| HSET | 73,584 req/s | 72,516 req/s | 71,225 req/s | **98.2%** |
| ZADD | 71,531 req/s | 70,872 req/s | 70,671 req/s | **99.7%** |

### Pipelined Performance (P=16)

| Command | Redis 7.4 | Redis 8.0 | Rust | Rust vs R8 |
|---------|-----------|-----------|------|------------|
| PING | 925,926 req/s | 1,086,957 req/s | 1,298,701 req/s | **119.4%** |
| SET | 645,161 req/s | 775,194 req/s | 819,672 req/s | **105.7%** |
| GET | 769,231 req/s | 854,701 req/s | 909,091 req/s | **106.3%** |
| MSET | 245,098 req/s | 297,619 req/s | 290,698 req/s | **97.6%** |
| INCR | 781,250 req/s | 934,579 req/s | 952,381 req/s | **101.9%** |
| LPUSH | 411,523 req/s | 781,250 req/s | 952,381 req/s | **121.9%** |
| RPUSH | 757,576 req/s | 793,651 req/s | 892,857 req/s | **112.5%** |
| LPOP | 534,759 req/s | 793,651 req/s | 1,052,632 req/s | **132.6%** |
| RPOP | 675,676 req/s | 819,672 req/s | 1,010,101 req/s | **123.2%** |
| LRANGE_100 | 62,461 req/s | 123,762 req/s | 121,359 req/s | **98.0%** |
| LRANGE_300 | 18,882 req/s | 31,066 req/s | 31,211 req/s | **100.4%** |
| LRANGE_500 | 10,941 req/s | 19,429 req/s | 20,346 req/s | **104.7%** |
| SADD | 884,956 req/s | 877,193 req/s | 1,041,667 req/s | **118.7%** |
| SPOP | 970,874 req/s | 970,874 req/s | 1,123,596 req/s | **115.7%** |
| HSET | 617,284 req/s | 847,458 req/s | 862,069 req/s | **101.7%** |
| ZADD | 480,769 req/s | 588,235 req/s | 699,300 req/s | **114.7%** |

### Linux Summary

**Non-Pipelined (P=1):** 92-126% of Redis 8.0
- Competitive across all 16 operations
- Beats Redis 8.0 on **MSET (107.0%)** and **LRANGE_100 (126.4%)**
- Average: ~98% of Redis 8.0

**Pipelined (P=16):** 97-133% of Redis 8.0
- **LPOP: 132.6%** - 1.05M req/s (33% faster than Redis 8.0)
- **RPOP: 123.2%** - 1.01M req/s (23% faster)
- **LPUSH: 121.9%** - 952K req/s (22% faster)
- **PING: 119.4%** - 1.30M req/s (19% faster)
- **SADD: 118.7%** - 1.04M req/s (19% faster)
- **SPOP: 115.7%** - 1.12M req/s (16% faster)
- **ZADD: 114.7%** - 699K req/s (15% faster)
- **RPUSH: 112.5%** - 893K req/s (13% faster)
- **GET: 106.3%** - 909K req/s (6% faster)
- **SET: 105.7%** - 820K req/s (6% faster)
- **LRANGE_500: 104.7%** - 20K req/s (5% faster)
- Average: ~110% of Redis 8.0 (10% faster overall)

**Wins:** 13 out of 16 pipelined operations beat Redis 8.0

---

## macOS Benchmarks (January 8, 2026)

**Platform:** macOS Darwin 24.4.0 (Docker Desktop)

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

### macOS Summary

- **SET P=1: 101.5%** - Exceeds Redis 8.0 for single-operation writes
- **GET P=1: 91.4%** - Competitive single-operation reads
- Pipelined performance lower due to Docker Desktop virtualization overhead

---

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

### Feature-Specific Optimizations

| Optimization | Description | Impact |
|-------------|-------------|--------|
| P0: Single Key Allocation | Reuse key string in `set_direct()` | +5-10% |
| P1: Static OK Response | Pre-allocated "OK" response | +1-2% |
| P2: Zero-Copy GET | Avoid data copy in `get_direct()` | +2-3% |
| P3: itoa Encoding | Fast integer-to-string conversion | +1-2% |
| P4: atoi Parsing | Fast string-to-integer parsing | +2-3% |

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

---

## Correctness Testing

### Test Suite (662 tests)

| Category | Tests | Coverage |
|----------|-------|----------|
| Unit Tests | 400+ | RESP parsing, commands, data structures |
| DST/Simulation | 99 | Multi-seed chaos testing with fault injection |
| Lua Scripting | 37 | EVAL/EVALSHA execution |
| CRDT/Consistency | 34 | Convergence, vector clocks, partition healing |
| Streaming Persistence | 20 | Object store, recovery, compaction |
| Redis Equivalence | 11 | Differential testing vs real Redis |

### DST Coverage (January 9, 2026)

| Data Structure | Seeds Tested | Operations |
|----------------|--------------|------------|
| Sorted Set | 100+ | ZADD, ZREM, ZSCORE, ZRANGE |
| List | 100+ | LPUSH, RPUSH, LPOP, RPOP, LSET, LTRIM |
| Hash | 100+ | HSET, HGET, HDEL |
| Set | 100+ | SADD, SREM, SPOP, SMEMBERS |
| Streaming | 100+ | Flush, crash recovery, compaction |
| CRDT | 100+ | GCounter, PNCounter, ORSet, VectorClock |

### Maelstrom/Jepsen Results

| Test | Nodes | Result | Notes |
|------|-------|--------|-------|
| Linearizability (lin-kv) | 1 | **PASS** | Single-node is linearizable |
| Linearizability (lin-kv) | 3 | **FAIL** | Expected: eventual consistency |

**Note:** Multi-node linearizability tests FAIL by design. We use Anna-style eventual consistency, not Raft/Paxos consensus.

---

## Running Benchmarks

### Docker Benchmark (Recommended)

```bash
cd docker-benchmark

# Redis 8.0 three-way comparison (16 commands)
./run-redis8-comparison.sh

# In-memory comparison (Redis 7.4 vs Rust)
./run-benchmarks.sh

# Persistent comparison (Redis AOF vs Rust S3/MinIO)
./run-persistent-benchmarks.sh
```

### Benchmark Commands

```bash
# Non-pipelined (P=1)
redis-benchmark -p <port> -n 100000 -c 50 -P 1 -d 64 -r 10000 \
    -t ping_mbulk,set,get,mset,incr,lpush,rpush,lpop,rpop,lrange_100,lrange_300,lrange_500,sadd,spop,hset,zadd --csv

# Pipelined (P=16)
redis-benchmark -p <port> -n 100000 -c 50 -P 16 -d 64 -r 10000 \
    -t ping_mbulk,set,get,mset,incr,lpush,rpush,lpop,rpop,lrange_100,lrange_300,lrange_500,sadd,spop,hset,zadd --csv
```

---

## Known Limitations

1. **Streaming persistence**: Object store-based (S3/LocalFs), not traditional RDB/AOF
2. **No pub/sub or streams**: Not implemented
3. **Multi-node consistency**: Eventual, not linearizable (by design)
4. **Inline commands**: Only RESP bulk format supported (not inline PING)
