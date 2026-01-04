//! No-op observability stubs
//!
//! Zero-sized types that compile away completely when datadog feature is disabled.
//! All methods are marked `#[inline(always)]` to ensure zero overhead.

use std::time::Instant;

/// No-op metrics client - compiles to nothing
#[derive(Clone, Copy, Default)]
pub struct Metrics;

impl Metrics {
    #[inline(always)]
    pub fn new(_config: &DatadogConfig) -> Self {
        Metrics
    }

    #[inline(always)]
    pub fn incr(&self, _name: &str, _tags: &[&str]) {}

    #[inline(always)]
    pub fn histogram(&self, _name: &str, _value: f64, _tags: &[&str]) {}

    #[inline(always)]
    pub fn gauge(&self, _name: &str, _value: f64, _tags: &[&str]) {}

    #[inline(always)]
    pub fn timing(&self, _name: &str, _duration_ms: f64, _tags: &[&str]) {}

    #[inline(always)]
    pub fn record_command(&self, _command: &str, _duration_ms: f64, _success: bool) {}

    #[inline(always)]
    pub fn record_connection(&self, _event: &str) {}

    #[inline(always)]
    pub fn set_connections(&self, _count: usize) {}

    #[inline(always)]
    pub fn record_shard_operation(&self, _shard_id: usize, _duration_ms: f64) {}

    #[inline(always)]
    pub fn record_persistence_flush(&self, _bytes: usize, _deltas: usize, _duration_ms: f64) {}

    #[inline(always)]
    pub fn record_ttl_eviction(&self, _count: usize) {}

    #[inline(always)]
    pub fn timer(&self, _name: &'static str) -> Timer {
        Timer { _start: Instant::now() }
    }
}

/// No-op timer - records nothing on drop
pub struct Timer {
    _start: Instant,
}

impl Drop for Timer {
    #[inline(always)]
    fn drop(&mut self) {
        // No-op
    }
}

/// No-op configuration
#[derive(Clone, Default)]
pub struct DatadogConfig;

impl DatadogConfig {
    #[inline(always)]
    pub fn from_env() -> Self {
        DatadogConfig
    }
}

/// No-op tracing initialization - uses basic fmt subscriber
pub fn init_tracing(_config: &DatadogConfig) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .try_init()
        .map_err(|e| e.to_string().into())
}

/// No-op shutdown
#[inline(always)]
pub fn shutdown() {}

/// No-op span helpers
pub mod spans {
    use tracing::Span;

    #[inline(always)]
    pub fn connection_span(_client_addr: &str) -> Span {
        Span::none()
    }

    #[inline(always)]
    pub fn command_span(_command: &str, _key: Option<&str>) -> Span {
        Span::none()
    }

    #[inline(always)]
    pub fn shard_span(_shard_id: usize) -> Span {
        Span::none()
    }

    #[inline(always)]
    pub fn persistence_span(_operation: &str) -> Span {
        Span::none()
    }

    #[inline(always)]
    pub fn ttl_span() -> Span {
        Span::none()
    }
}
