pub mod connection;
pub mod crash;
pub mod dst;
pub mod dst_integration;
mod executor;
pub mod harness;
pub mod multi_node;
mod network;
pub mod partition_tests;
mod rng;
mod time;

pub use connection::{
    ExecutionRecord, PipelineResult, PipelineSimulator, SimulatedConnection, SimulatedReadBuffer,
    SimulatedWriteBuffer,
};
pub use crash::{CrashConfig, CrashReason, CrashSimulator, NodeSnapshot, NodeState};
pub use dst::{BatchResult, BatchRunner, DSTConfig, DSTSimulation, SimulationResult};
pub use executor::{Simulation, SimulationConfig};
pub use harness::{ScenarioBuilder, SimulatedRedisNode, SimulationHarness};
pub use multi_node::{
    check_single_key_linearizability, LinearizabilityResult, MultiNodeSimulation,
    TimestampedOperation,
};
pub use network::{Host, NetworkEvent, NetworkFault, PacketDelay};
pub use partition_tests::{
    run_partition_test, run_partition_test_batch, PartitionBatchResult, PartitionConfig,
    PartitionTestResult,
};
pub use rng::{buggify, DeterministicRng};
pub use time::{Duration, VirtualTime};

use std::cmp::Ordering;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HostId(pub usize);

#[derive(Debug, Clone)]
pub struct Event {
    pub time: VirtualTime,
    pub host_id: HostId,
    pub event_type: EventType,
}

impl PartialEq for Event {
    fn eq(&self, other: &Self) -> bool {
        self.time == other.time
    }
}

impl Eq for Event {}

impl PartialOrd for Event {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Event {
    fn cmp(&self, other: &Self) -> Ordering {
        other.time.cmp(&self.time)
    }
}

#[derive(Debug, Clone)]
pub enum EventType {
    Timer(TimerId),
    NetworkMessage(Message),
    HostStart,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TimerId(pub u64);

#[derive(Debug, Clone)]
pub struct Message {
    pub from: HostId,
    pub to: HostId,
    pub payload: Vec<u8>,
}
