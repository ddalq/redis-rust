//! Telemetry-Style Metrics Aggregation Module
//!
//! This module provides a metrics aggregation service that showcases
//! the unique features of this redis-rust implementation:
//!
//! - **CRDT counters** for coordination-free distributed counting
//! - **Hot key detection** for popular dashboard metrics
//! - **Pipelining** for high-throughput batch ingestion
//! - **Eventual consistency** for multi-node metric aggregation

mod commands;
mod key_encoder;
mod query;
mod state;
mod types;

pub use commands::{MetricsCommand, MetricsCommandExecutor, MetricsResult};
pub use key_encoder::MetricKeyEncoder;
pub use query::{AggregationType, MetricsQuery, QueryExecutor, QueryResult};
pub use state::{MetricsDelta, MetricsState};
pub use types::{MetricPoint, MetricType, MetricValue, TagSet};
