//! Metrics Recorder Trait for DST Compatibility
//!
//! Defines a trait abstraction for metrics recording that supports:
//! - Production: Real DogStatsD client
//! - Simulation: In-memory recording for testing
//!
//! Following TigerStyle principles: all I/O through trait abstractions.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use parking_lot::Mutex;

/// Trait for recording metrics - enables DST-compatible observability
pub trait MetricsRecorder: Send + Sync + 'static {
    /// Increment a counter by 1
    fn incr(&self, name: &str, tags: &[&str]);

    /// Record a histogram/distribution value
    fn histogram(&self, name: &str, value: f64, tags: &[&str]);

    /// Set a gauge value
    fn gauge(&self, name: &str, value: f64, tags: &[&str]);

    /// Record a timing in milliseconds
    fn timing(&self, name: &str, duration_ms: f64, tags: &[&str]);

    // Convenience methods with default implementations

    /// Record a command execution with timing and status
    fn record_command(&self, command: &str, duration_ms: f64, success: bool) {
        let status = if success { "success" } else { "error" };
        let cmd_tag = format!("command:{}", command.to_lowercase());
        let status_tag = format!("status:{}", status);

        self.histogram("command.duration", duration_ms, &[&cmd_tag, &status_tag]);
        self.incr("command.count", &[&cmd_tag, &status_tag]);
    }

    /// Record a connection event
    fn record_connection(&self, event: &str) {
        let event_tag = format!("event:{}", event);
        self.incr("connection.events", &[&event_tag]);
    }

    /// Update active connections gauge
    fn set_connections(&self, count: usize) {
        self.gauge("connections.active", count as f64, &[]);
    }

    /// Record TTL eviction count
    fn record_ttl_eviction(&self, count: usize) {
        if count > 0 {
            self.incr("ttl.evictions", &[]);
            self.histogram("ttl.evictions.batch_size", count as f64, &[]);
        }
    }
}

/// No-op metrics recorder - zero overhead when metrics are disabled
#[derive(Clone, Default)]
pub struct NoopMetrics;

impl MetricsRecorder for NoopMetrics {
    #[inline]
    fn incr(&self, _name: &str, _tags: &[&str]) {}
    #[inline]
    fn histogram(&self, _name: &str, _value: f64, _tags: &[&str]) {}
    #[inline]
    fn gauge(&self, _name: &str, _value: f64, _tags: &[&str]) {}
    #[inline]
    fn timing(&self, _name: &str, _duration_ms: f64, _tags: &[&str]) {}
}

/// Recorded metric for testing/simulation
#[derive(Debug, Clone)]
pub struct RecordedMetric {
    pub name: String,
    pub value: f64,
    pub tags: Vec<String>,
    pub metric_type: MetricType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricType {
    Counter,
    Histogram,
    Gauge,
    Timing,
}

/// Simulated metrics recorder for DST - records all metrics for verification
#[derive(Default)]
pub struct SimulatedMetrics {
    recorded: Mutex<Vec<RecordedMetric>>,
    command_count: AtomicU64,
    connection_count: AtomicU64,
    eviction_count: AtomicU64,
}

impl SimulatedMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get all recorded metrics
    pub fn get_recorded(&self) -> Vec<RecordedMetric> {
        self.recorded.lock().clone()
    }

    /// Get metrics by name
    pub fn get_by_name(&self, name: &str) -> Vec<RecordedMetric> {
        self.recorded
            .lock()
            .iter()
            .filter(|m| m.name == name)
            .cloned()
            .collect()
    }

    /// Get total command count
    pub fn command_count(&self) -> u64 {
        self.command_count.load(Ordering::SeqCst)
    }

    /// Get total connection events
    pub fn connection_count(&self) -> u64 {
        self.connection_count.load(Ordering::SeqCst)
    }

    /// Get total eviction count
    pub fn eviction_count(&self) -> u64 {
        self.eviction_count.load(Ordering::SeqCst)
    }

    /// Clear all recorded metrics
    pub fn clear(&self) {
        self.recorded.lock().clear();
        self.command_count.store(0, Ordering::SeqCst);
        self.connection_count.store(0, Ordering::SeqCst);
        self.eviction_count.store(0, Ordering::SeqCst);
    }

    /// Assert a metric was recorded with specific value
    pub fn assert_metric(&self, name: &str, metric_type: MetricType) -> bool {
        self.recorded
            .lock()
            .iter()
            .any(|m| m.name == name && m.metric_type == metric_type)
    }
}

impl MetricsRecorder for SimulatedMetrics {
    fn incr(&self, name: &str, tags: &[&str]) {
        self.recorded.lock().push(RecordedMetric {
            name: name.to_string(),
            value: 1.0,
            tags: tags.iter().map(|s| s.to_string()).collect(),
            metric_type: MetricType::Counter,
        });
    }

    fn histogram(&self, name: &str, value: f64, tags: &[&str]) {
        self.recorded.lock().push(RecordedMetric {
            name: name.to_string(),
            value,
            tags: tags.iter().map(|s| s.to_string()).collect(),
            metric_type: MetricType::Histogram,
        });
    }

    fn gauge(&self, name: &str, value: f64, tags: &[&str]) {
        self.recorded.lock().push(RecordedMetric {
            name: name.to_string(),
            value,
            tags: tags.iter().map(|s| s.to_string()).collect(),
            metric_type: MetricType::Gauge,
        });
    }

    fn timing(&self, name: &str, duration_ms: f64, tags: &[&str]) {
        self.recorded.lock().push(RecordedMetric {
            name: name.to_string(),
            value: duration_ms,
            tags: tags.iter().map(|s| s.to_string()).collect(),
            metric_type: MetricType::Timing,
        });
    }

    fn record_command(&self, command: &str, duration_ms: f64, success: bool) {
        self.command_count.fetch_add(1, Ordering::SeqCst);

        let status = if success { "success" } else { "error" };
        let cmd_tag = format!("command:{}", command.to_lowercase());
        let status_tag = format!("status:{}", status);

        self.histogram("command.duration", duration_ms, &[&cmd_tag, &status_tag]);
        self.incr("command.count", &[&cmd_tag, &status_tag]);
    }

    fn record_connection(&self, event: &str) {
        self.connection_count.fetch_add(1, Ordering::SeqCst);
        let event_tag = format!("event:{}", event);
        self.incr("connection.events", &[&event_tag]);
    }

    fn record_ttl_eviction(&self, count: usize) {
        if count > 0 {
            self.eviction_count.fetch_add(count as u64, Ordering::SeqCst);
            self.incr("ttl.evictions", &[]);
            self.histogram("ttl.evictions.batch_size", count as f64, &[]);
        }
    }
}

/// Arc wrapper for trait object usage
pub type SharedMetrics = Arc<dyn MetricsRecorder>;

/// Create a no-op metrics recorder
pub fn noop_metrics() -> SharedMetrics {
    Arc::new(NoopMetrics)
}

/// Create a simulated metrics recorder for testing
pub fn simulated_metrics() -> Arc<SimulatedMetrics> {
    Arc::new(SimulatedMetrics::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simulated_metrics_records() {
        let metrics = SimulatedMetrics::new();

        metrics.incr("test.counter", &["tag:value"]);
        metrics.histogram("test.histogram", 42.0, &[]);
        metrics.gauge("test.gauge", 100.0, &[]);
        metrics.timing("test.timing", 5.5, &[]);

        let recorded = metrics.get_recorded();
        assert_eq!(recorded.len(), 4);
        assert!(metrics.assert_metric("test.counter", MetricType::Counter));
        assert!(metrics.assert_metric("test.histogram", MetricType::Histogram));
        assert!(metrics.assert_metric("test.gauge", MetricType::Gauge));
        assert!(metrics.assert_metric("test.timing", MetricType::Timing));
    }

    #[test]
    fn test_simulated_metrics_command_tracking() {
        let metrics = SimulatedMetrics::new();

        metrics.record_command("GET", 1.0, true);
        metrics.record_command("SET", 2.0, true);
        metrics.record_command("GET", 0.5, false);

        assert_eq!(metrics.command_count(), 3);

        let durations = metrics.get_by_name("command.duration");
        assert_eq!(durations.len(), 3);
    }

    #[test]
    fn test_simulated_metrics_connection_tracking() {
        let metrics = SimulatedMetrics::new();

        metrics.record_connection("established");
        metrics.record_connection("closed");

        assert_eq!(metrics.connection_count(), 2);
    }

    #[test]
    fn test_simulated_metrics_eviction_tracking() {
        let metrics = SimulatedMetrics::new();

        metrics.record_ttl_eviction(10);
        metrics.record_ttl_eviction(5);
        metrics.record_ttl_eviction(0); // Should not count

        assert_eq!(metrics.eviction_count(), 15);
    }

    #[test]
    fn test_noop_metrics_no_panic() {
        let metrics = NoopMetrics;

        // All operations should be no-ops without panicking
        metrics.incr("test", &[]);
        metrics.histogram("test", 1.0, &[]);
        metrics.gauge("test", 1.0, &[]);
        metrics.timing("test", 1.0, &[]);
        metrics.record_command("GET", 1.0, true);
        metrics.record_connection("test");
        metrics.record_ttl_eviction(10);
    }

    #[test]
    fn test_clear_metrics() {
        let metrics = SimulatedMetrics::new();

        metrics.record_command("GET", 1.0, true);
        assert_eq!(metrics.command_count(), 1);

        metrics.clear();
        assert_eq!(metrics.command_count(), 0);
        assert!(metrics.get_recorded().is_empty());
    }
}
