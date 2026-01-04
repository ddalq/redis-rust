pub mod io;
pub mod buggify;
pub mod simulator;
pub mod redis;
pub mod production;
pub mod replication;
pub mod metrics;
pub mod streaming;

// Observability: feature-gated Datadog integration
#[cfg(feature = "datadog")]
pub mod observability;

#[cfg(not(feature = "datadog"))]
#[path = "observability_noop.rs"]
pub mod observability;

pub use simulator::{Simulation, SimulationConfig, Host, NetworkEvent};
pub use redis::{RedisServer, RedisClient, Value, RespParser};
