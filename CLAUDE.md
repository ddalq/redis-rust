# Claude Code Guidelines for redis-rust

## Project Philosophy

This project follows **Simulation-First Development** inspired by FoundationDB and TigerBeetle. The core principle: **if you can't simulate it, you can't test it properly**.

## Architecture Principles

### 1. Deterministic Simulation Testing (DST)

All I/O operations must go through abstractions that can be:
- **Simulated**: Deterministic, controllable behavior
- **Fault-injected**: Network partitions, disk failures, message drops
- **Time-controlled**: Fast-forward time, test timeout scenarios

```rust
// GOOD: I/O through trait abstraction
trait ObjectStore: Send + Sync {
    async fn put(&self, key: &str, data: &[u8]) -> Result<()>;
    async fn get(&self, key: &str) -> Result<Vec<u8>>;
}

// BAD: Direct I/O that can't be simulated
std::fs::write(path, data)?;
```

### 2. Actor Architecture

Components communicate via message passing, not shared mutable state:

```rust
// GOOD: Actor owns state exclusively
struct PersistenceActor {
    state: PersistenceState,  // Owned, not shared
    rx: mpsc::Receiver<Message>,
}

// BAD: Shared state with mutex
struct SharedPersistence {
    state: Arc<Mutex<PersistenceState>>,  // Contention, hard to test
}
```

### 3. TigerStyle Coding

- **Assertions**: Use `debug_assert!` liberally for invariants
- **No hidden allocations**: Be explicit about where memory is allocated
- **Simple control flow**: Prefer early returns, avoid deep nesting
- **Explicit errors**: No panics in production paths, explicit `Result<T, E>`

### 4. Static Stability

Systems must remain stable under partial failures:
- Graceful degradation when dependencies fail
- Bounded queues with backpressure
- Timeouts on all external operations

## Testing Strategy

### Unit Tests
- Test pure logic without I/O
- Use `InMemoryObjectStore` for storage tests

### Simulation Tests (DST)
- Use `SimulatedObjectStore` with fault injection
- Control time via `SimulatedClock`
- Run thousands of seeds to find edge cases

### Linearizability Tests (Jepsen-style)
- Use Maelstrom for distributed correctness
- Test under network partitions
- Verify consistency guarantees

## Key Files

| File | Purpose |
|------|---------|
| `src/io/mod.rs` | I/O abstractions (Clock, Network, RNG) |
| `src/simulator/` | DST harness and fault injection |
| `src/streaming/simulated_store.rs` | Fault-injectable object store |
| `src/buggify/` | Probabilistic fault injection |

## Common Patterns

### Creating Testable Components

```rust
// 1. Define trait for the I/O operation
pub trait ObjectStore: Send + Sync + 'static {
    fn put(&self, key: &str, data: &[u8]) -> impl Future<Output = IoResult<()>>;
}

// 2. Create production implementation
pub struct LocalFsObjectStore { path: PathBuf }

// 3. Create simulated implementation with fault injection
pub struct SimulatedObjectStore {
    inner: InMemoryObjectStore,
    fault_config: FaultConfig,
    rng: SimulatedRng,
}

// 4. Use generic in component
pub struct StreamingPersistence<S: ObjectStore> {
    store: S,
    // ...
}
```

### Actor Shutdown Pattern

```rust
enum Message {
    DoWork(Work),
    Shutdown { response: oneshot::Sender<()> },
}

async fn run(mut self) {
    while let Some(msg) = self.rx.recv().await {
        match msg {
            Message::DoWork(w) => self.handle_work(w).await,
            Message::Shutdown { response } => {
                self.cleanup().await;
                let _ = response.send(());
                break;
            }
        }
    }
}
```

### Bridging Sync to Async

When you need to connect sync code (like command execution) to async actors:

```rust
// Use std::sync::mpsc for fire-and-forget from sync context
let (tx, rx) = std::sync::mpsc::channel();

// Bridge task drains sync channel into async actor
async fn bridge(rx: Receiver<Delta>, actor: ActorHandle) {
    loop {
        // Use recv_timeout to stay responsive to shutdown
        if let Some(delta) = rx.recv_timeout(Duration::from_millis(50)) {
            actor.send(delta);
        }
        if shutdown.load(Ordering::SeqCst) {
            break;
        }
    }
}
```

## Debugging Tips

1. **Seed-based reproduction**: All simulations use seeds. Save failing seeds to reproduce bugs.
2. **Trace logging**: Use `tracing` crate with structured logging.
3. **Invariant checks**: Add assertions for state invariants after each operation.

## Performance Guidelines

1. **Batch operations**: Accumulate deltas in write buffer before flushing
2. **Async I/O**: Use `tokio::spawn` for background operations
3. **Zero-copy where possible**: Use `Bytes` for large data transfers
4. **Profile before optimizing**: Use `cargo flamegraph`

## Dependencies

Core dependencies and their purposes:
- `tokio`: Async runtime
- `bincode`: Binary serialization (3-5x smaller than JSON)
- `crc32fast`: Checksums for data integrity
- `tracing`: Structured logging
