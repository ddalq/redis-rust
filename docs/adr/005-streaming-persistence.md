# ADR-005: Streaming Persistence to Object Store

## Status

Accepted

## Context

Redis provides two persistence mechanisms:

1. **RDB (Snapshotting)**: Periodic point-in-time snapshots to disk
2. **AOF (Append-Only File)**: Log every write operation

Both assume local disk storage, which creates challenges for cloud-native deployments:

- **Disk sizing**: Must provision for peak storage
- **Recovery time**: Large snapshots take minutes to load
- **Cross-region**: No built-in replication to remote regions
- **Cost**: Local NVMe is expensive vs object storage

Modern cloud-native systems like WarpStream and Redpanda have shown that **streaming directly to object storage** (S3, GCS, MinIO) provides:

- **Elastic storage**: Pay for what you use
- **Durability**: 11 9's durability in S3
- **Cross-region**: Native replication to any region
- **Cost efficiency**: ~10x cheaper than EBS/NVMe

## Decision

We will implement **streaming persistence to object store** instead of traditional RDB/AOF:

### Architecture

```
Commands                     Background Flush                  Object Store
    |                              |                               |
[Command Executor]           [Persistence Actor]             [S3/MinIO/LocalFs]
    |                              |                               |
[Delta Capture]  -------->   [Write Buffer]  ---------->   [Segment Files]
    |                              |                               |
[DeltaSink]                  [Compaction]                    [Manifest]
```

### Segment Format

Binary segments with:
- **Header**: Magic bytes, version, CRC32
- **Delta log**: Bincode-serialized operations
- **Footer**: Checksums, offsets

### Persistence Flow

```rust
pub trait ObjectStore: Send + Sync + 'static {
    fn put(&self, key: &str, data: &[u8]) -> impl Future<Output = IoResult<()>>;
    fn get(&self, key: &str) -> impl Future<Output = IoResult<Vec<u8>>>;
    fn list(&self, prefix: &str) -> impl Future<Output = IoResult<Vec<String>>>;
    fn delete(&self, key: &str) -> impl Future<Output = IoResult<()>>;
}
```

### Write Buffer Strategy

```rust
pub struct WriteBuffer {
    buffer: Vec<Delta>,
    size_bytes: usize,
    oldest_timestamp: Option<Instant>,
}

impl WriteBuffer {
    pub fn should_flush(&self, config: &WriteBufferConfig) -> bool {
        self.size_bytes >= config.max_size_bytes
            || self.oldest_timestamp
                .map(|t| t.elapsed() >= config.max_age)
                .unwrap_or(false)
    }
}
```

### Recovery

1. List segments from manifest
2. Load segments in order (oldest first)
3. Replay deltas to rebuild state
4. Resume accepting commands

## Consequences

### Positive

- **Cloud-native**: Works with S3, GCS, Azure Blob, MinIO
- **Cost efficiency**: Object storage is cheap and elastic
- **Durability**: Leverage cloud provider's durability guarantees
- **Testability**: SimulatedObjectStore enables DST for persistence
- **Cross-region**: Native replication via object store features

### Negative

- **Latency**: Object store writes have higher latency than local disk
- **Complexity**: Must handle partial failures, retries, checksums
- **Recovery time**: May be slower than memory-mapped RDB
- **No AOF semantics**: Lose sub-second durability guarantee

### Risks

- **Object store availability**: Depends on cloud provider SLA
- **Cost unpredictability**: High write rates may be expensive
- **Consistency window**: Data in write buffer not yet durable
- **Segment corruption**: Must detect and handle corrupted segments

## Decision Log

| Date | Decision | Rationale |
|------|----------|-----------|
| 2026-01-03 | Initial ADR created | Cloud-native persistence for elastic storage |
| 2026-01-03 | Use bincode for serialization | 3-5x smaller than JSON, fast |
| 2026-01-04 | Add CRC32 checksums | Detect corruption in segments |
| 2026-01-04 | Implement SimulatedObjectStore | Enable DST for persistence |
| 2026-01-05 | Add write buffer with age/size limits | Balance durability and batching |
| 2026-01-06 | Implement compaction | Merge small segments, garbage collect |
| 2026-01-07 | Add manifest for segment tracking | Atomic updates to segment list |
| 2026-01-08 | Implement recovery with checkpointing | Faster recovery with periodic full snapshots |

## Implementation Status

### Implemented

| Component | Location | Status |
|-----------|----------|--------|
| ObjectStore trait | `src/streaming/object_store.rs` | S3, LocalFs implementations |
| S3Store | `src/streaming/s3_store.rs` | AWS S3 via object_store crate |
| SimulatedObjectStore | `src/streaming/simulated_store.rs` | Fault-injectable for DST |
| WriteBuffer | `src/streaming/write_buffer.rs` | Batching with size/age limits |
| Segment format | `src/streaming/segment.rs` | Binary format with checksums |
| PersistenceActor | `src/streaming/persistence.rs` | Background flush actor |
| DeltaSink | `src/streaming/delta_sink.rs` | Capture deltas from commands |
| Compaction | `src/streaming/compaction.rs` | Merge and garbage collect |
| Manifest | `src/streaming/manifest.rs` | Atomic segment tracking |
| Recovery | `src/streaming/recovery.rs` | State reconstruction |
| Checkpointing | `src/streaming/checkpoint.rs` | Periodic full snapshots |
| DST tests | `src/streaming/dst.rs` | Multi-seed fault injection |

### Validated

- DST tests with fault injection pass
- Recovery reconstructs state correctly
- Compaction reduces segment count
- Checkpointing speeds recovery

### Not Yet Implemented

| Component | Notes |
|-----------|-------|
| Incremental snapshots | Full snapshots only |
| Tiered storage | No hot/cold tiering |
| Encryption at rest | No client-side encryption |

## References

- [WarpStream Architecture](https://www.warpstream.com/blog/warpstream-technical-deep-dive)
- [Redpanda Tiered Storage](https://redpanda.com/blog/tiered-storage-architecture)
- [AWS S3 Durability](https://aws.amazon.com/s3/features/)
- [Object Store Crate](https://docs.rs/object_store/latest/object_store/)
