//! Span Helpers for Request Tracing
//!
//! Provides structured spans for the request lifecycle with proper
//! OpenTelemetry semantic conventions.

use tracing::{span, Level, Span};

/// Create a span for connection handling
#[inline]
pub fn connection_span(client_addr: &str) -> Span {
    span!(
        Level::INFO,
        "redis.connection",
        client.address = %client_addr,
        otel.kind = "server"
    )
}

/// Create a span for command execution
#[inline]
pub fn command_span(command: &str, key: Option<&str>) -> Span {
    match key {
        Some(k) => span!(
            Level::INFO,
            "redis.command",
            db.operation = %command,
            db.redis.key = %k,
            otel.kind = "internal"
        ),
        None => span!(
            Level::INFO,
            "redis.command",
            db.operation = %command,
            otel.kind = "internal"
        ),
    }
}

/// Create a span for shard execution
#[inline]
pub fn shard_span(shard_id: usize) -> Span {
    span!(
        Level::DEBUG,
        "redis.shard",
        shard.id = shard_id,
        otel.kind = "internal"
    )
}

/// Create a span for persistence operations
#[inline]
pub fn persistence_span(operation: &str) -> Span {
    span!(
        Level::INFO,
        "redis.persistence",
        persistence.operation = %operation,
        otel.kind = "internal"
    )
}

/// Create a span for TTL eviction
#[inline]
pub fn ttl_span() -> Span {
    span!(Level::DEBUG, "redis.ttl_eviction", otel.kind = "internal")
}

/// Create a span for network I/O
#[inline]
pub fn network_span(operation: &str, bytes: usize) -> Span {
    span!(
        Level::TRACE,
        "redis.network",
        net.operation = %operation,
        net.bytes = bytes,
        otel.kind = "internal"
    )
}
