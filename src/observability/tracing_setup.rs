//! Tracing and APM Setup
//!
//! Initializes tracing-subscriber with OpenTelemetry for Datadog APM.

use opentelemetry_datadog::DatadogPropagator;
use opentelemetry_sdk::trace::Sampler;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use super::config::DatadogConfig;

/// Initialize the complete observability stack
///
/// Sets up:
/// - OpenTelemetry with Datadog exporter for distributed tracing
/// - tracing-subscriber with environment-based filtering
pub fn init(config: &DatadogConfig) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Set global propagator for distributed tracing context
    opentelemetry::global::set_text_map_propagator(DatadogPropagator::default());

    // Create Datadog exporter pipeline - returns a Tracer directly
    let tracer = opentelemetry_datadog::new_pipeline()
        .with_service_name(&config.service_name)
        .with_agent_endpoint(&config.trace_addr)
        .with_trace_config(
            opentelemetry_sdk::trace::Config::default()
                .with_sampler(Sampler::TraceIdRatioBased(config.trace_sample_rate))
                .with_resource(opentelemetry_sdk::Resource::new(vec![
                    opentelemetry::KeyValue::new("service.name", config.service_name.clone()),
                    opentelemetry::KeyValue::new("service.version", config.version.clone()),
                    opentelemetry::KeyValue::new("deployment.environment", config.env.clone()),
                ])),
        )
        .install_batch(opentelemetry_sdk::runtime::Tokio)?;

    // Create OpenTelemetry tracing layer
    let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);

    // Environment filter for log levels
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    // Build subscriber with all layers
    // Note: fmt layer goes before otel layer so spans are logged before being exported
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .with(otel_layer)
        .init();

    tracing::info!(
        service = %config.service_name,
        env = %config.env,
        version = %config.version,
        sample_rate = %config.trace_sample_rate,
        "Datadog observability initialized"
    );

    Ok(())
}

/// Shutdown tracing gracefully
///
/// Flushes any pending spans to the Datadog agent.
/// Should be called before application exit.
pub fn shutdown() {
    tracing::info!("Shutting down Datadog tracing...");
    opentelemetry::global::shutdown_tracer_provider();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_creation() {
        // Just verify config can be created without panicking
        let _config = DatadogConfig::from_env();
    }
}
