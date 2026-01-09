# Implementation Plan: Addressing HN-Style Feedback

This plan addresses the feedback to make the project more credible to skeptical systems engineers.

---

## 1. Rename the "Production-Ready" Claim

**Problem**: The README claims "production-ready" but has major feature gaps (no auth, no pub/sub, different consistency model).

**Changes**:

### README.md (Line 1-3)
- **Before**: "A production-ready, actor-based Redis cache server..."
- **After**: "An experimental, actor-based Redis-compatible cache server with production-oriented architecture and correctness tooling..."

### README.md - Add Experimental Banner
Add a clear status section after the title:
```markdown
> **Status**: Experimental research project. Production-oriented architecture with deterministic simulation testing, but not yet a production Redis replacement. See [Compatibility](#redis-compatibility) for differences.
```

### BENCHMARK_RESULTS.md
- Update language to reflect experimental status
- Remove any "production-ready" claims

---

## 2. Make Redis Compatibility Contract Explicit

**Problem**: Users need to know exactly what's identical vs intentionally different from Redis.

**Changes**:

### README.md - New Section: "Redis Compatibility"
Add after "Features" section:

```markdown
## Redis Compatibility

### Wire Protocol
- **RESP2**: Full support (compatible with all Redis clients)
- **RESP3**: Not supported

### Semantic Differences from Redis

| Behavior | Redis | This Implementation |
|----------|-------|---------------------|
| **Consistency** | Single-leader strong | Eventual/Causal (multi-node) |
| **Transactions** | MULTI/EXEC atomic | Not supported |
| **Keyspace Notifications** | Supported | Not supported |
| **Eviction Policies** | LRU/LFU/Random | TTL-only |
| **Memory Limits** | maxmemory + policy | No memory limits |
| **Persistence** | RDB/AOF | Streaming to object store |
| **Cluster Protocol** | Redis Cluster | Anna-style CRDT gossip |

### Supported Commands (50+)
[existing command list]

### Not Implemented
- Pub/Sub (PUBLISH, SUBSCRIBE, PSUBSCRIBE)
- Lua scripting (EVAL, EVALSHA)
- Streams (XADD, XREAD, XRANGE)
- Transactions (MULTI, EXEC, WATCH)
- ACL/Authentication
- Cluster commands (CLUSTER *)
- Blocking operations (BLPOP, BRPOP)
```

---

## 3. Enhance Benchmarks with Tail Latencies and Resource Metrics

**Problem**: Throughput alone isn't convincing. Need p95/p99 latency, CPU%, and memory usage.

**Changes**:

### docker-benchmark/run-benchmarks.sh
Modify to capture:
1. Latency percentiles (p50, p95, p99)
2. CPU usage during benchmark
3. Memory (RSS) before/after
4. Exact command lines for reproduction

### BENCHMARK_RESULTS.md - Add New Sections

```markdown
## Latency Distribution (Docker, P=1, 100K requests)

| Percentile | Redis 7.4 | Rust Implementation |
|------------|-----------|---------------------|
| p50 | X.XX ms | X.XX ms |
| p95 | X.XX ms | X.XX ms |
| p99 | X.XX ms | X.XX ms |
| p99.9 | X.XX ms | X.XX ms |

## Resource Usage During Benchmark

| Metric | Redis 7.4 | Rust Implementation |
|--------|-----------|---------------------|
| Peak CPU % | XX% | XX% |
| Avg CPU % | XX% | XX% |
| RSS (idle) | XX MB | XX MB |
| RSS (under load) | XX MB | XX MB |

## Reproduction Commands

```bash
# Start containers
docker run -d --name redis-bench --cpus=2 --memory=1g redis:7.4
docker run -d --name rust-bench --cpus=2 --memory=1g redis-rust:latest

# Run benchmark with latency histogram
redis-benchmark -h localhost -p 6379 -n 100000 -c 50 --csv -t set,get

# Capture resource usage
docker stats --no-stream redis-bench rust-bench
```
```

### New benchmark script: docker-benchmark/run-detailed-benchmarks.sh
Create script that:
1. Runs redis-benchmark with latency histogram output
2. Uses `docker stats` to capture CPU/memory
3. Outputs structured results for documentation

---

## 4. Runtime Hygiene: Fix Constructor/Runtime Coupling

**Problem**: `ReplicatedShardedState::new()` requires an active Tokio runtime because it spawns actors internally.

**Changes**:

### Option A: Async Constructor (Recommended)
```rust
// Before (src/production/replicated_state.rs)
impl ReplicatedShardedState {
    pub fn new(config: ReplicationConfig) -> Self {
        // spawns actors internally - requires runtime
    }
}

// After
impl ReplicatedShardedState {
    pub async fn new(config: ReplicationConfig) -> Self {
        // Can now safely spawn actors
    }

    // Alternative: pass handle explicitly
    pub fn with_handle(config: ReplicationConfig, handle: &tokio::runtime::Handle) -> Self {
        // Use handle.spawn() instead of tokio::spawn()
    }
}
```

### Files to Update:
- `src/production/replicated_state.rs` - Change `new()` to `async fn new()`
- `src/production/sharded_actor.rs` - Update actor spawning
- `src/bin/maelstrom_kv_replicated.rs` - Already uses `rt.block_on()`, will work
- `src/bin/server_persistent.rs` - Update to use async context
- Tests that create `ReplicatedShardedState`

### Alternative: Builder Pattern with Explicit Handle
```rust
impl ReplicatedShardedState {
    pub fn builder() -> ReplicatedShardedStateBuilder { ... }
}

impl ReplicatedShardedStateBuilder {
    pub fn with_runtime_handle(mut self, handle: Handle) -> Self { ... }
    pub fn build(self) -> ReplicatedShardedState { ... }
}
```

---

## 5. Add Security Disclaimer

**Problem**: No auth/ACL means anyone who exposes this to the internet creates a security incident.

**Changes**:

### README.md - Add Security Warning (Top of File)
After the experimental status banner:

```markdown
> **Security Warning**: This server has **no authentication or access control**.
> Do NOT expose to untrusted networks or the public internet.
> Bind to localhost or use network-level access control (firewall, VPC).
```

### README.md - Security Section
Add new section before "License":

```markdown
## Security Considerations

### What's NOT Implemented
- **Authentication**: No AUTH command, no password protection
- **ACL**: No user-based access control
- **TLS**: No encrypted connections
- **Command Restrictions**: All commands available to all clients

### Deployment Recommendations
1. **Never expose to public internet**
2. Bind to `127.0.0.1` or private network interfaces only
3. Use network-level access control (iptables, security groups, VPC)
4. Run in isolated container/namespace
5. Monitor for unauthorized access attempts

### Roadmap
- [ ] AUTH command support
- [ ] TLS encryption
- [ ] Basic ACL
```

---

## 6. Bonus: Improve Assertion Organization

**Problem**: The `#[cfg(debug_assertions)]` pattern is spreading and could become hard to maintain.

**Changes**:

### Create `src/invariants.rs` Module
Centralize invariant checking:

```rust
/// Macro for invariant checks that only run in debug builds
#[macro_export]
macro_rules! verify {
    ($cond:expr, $msg:expr) => {
        #[cfg(debug_assertions)]
        {
            debug_assert!($cond, $msg);
        }
    };
}

/// Trait for types with checkable invariants
pub trait Invariant {
    /// Check all invariants. Only runs in debug builds.
    fn verify_invariants(&self);
}
```

### Update Data Structures
Implement `Invariant` trait for:
- `RedisSortedSet`
- `RedisHash`
- `RedisList`
- `RedisSet`
- CRDT types in `src/replication/lattice.rs`

---

## Implementation Order

1. **Security Disclaimer** (30 min) - Critical, immediate impact
2. **Rename Claims** (30 min) - Quick documentation fix
3. **Compatibility Contract** (1 hr) - Documentation improvement
4. **Enhanced Benchmarks** (2-3 hr) - New scripts + data collection
5. **Runtime Hygiene** (2-3 hr) - Code changes, test updates
6. **Assertion Organization** (1-2 hr) - Refactoring, optional

---

## Files Changed Summary

| File | Changes |
|------|---------|
| `README.md` | Status banner, security warning, compatibility section |
| `BENCHMARK_RESULTS.md` | Latency/resource tables, reproduction commands |
| `docker-benchmark/run-detailed-benchmarks.sh` | New script |
| `src/production/replicated_state.rs` | Async constructor |
| `src/production/sharded_actor.rs` | Handle-aware spawning |
| `src/invariants.rs` | New module (optional) |
| `src/bin/*.rs` | Update for async constructors |
