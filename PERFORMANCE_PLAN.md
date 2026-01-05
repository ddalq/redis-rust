# Performance Optimization Plan

Based on [Abseil Performance Tips](https://abseil.io/fast/hints.html), tailored for this Redis implementation.

---

## Current Architecture Recap

```
Client → TCP → Connection Handler → hash(key) → Shard Actor → CommandExecutor
                    ↓
              Buffer Pool (crossbeam::ArrayQueue)
```

**Already Implemented:**
- jemalloc allocator (~10% improvement)
- Actor-per-shard lock-free design (~30% improvement)
- Buffer pooling (~20% improvement)
- Zero-copy RESP parser with `bytes::Bytes` (~15% improvement)
- TCP_NODELAY for latency
- Batched response flushing

---

## Phase 1: Low-Hanging Fruit (High Impact, Low Risk)

### 1.1 Bulk APIs (Abseil Tip #5)
**Problem**: MGET/MSET cross shard boundaries, requiring multiple actor messages.

**Current**:
```rust
// MGET sends N messages to potentially N shards
for key in keys {
    let shard = hash(key) % num_shards;
    shard_tx[shard].send(GetCommand(key)).await;
}
```

**Optimization**:
```rust
// Batch by shard, single message per shard
let batches: HashMap<usize, Vec<Key>> = group_by_shard(keys);
for (shard, batch) in batches {
    shard_tx[shard].send(BatchGetCommand(batch)).await;
}
```

**Files**: `src/production/connection_optimized.rs`, `src/production/sharded_actor.rs`
**Estimated Impact**: 20-40% improvement on MGET/MSET with many keys

---

### 1.2 Pre-allocated Responses (Abseil Tip #7)
**Problem**: Each response allocates a new `BytesMut`.

**Current**:
```rust
fn format_response(value: &RespValue) -> BytesMut {
    let mut buf = BytesMut::with_capacity(64);  // allocation
    // ... format
    buf
}
```

**Optimization**:
```rust
fn format_response_into(value: &RespValue, buf: &mut BytesMut) {
    buf.clear();
    // ... format into existing buffer
}

// Connection owns a reusable response buffer
struct Connection {
    response_buf: BytesMut,  // reused across commands
}
```

**Files**: `src/redis/resp.rs`, `src/production/connection_optimized.rs`
**Estimated Impact**: 5-10% reduction in allocator pressure

---

### 1.3 Reserve Container Capacity (Abseil Tip #19)
**Problem**: Vectors grow incrementally during command parsing.

**Audit these locations**:
- `Command::MSet(Vec<(String, SDS)>)` - reserve based on arg count
- `Command::MGet(Vec<String>)` - reserve based on arg count
- Hash operations: `HGETALL` result building

```rust
// Before
let mut pairs = Vec::new();
for chunk in args.chunks(2) {
    pairs.push(...);
}

// After
let mut pairs = Vec::with_capacity(args.len() / 2);
for chunk in args.chunks(2) {
    pairs.push(...);
}
```

**Files**: `src/redis/commands.rs`, `src/redis/resp_optimized.rs`
**Estimated Impact**: 2-5% on multi-key operations

---

### 1.4 Fast Path for Common Cases (Abseil Tip #22)
**Problem**: Every command goes through the same parsing/dispatch path.

**Optimization**: Add fast paths for GET/SET (80%+ of traffic):

```rust
// In connection handler, before full RESP parsing
fn try_fast_path(buf: &[u8]) -> Option<FastCommand> {
    // Quick check for *3\r\n$3\r\nSET or *2\r\n$3\r\nGET
    if buf.starts_with(b"*3\r\n$3\r\nSET") {
        return Some(parse_set_fast(buf));
    }
    if buf.starts_with(b"*2\r\n$3\r\nGET") {
        return Some(parse_get_fast(buf));
    }
    None  // Fall back to general parser
}
```

**Files**: `src/production/connection_optimized.rs`
**Estimated Impact**: 10-15% on GET/SET-heavy workloads

---

## Phase 2: Data Structure Optimizations (Medium Risk)

### 2.1 Compact Data Structures (Abseil Tip #10)
**Problem**: `ReplicatedValue` is 72+ bytes, most fields cold.

**Current** (estimate):
```rust
struct ReplicatedValue {
    crdt: CrdtValue,         // 40 bytes (enum with variants)
    timestamp: LamportClock, // 16 bytes
    expiry_ms: Option<u64>,  // 16 bytes
    vector_clock: Option<VectorClock>, // 24 bytes (Box)
}
```

**Optimization**: Separate hot/cold data:
```rust
// Hot path (fits in cache line)
struct ReplicatedValueHot {
    value_ptr: u32,      // index into value storage
    timestamp: u64,      // compressed clock
    expiry_ms: u64,      // 0 = no expiry
}

// Cold path (accessed rarely)
struct ReplicatedValueCold {
    vector_clock: Option<VectorClock>,
    crdt_metadata: CrdtMetadata,
}
```

**Files**: `src/replication/state.rs`
**Estimated Impact**: 10-20% improvement in memory bandwidth

---

### 2.2 Indices Instead of Pointers (Abseil Tip #11)
**Problem**: HashMap<String, ReplicatedValue> has pointer overhead.

**Optimization**: Use arena allocation with indices:
```rust
struct ShardStorage {
    // Keys stored contiguously
    keys: StringArena,
    // Values stored contiguously
    values: Vec<ReplicatedValueHot>,
    // Index: key_hash -> value_index
    index: HashMap<u64, u32>,
}
```

**Files**: `src/redis/commands.rs` (storage layer)
**Estimated Impact**: 15-25% memory reduction, better cache locality
**Risk**: Significant refactor

---

### 2.3 Small String Optimization (Abseil Tip #13)
**Problem**: Every key/value is a heap-allocated String/SDS.

**Optimization**: Inline small strings (Redis does this):
```rust
enum CompactString {
    Inline { len: u8, data: [u8; 23] },  // 24 bytes total, no alloc
    Heap(String),
}

impl CompactString {
    fn new(s: &str) -> Self {
        if s.len() <= 23 {
            let mut data = [0u8; 23];
            data[..s.len()].copy_from_slice(s.as_bytes());
            CompactString::Inline { len: s.len() as u8, data }
        } else {
            CompactString::Heap(s.to_string())
        }
    }
}
```

**Files**: `src/redis/sds.rs`
**Estimated Impact**: 20-30% reduction in allocations for typical workloads
**Note**: Most Redis keys are <24 bytes

---

## Phase 3: Algorithmic Improvements (High Impact, High Risk)

### 3.1 Batch Gossip Deltas (Abseil Tip #5, #25)
**Problem**: Each write generates a delta sent immediately to gossip.

**Current**:
```rust
fn execute_set(&mut self, key: String, value: SDS) {
    self.data.insert(key.clone(), value);
    self.delta_sink.send(Delta::Set(key, value));  // immediate
}
```

**Optimization**: Batch deltas, flush on interval:
```rust
struct DeltaBatcher {
    pending: Vec<Delta>,
    last_flush: Instant,
    flush_interval: Duration,  // e.g., 10ms
}

fn execute_set(&mut self, key: String, value: SDS) {
    self.data.insert(key.clone(), value);
    self.batcher.push(Delta::Set(key, value));

    if self.batcher.should_flush() {
        self.flush_deltas();
    }
}
```

**Files**: `src/production/replicated_shard_actor.rs`, `src/streaming/delta_sink.rs`
**Estimated Impact**: 30-50% reduction in gossip overhead

---

### 3.2 Specialize Hash Commands (Abseil Tip #26)
**Problem**: HINCRBY goes through generic command dispatch.

**Current**:
```rust
Command::HIncrBy(key, field, delta) => {
    let hash = self.get_or_create_hash(&key);
    hash.incrby(&field, delta)
}
```

**Optimization**: Direct path for hot commands:
```rust
// Specialized actor message
enum ShardMessage {
    Generic(Command),
    // Hot path: skip command parsing entirely
    HIncrByFast { key_hash: u64, field_hash: u64, delta: i64 },
}
```

**Files**: `src/production/sharded_actor.rs`
**Estimated Impact**: 15-20% on HINCRBY-heavy workloads (metrics use case)

---

### 3.3 Lock-Free TTL Scanning (Abseil Tip #8)
**Problem**: TTL manager sends messages to all shards every 100ms.

**Current**:
```rust
// TTL manager actor
loop {
    sleep(100ms);
    for shard in &shards {
        shard.send(EvictExpired).await;
    }
}
```

**Optimization**: Shards track their own TTLs with a heap:
```rust
struct ShardState {
    data: HashMap<String, Value>,
    // Min-heap of (expiry_time, key)
    expiry_heap: BinaryHeap<Reverse<(u64, String)>>,
}

fn check_expiry(&mut self, now: u64) {
    while let Some(Reverse((expiry, key))) = self.expiry_heap.peek() {
        if *expiry > now { break; }
        self.expiry_heap.pop();
        self.data.remove(key);
    }
}
```

**Files**: `src/production/sharded_actor.rs`, `src/production/ttl_manager.rs`
**Estimated Impact**: Eliminates 100ms message storm, smoother latency

---

## Phase 4: Advanced Optimizations

### 4.1 SIMD Key Hashing
**Problem**: `hash(key)` on every operation.

**Optimization**: Use SIMD-accelerated hashing:
```rust
// Use ahash (already a dependency) or xxhash
use ahash::AHasher;

fn hash_key(key: &[u8]) -> u64 {
    let mut hasher = AHasher::default();
    hasher.write(key);
    hasher.finish()
}
```

**Note**: Already using ahash, verify it's used everywhere.

---

### 4.2 io_uring for Linux (Future)
**Problem**: Each TCP read/write is a syscall.

**Optimization**: Use io_uring for batched I/O:
```rust
#[cfg(target_os = "linux")]
use tokio_uring::net::TcpStream;
```

**Estimated Impact**: 20-30% on high-connection workloads
**Risk**: Platform-specific, requires significant refactor

---

### 4.3 Connection Affinity
**Problem**: Connections can be handled by any thread.

**Optimization**: Pin connections to cores, keep cache hot:
```rust
// Route connection to worker based on client IP hash
let worker_id = hash(client_ip) % num_workers;
workers[worker_id].spawn(handle_connection(stream));
```

**Files**: `src/production/server_optimized.rs`
**Estimated Impact**: 5-10% from better cache locality

---

## Measurement Plan

### Microbenchmarks (Fast Iteration)
```bash
cargo bench  # Add criterion benchmarks
```

Add benchmarks for:
- [ ] RESP parsing (various command sizes)
- [ ] Key hashing
- [ ] Shard message dispatch
- [ ] Delta serialization

### Integration Benchmarks
```bash
./docker-benchmark/run-detailed-benchmarks.sh
```

Measure:
- [ ] p50/p95/p99 latency
- [ ] CPU% per operation
- [ ] Memory per key
- [ ] Allocation rate (via jemalloc stats)

### Profiling
```bash
# CPU profile
cargo flamegraph --bin redis-server-optimized

# Memory profile
MALLOC_CONF=prof:true cargo run --release --bin redis-server-optimized
```

---

## Implementation Priority

| Phase | Optimization | Impact | Risk | Effort |
|-------|-------------|--------|------|--------|
| 1.4 | Fast path GET/SET | High | Low | 2 days |
| 1.1 | Bulk APIs (MGET batch) | High | Low | 2 days |
| 1.2 | Pre-allocated responses | Medium | Low | 1 day |
| 1.3 | Reserve capacity | Low | Low | 0.5 day |
| 2.3 | Small string optimization | High | Medium | 3 days |
| 3.1 | Batch gossip deltas | High | Medium | 2 days |
| 3.3 | Lock-free TTL | Medium | Medium | 2 days |
| 2.1 | Compact structs | Medium | High | 3 days |
| 2.2 | Arena allocation | High | High | 5 days |

**Recommended Order**: 1.4 → 1.1 → 2.3 → 3.1 → 1.2 → 3.3

---

## Anti-Patterns to Avoid

1. **Premature SIMD**: Profile first, most gains come from algorithmic changes
2. **Over-specialization**: Don't special-case commands used <1% of the time
3. **Breaking the abstraction**: Keep DST compatibility, don't bypass actors
4. **Micro-optimizing cold paths**: Focus on the hot 3% (Knuth's rule)
5. **Platform-specific hacks**: Keep cross-platform unless gains are huge

---

## Success Criteria

- [ ] P99 latency ≤ Redis 7.4 p99 (currently unknown)
- [ ] Memory per key ≤ Redis 7.4 (currently unknown)
- [ ] Pipelined throughput ≥ 1.2M req/sec (currently ~1M)
- [ ] No regression in DST test pass rate
