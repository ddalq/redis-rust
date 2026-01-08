//! DogStatsD Metrics Client
//!
//! Thread-safe, non-blocking UDP metrics client for Datadog.
//! Gracefully degrades if the Datadog agent is unavailable.
//!
//! Implements MetricsRecorder trait for DST compatibility.

use dogstatsd::{Client, Options};
use std::sync::Arc;
use std::time::Instant;

use super::config::DatadogConfig;
use super::recorder::MetricsRecorder;

/// Metrics client wrapper with graceful degradation
#[derive(Clone)]
pub struct Metrics {
    client: Arc<Option<Client>>,
    prefix: String,
    global_tags: Vec<String>,
}

impl Metrics {
    /// Create a new metrics client from configuration
    pub fn new(config: &DatadogConfig) -> Self {
        let client = match Client::new(Options {
            to_addr: config.statsd_addr.to_string(),
            ..Default::default()
        }) {
            Ok(c) => {
                tracing::info!("DogStatsD client connected to {}", config.statsd_addr);
                Some(c)
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to create DogStatsD client: {}. Metrics disabled.",
                    e
                );
                None
            }
        };

        Metrics {
            client: Arc::new(client),
            prefix: config.metric_prefix.clone(),
            global_tags: config.formatted_tags(),
        }
    }

    /// Increment a counter by 1
    #[inline]
    pub fn incr(&self, name: &str, tags: &[&str]) {
        if let Some(ref client) = *self.client {
            let metric_name = format!("{}.{}", self.prefix, name);
            let all_tags = self.merge_tags(tags);
            let _ = client.incr(&metric_name, all_tags);
        }
    }

    /// Record a histogram/distribution value
    #[inline]
    pub fn histogram(&self, name: &str, value: f64, tags: &[&str]) {
        if let Some(ref client) = *self.client {
            let metric_name = format!("{}.{}", self.prefix, name);
            let all_tags = self.merge_tags(tags);
            // dogstatsd uses i64 for histogram, so we convert
            let _ = client.histogram(&metric_name, value.to_string(), all_tags);
        }
    }

    /// Set a gauge value
    #[inline]
    pub fn gauge(&self, name: &str, value: f64, tags: &[&str]) {
        if let Some(ref client) = *self.client {
            let metric_name = format!("{}.{}", self.prefix, name);
            let all_tags = self.merge_tags(tags);
            let _ = client.gauge(&metric_name, value.to_string(), all_tags);
        }
    }

    /// Record a timing in milliseconds
    #[inline]
    pub fn timing(&self, name: &str, duration_ms: f64, tags: &[&str]) {
        if let Some(ref client) = *self.client {
            let metric_name = format!("{}.{}", self.prefix, name);
            let all_tags = self.merge_tags(tags);
            let _ = client.timing(&metric_name, duration_ms as i64, all_tags);
        }
    }

    /// Create a timer that records duration on drop
    #[inline]
    pub fn timer(&self, name: &'static str) -> Timer {
        Timer {
            metrics: self.clone(),
            name,
            tags: Vec::new(),
            start: Instant::now(),
        }
    }

    /// Create a timer with tags
    #[inline]
    pub fn timer_with_tags(&self, name: &'static str, tags: Vec<String>) -> Timer {
        Timer {
            metrics: self.clone(),
            name,
            tags,
            start: Instant::now(),
        }
    }

    fn merge_tags(&self, tags: &[&str]) -> Vec<String> {
        self.global_tags
            .iter()
            .cloned()
            .chain(tags.iter().map(|s| s.to_string()))
            .collect()
    }
}

/// RAII timer that records duration when dropped
pub struct Timer {
    metrics: Metrics,
    name: &'static str,
    tags: Vec<String>,
    start: Instant,
}

impl Timer {
    /// Add a tag to the timer
    pub fn with_tag(mut self, tag: String) -> Self {
        self.tags.push(tag);
        self
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        let duration_ms = self.start.elapsed().as_secs_f64() * 1000.0;
        let tag_refs: Vec<&str> = self.tags.iter().map(|s| s.as_str()).collect();
        self.metrics.timing(self.name, duration_ms, &tag_refs);
    }
}

// Convenience methods for common Redis metrics
impl Metrics {
    /// Record a command execution with timing and status
    #[inline]
    pub fn record_command(&self, command: &str, duration_ms: f64, success: bool) {
        let status = if success { "success" } else { "error" };
        let cmd_tag = format!("command:{}", command.to_lowercase());
        let status_tag = format!("status:{}", status);

        self.histogram("command.duration", duration_ms, &[&cmd_tag, &status_tag]);
        self.incr("command.count", &[&cmd_tag, &status_tag]);
    }

    /// Record a connection event (established, closed, error)
    #[inline]
    pub fn record_connection(&self, event: &str) {
        let event_tag = format!("event:{}", event);
        self.incr("connection.events", &[&event_tag]);
    }

    /// Update the active connections gauge
    #[inline]
    pub fn set_connections(&self, count: usize) {
        self.gauge("connections.active", count as f64, &[]);
    }

    /// Record a shard operation duration
    #[inline]
    pub fn record_shard_operation(&self, shard_id: usize, duration_ms: f64) {
        let shard_tag = format!("shard_id:{}", shard_id);
        self.histogram("shard.operation.duration", duration_ms, &[&shard_tag]);
    }

    /// Record a persistence flush operation
    #[inline]
    pub fn record_persistence_flush(&self, bytes: usize, deltas: usize, duration_ms: f64) {
        self.histogram("persistence.flush.duration", duration_ms, &[]);
        self.histogram("persistence.flush.bytes", bytes as f64, &[]);
        self.histogram("persistence.flush.deltas", deltas as f64, &[]);
        self.incr("persistence.flush.count", &[]);
    }

    /// Record TTL eviction count
    #[inline]
    pub fn record_ttl_eviction(&self, count: usize) {
        if count > 0 {
            self.incr("ttl.evictions", &[]);
            self.histogram("ttl.evictions.batch_size", count as f64, &[]);
        }
    }
}

// Implement MetricsRecorder trait for DST compatibility
impl MetricsRecorder for Metrics {
    #[inline]
    fn incr(&self, name: &str, tags: &[&str]) {
        Metrics::incr(self, name, tags)
    }

    #[inline]
    fn histogram(&self, name: &str, value: f64, tags: &[&str]) {
        Metrics::histogram(self, name, value, tags)
    }

    #[inline]
    fn gauge(&self, name: &str, value: f64, tags: &[&str]) {
        Metrics::gauge(self, name, value, tags)
    }

    #[inline]
    fn timing(&self, name: &str, duration_ms: f64, tags: &[&str]) {
        Metrics::timing(self, name, duration_ms, tags)
    }

    fn record_command(&self, command: &str, duration_ms: f64, success: bool) {
        Metrics::record_command(self, command, duration_ms, success)
    }

    fn record_connection(&self, event: &str) {
        Metrics::record_connection(self, event)
    }

    fn set_connections(&self, count: usize) {
        Metrics::set_connections(self, count)
    }

    fn record_ttl_eviction(&self, count: usize) {
        Metrics::record_ttl_eviction(self, count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_graceful_degradation() {
        // Create metrics with invalid address - should not panic
        let config = DatadogConfig {
            statsd_addr: "127.0.0.1:0".parse().unwrap(),
            ..DatadogConfig::from_env()
        };
        let metrics = Metrics::new(&config);

        // All operations should be no-ops without panicking
        metrics.incr("test.counter", &[]);
        metrics.gauge("test.gauge", 42.0, &[]);
        metrics.histogram("test.histogram", 1.5, &[]);
        metrics.record_command("GET", 0.5, true);
    }
}
