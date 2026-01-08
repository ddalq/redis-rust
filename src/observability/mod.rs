//! Datadog Observability Module
//!
//! Provides full observability support for Redis Rust:
//! - Metrics via DogStatsD (UDP)
//! - Distributed tracing via Datadog APM
//! - Structured logging with trace correlation
//!
//! # Usage
//!
//! ```rust,ignore
//! use redis_sim::observability::{DatadogConfig, Metrics, init_tracing, shutdown};
//!
//! #[tokio::main]
//! async fn main() {
//!     let config = DatadogConfig::from_env();
//!     init_tracing(&config).expect("Failed to initialize tracing");
//!     let metrics = Metrics::new(&config);
//!
//!     // Use metrics throughout your application
//!     metrics.record_command("GET", 0.5, true);
//!
//!     // On shutdown
//!     shutdown();
//! }
//! ```
//!
//! # Environment Variables
//!
//! | Variable | Default | Description |
//! |----------|---------|-------------|
//! | `DD_SERVICE` | `redis-rust` | Service name |
//! | `DD_ENV` | `development` | Environment tag |
//! | `DD_VERSION` | pkg version | Service version |
//! | `DD_DOGSTATSD_URL` | `127.0.0.1:8125` | DogStatsD address |
//! | `DD_TRACE_AGENT_URL` | `http://127.0.0.1:8126` | APM agent URL |
//! | `DD_TRACE_SAMPLE_RATE` | `1.0` | Trace sampling rate |
//! | `DD_LOGS_INJECTION` | `false` | Enable JSON logs |
//! | `DD_METRIC_PREFIX` | `redis_rust` | Metric name prefix |
//! | `DD_TAGS` | `` | Global tags (k1:v1,k2:v2) |

pub mod config;
pub mod metrics;
pub mod recorder;
pub mod spans;
pub mod tracing_setup;

// Re-export commonly used types
pub use config::DatadogConfig;
pub use metrics::{Metrics, Timer};
pub use spans::*;
pub use tracing_setup::{init as init_tracing, shutdown};

// DST-compatible metrics abstractions
pub use recorder::{
    noop_metrics, simulated_metrics, MetricType, MetricsRecorder, NoopMetrics, RecordedMetric,
    SharedMetrics, SimulatedMetrics,
};
