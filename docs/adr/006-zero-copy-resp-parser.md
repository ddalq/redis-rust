# ADR-006: Zero-Copy RESP Parser

## Status

Accepted

## Context

Redis uses the RESP (REdis Serialization Protocol) for client-server communication. A naive parser implementation allocates new strings for every command, which becomes a bottleneck at high throughput:

- **100K req/s with 64-byte values**: 6.4 MB/s of allocations
- **1M req/s with pipelining**: 64 MB/s of allocations

Profiling showed that string allocation and copying accounted for ~20% of CPU time in the hot path.

Zero-copy parsing techniques can eliminate most allocations by:
1. Using references into the input buffer instead of copying
2. Reusing buffers across requests
3. Leveraging SIMD-optimized scanning (memchr)

## Decision

We will implement a **zero-copy RESP parser** using:

### 1. Bytes Crate for Buffer Management

```rust
use bytes::{Bytes, BytesMut};

pub struct RespCodec {
    buffer: BytesMut,
}

impl Decoder for RespCodec {
    type Item = Value;
    type Error = Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Value>, Error> {
        // Parse returns Bytes (zero-copy slice of src)
        parse_value(src)
    }
}
```

### 2. Zero-Copy Value Type

```rust
pub enum RespValueZeroCopy<'a> {
    SimpleString(&'a [u8]),
    Error(&'a [u8]),
    Integer(i64),
    BulkString(Bytes),  // Zero-copy reference
    Array(Vec<RespValueZeroCopy<'a>>),
    Null,
}
```

### 3. memchr for Fast CRLF Scanning

```rust
use memchr::memchr;

fn find_crlf(data: &[u8]) -> Option<usize> {
    let pos = memchr(b'\r', data)?;
    if data.get(pos + 1) == Some(&b'\n') {
        Some(pos)
    } else {
        find_crlf(&data[pos + 1..]).map(|p| p + pos + 1)
    }
}
```

### 4. Parser Architecture

```
[Network Buffer]
       |
  [memchr] for CRLF scanning (SIMD-optimized)
       |
  [bytes::Bytes] zero-copy slicing
       |
  [RespValueZeroCopy] borrowed references
       |
  [Command extraction] (only copies when needed)
```

### Performance Impact

| Optimization | Improvement |
|-------------|-------------|
| Zero-copy Bytes | ~10% latency reduction |
| memchr SIMD | ~5% throughput increase |
| Buffer pooling | ~5% allocation reduction |
| Combined | ~15-20% overall improvement |

## Consequences

### Positive

- **Reduced allocations**: Most commands don't allocate for parsing
- **Lower latency**: Less time spent in allocator
- **Better cache utilization**: Data stays in L1/L2 cache
- **SIMD utilization**: memchr uses AVX2/SSE on x86

### Negative

- **Lifetime complexity**: Zero-copy requires careful lifetime management
- **Bytes dependency**: Additional crate dependency
- **Partial compatibility**: Some commands may still need copies
- **Debugging difficulty**: References harder to inspect than owned data

### Risks

- **Buffer lifetime**: Must ensure buffer outlives parsed values
- **Memory fragmentation**: Long-lived Bytes references may pin memory
- **Correctness**: Zero-copy parsing is easy to get wrong

## Decision Log

| Date | Decision | Rationale |
|------|----------|-----------|
| 2026-01-03 | Initial ADR created | Zero-copy needed for performance |
| 2026-01-03 | Use bytes crate | Industry standard, well-tested |
| 2026-01-04 | Use memchr for scanning | SIMD-optimized, 2-3x faster than manual |
| 2026-01-05 | Implement RespValueZeroCopy | Minimize allocations in hot path |
| 2026-01-05 | Add buffer pooling with crossbeam | Reuse BytesMut across connections |
| 2026-01-06 | Keep both parsers | resp.rs for compatibility, resp_optimized.rs for performance |

## Implementation Status

### Implemented

| Component | Location | Status |
|-----------|----------|--------|
| RespParser | `src/redis/resp.rs` | Standard parser |
| RespParserOptimized | `src/redis/resp_optimized.rs` | Zero-copy parser |
| memchr scanning | `src/redis/resp_optimized.rs` | SIMD CRLF detection |
| Buffer pooling | `src/production/response_pool.rs` | crossbeam::ArrayQueue reuse |
| Bytes integration | Throughout | Zero-copy value handling |
| Hot path benchmarks | `benches/hot_paths.rs` | Criterion benchmarks |

### Validated

- Benchmarks show ~15% improvement over naive parser
- Zero-copy parsing verified with DST
- Buffer pooling reduces allocation pressure
- memchr uses SIMD on supported platforms

### Not Yet Implemented

| Component | Notes |
|-----------|-------|
| RESP3 support | Only RESP2 implemented |
| Streaming arrays | Large arrays fully buffered |
| Inline commands | Only standard RESP |

## References

- [Redis Protocol Specification](https://redis.io/docs/reference/protocol-spec/)
- [bytes crate](https://docs.rs/bytes/latest/bytes/)
- [memchr crate](https://docs.rs/memchr/latest/memchr/)
- [tokio-util codec](https://docs.rs/tokio-util/latest/tokio_util/codec/)
