pub mod buggify;
pub mod io;
pub mod metrics;
pub mod production;
pub mod redis;
pub mod replication;
pub mod simulator;
pub mod streaming;

// Observability: feature-gated Datadog integration
#[cfg(feature = "datadog")]
pub mod observability;

#[cfg(not(feature = "datadog"))]
#[path = "observability_noop.rs"]
pub mod observability;

pub use redis::{RedisClient, RedisServer, RespParser, Value};
pub use simulator::{Host, NetworkEvent, Simulation, SimulationConfig};
