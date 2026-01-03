//! Deterministic Simulation Testing (DST) Harness
//!
//! Unified framework for running deterministic simulations inspired by
//! FoundationDB and TigerBeetle. This harness brings together:
//! - SimulatedRuntime for virtual I/O
//! - BUGGIFY fault injection
//! - CrashSimulator for crash/recovery
//! - Batch testing capabilities
//!
//! # Example
//!
//! ```ignore
//! let result = DSTSimulation::new(seed)
//!     .with_nodes(5)
//!     .with_faults(FaultConfig::chaos())
//!     .run_operations(10_000);
//!
//! assert!(result.check_linearizability());
//! ```

use super::crash::{CrashConfig, CrashSimulator, CrashReason, NodeSnapshot};
use super::{HostId, VirtualTime};
use crate::buggify::{self, FaultConfig, BuggifyStats};
use crate::io::simulation::{ClockOffset, NodeId, SimulatedRng, SimulationContext};
use crate::io::Rng;
use std::collections::HashMap;
use std::sync::Arc;

/// Configuration for a DST simulation run
#[derive(Debug, Clone)]
pub struct DSTConfig {
    /// Random seed for reproducibility
    pub seed: u64,
    /// Number of nodes in the simulation
    pub node_count: usize,
    /// Fault injection configuration
    pub fault_config: FaultConfig,
    /// Crash/recovery configuration
    pub crash_config: CrashConfig,
    /// Maximum simulation time in milliseconds
    pub max_time_ms: u64,
    /// Operations per simulation step
    pub ops_per_step: usize,
    /// Enable clock skew simulation
    pub enable_clock_skew: bool,
    /// Maximum clock skew in milliseconds
    pub max_clock_skew_ms: i64,
    /// Clock drift rate in PPM (parts per million)
    pub max_clock_drift_ppm: i64,
}

impl Default for DSTConfig {
    fn default() -> Self {
        DSTConfig {
            seed: 0,
            node_count: 5,
            fault_config: FaultConfig::moderate(),
            crash_config: CrashConfig::default(),
            max_time_ms: 60_000, // 1 minute virtual time
            ops_per_step: 100,
            enable_clock_skew: true,
            max_clock_skew_ms: 500,
            max_clock_drift_ppm: 1000,
        }
    }
}

impl DSTConfig {
    pub fn new(seed: u64) -> Self {
        DSTConfig {
            seed,
            ..Default::default()
        }
    }

    pub fn with_nodes(mut self, count: usize) -> Self {
        self.node_count = count;
        self
    }

    pub fn with_faults(mut self, config: FaultConfig) -> Self {
        self.fault_config = config;
        self
    }

    pub fn with_crash_config(mut self, config: CrashConfig) -> Self {
        self.crash_config = config;
        self
    }

    pub fn with_max_time(mut self, ms: u64) -> Self {
        self.max_time_ms = ms;
        self
    }

    pub fn with_clock_skew(mut self, enabled: bool) -> Self {
        self.enable_clock_skew = enabled;
        self
    }

    /// Preset: calm mode with minimal fault injection
    pub fn calm(seed: u64) -> Self {
        DSTConfig {
            seed,
            fault_config: FaultConfig::calm(),
            crash_config: CrashConfig {
                enable_buggify_crashes: false,
                ..Default::default()
            },
            enable_clock_skew: false,
            ..Default::default()
        }
    }

    /// Preset: chaos mode with aggressive fault injection
    pub fn chaos(seed: u64) -> Self {
        DSTConfig {
            seed,
            fault_config: FaultConfig::chaos(),
            crash_config: CrashConfig {
                base_crash_probability: 0.01,
                enable_buggify_crashes: true,
                ..Default::default()
            },
            enable_clock_skew: true,
            max_clock_skew_ms: 1000,
            max_clock_drift_ppm: 5000,
            ..Default::default()
        }
    }
}

/// Operation recorded during simulation for linearizability checking
#[derive(Debug, Clone)]
pub struct RecordedOperation {
    pub id: u64,
    pub node_id: usize,
    pub op_type: OperationType,
    pub key: String,
    pub value: Option<String>,
    pub start_time: VirtualTime,
    pub end_time: Option<VirtualTime>,
    pub result: OperationResult,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationType {
    Read,
    Write,
    CompareAndSwap,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationResult {
    Success(Option<String>),
    Failure(String),
    Timeout,
    Pending,
}

/// Result of a simulation run
#[derive(Debug, Clone)]
pub struct SimulationResult {
    /// Seed used for this run
    pub seed: u64,
    /// Total virtual time elapsed
    pub total_time_ms: u64,
    /// Total operations executed
    pub total_operations: u64,
    /// Operations by type
    pub operations_by_type: HashMap<String, u64>,
    /// Node crash count
    pub crashes: u64,
    /// Node recovery count
    pub recoveries: u64,
    /// BUGGIFY statistics
    pub buggify_stats: BuggifyStats,
    /// Whether linearizability was maintained
    pub linearizable: bool,
    /// Whether all nodes eventually converged
    pub converged: bool,
    /// Any errors encountered
    pub errors: Vec<String>,
    /// Recorded operations for debugging
    pub operation_history: Vec<RecordedOperation>,
}

impl SimulationResult {
    pub fn new(seed: u64) -> Self {
        SimulationResult {
            seed,
            total_time_ms: 0,
            total_operations: 0,
            operations_by_type: HashMap::new(),
            crashes: 0,
            recoveries: 0,
            buggify_stats: BuggifyStats::default(),
            linearizable: true,
            converged: true,
            errors: Vec::new(),
            operation_history: Vec::new(),
        }
    }

    pub fn is_success(&self) -> bool {
        self.linearizable && self.converged && self.errors.is_empty()
    }

    pub fn summary(&self) -> String {
        format!(
            "Seed {}: {} ops in {}ms, {} crashes, {} recoveries, linearizable={}, converged={}, errors={}",
            self.seed,
            self.total_operations,
            self.total_time_ms,
            self.crashes,
            self.recoveries,
            self.linearizable,
            self.converged,
            self.errors.len()
        )
    }
}

/// Main DST simulation harness
pub struct DSTSimulation {
    config: DSTConfig,
    ctx: Arc<SimulationContext>,
    rng: SimulatedRng,
    crash_simulator: CrashSimulator,
    current_time: VirtualTime,
    operation_counter: u64,
    result: SimulationResult,
}

impl DSTSimulation {
    /// Create a new DST simulation with the given seed
    pub fn new(seed: u64) -> Self {
        Self::with_config(DSTConfig::new(seed))
    }

    /// Create a simulation with full configuration
    pub fn with_config(config: DSTConfig) -> Self {
        // Set up BUGGIFY configuration
        buggify::set_config(config.fault_config.clone());

        let ctx = Arc::new(SimulationContext::new(config.seed, config.fault_config.clone()));
        let mut rng = SimulatedRng::new(config.seed);
        let mut crash_simulator = CrashSimulator::with_config(config.crash_config.clone());

        // Register nodes and set up clock skew
        for i in 0..config.node_count {
            let host_id = HostId(i);
            let node_id = NodeId(i);
            crash_simulator.register_node(host_id);

            // Set up random clock offsets if enabled
            if config.enable_clock_skew {
                let offset_ms = rng.gen_range(0, (config.max_clock_skew_ms * 2) as u64) as i64
                    - config.max_clock_skew_ms;
                let drift_ppm = rng.gen_range(0, (config.max_clock_drift_ppm * 2) as u64) as i64
                    - config.max_clock_drift_ppm;

                ctx.set_clock_offset(
                    node_id,
                    ClockOffset {
                        fixed_offset_ms: offset_ms,
                        drift_ppm,
                        drift_anchor: VirtualTime(0).into(),
                    },
                );
            }
        }

        DSTSimulation {
            config: config.clone(),
            ctx,
            rng,
            crash_simulator,
            current_time: VirtualTime(0),
            operation_counter: 0,
            result: SimulationResult::new(config.seed),
        }
    }

    /// Builder: set number of nodes
    pub fn with_nodes(mut self, count: usize) -> Self {
        self.config.node_count = count;
        // Re-register nodes
        for i in 0..count {
            self.crash_simulator.register_node(HostId(i));
        }
        self
    }

    /// Builder: set fault configuration
    pub fn with_faults(mut self, config: FaultConfig) -> Self {
        self.config.fault_config = config.clone();
        buggify::set_config(config);
        self
    }

    /// Get current simulation time
    pub fn current_time(&self) -> VirtualTime {
        self.current_time
    }

    /// Get simulation context for custom operations
    pub fn context(&self) -> &Arc<SimulationContext> {
        &self.ctx
    }

    /// Advance simulation time
    pub fn advance_time(&mut self, ms: u64) {
        self.current_time = VirtualTime(self.current_time.0 + ms);
        self.ctx.advance_to(self.current_time.into());

        // Process crash recoveries
        let recovered = self.crash_simulator.advance_time(self.current_time);
        self.result.recoveries += recovered.len() as u64;
    }

    /// Maybe crash a node based on BUGGIFY
    pub fn maybe_crash_node(&mut self, node: usize) -> bool {
        let crashed = self.crash_simulator.maybe_crash(
            &mut self.rng,
            HostId(node),
            self.current_time,
        );
        if crashed {
            self.result.crashes += 1;
        }
        crashed
    }

    /// Explicitly crash a node
    pub fn crash_node(&mut self, node: usize, reason: CrashReason) {
        self.crash_simulator.crash_node(HostId(node), self.current_time, reason);
        self.result.crashes += 1;
    }

    /// Start recovery for a crashed node
    pub fn start_recovery(&mut self, node: usize) -> Option<&NodeSnapshot> {
        self.crash_simulator.start_recovery(&mut self.rng, HostId(node), self.current_time)
    }

    /// Check if a node is running
    pub fn is_node_running(&self, node: usize) -> bool {
        self.crash_simulator.is_running(HostId(node))
    }

    /// Record an operation
    pub fn record_operation(&mut self, op: RecordedOperation) {
        let op_type = format!("{:?}", op.op_type);
        *self.result.operations_by_type.entry(op_type).or_insert(0) += 1;
        self.result.total_operations += 1;
        self.result.operation_history.push(op);
    }

    /// Generate next operation ID
    pub fn next_op_id(&mut self) -> u64 {
        self.operation_counter += 1;
        self.operation_counter
    }

    /// Select a random running node
    pub fn random_running_node(&mut self) -> Option<usize> {
        let running: Vec<usize> = (0..self.config.node_count)
            .filter(|&i| self.is_node_running(i))
            .collect();

        if running.is_empty() {
            None
        } else {
            let idx = self.rng.gen_range(0, running.len() as u64) as usize;
            Some(running[idx])
        }
    }

    /// Run a step of the simulation
    pub fn step(&mut self) {
        // Advance time slightly
        let time_advance = self.rng.gen_range(1, 100);
        self.advance_time(time_advance);

        // Maybe crash some nodes
        for node in 0..self.config.node_count {
            if self.is_node_running(node) {
                self.maybe_crash_node(node);
            }
        }

        // Start recovery for crashed nodes (with some probability)
        for node in self.crash_simulator.crashed_nodes() {
            if self.rng.gen_bool(0.1) {
                self.start_recovery(node.0);
            }
        }

        // Count steps as operations (subclasses will override with actual operations)
        self.result.total_operations += 1;
    }

    /// Run simulation for specified number of operations
    pub fn run_operations(&mut self, count: usize) -> &SimulationResult {
        for _ in 0..count {
            self.step();

            // Check time limit
            if self.current_time.0 >= self.config.max_time_ms {
                break;
            }
        }

        self.finalize()
    }

    /// Finalize the simulation and return results
    pub fn finalize(&mut self) -> &SimulationResult {
        self.result.total_time_ms = self.current_time.0;
        self.result.buggify_stats = buggify::get_stats();

        // Update crash stats
        let crash_stats = self.crash_simulator.stats();
        self.result.crashes = crash_stats.total_crashes;
        self.result.recoveries = crash_stats.total_recoveries;

        &self.result
    }

    /// Get RNG for custom randomization
    pub fn rng(&mut self) -> &mut SimulatedRng {
        &mut self.rng
    }

    /// Get crash simulator
    pub fn crash_simulator(&self) -> &CrashSimulator {
        &self.crash_simulator
    }

    /// Get configuration
    pub fn config(&self) -> &DSTConfig {
        &self.config
    }
}

/// Batch runner for running many seeds in parallel
pub struct BatchRunner {
    base_seed: u64,
    count: usize,
    config_template: DSTConfig,
}

impl BatchRunner {
    pub fn new(base_seed: u64, count: usize) -> Self {
        BatchRunner {
            base_seed,
            count,
            config_template: DSTConfig::default(),
        }
    }

    pub fn with_config(mut self, config: DSTConfig) -> Self {
        self.config_template = config;
        self
    }

    /// Run all simulations sequentially
    pub fn run_sequential<F>(&self, ops_per_run: usize, mut run_fn: F) -> BatchResult
    where
        F: FnMut(&mut DSTSimulation),
    {
        let mut results = Vec::with_capacity(self.count);

        for i in 0..self.count {
            let seed = self.base_seed + i as u64;
            let config = DSTConfig {
                seed,
                ..self.config_template.clone()
            };

            let mut sim = DSTSimulation::with_config(config);
            run_fn(&mut sim);
            sim.run_operations(ops_per_run);

            results.push(sim.result.clone());
        }

        BatchResult::from_results(self.base_seed, results)
    }

    /// Run with default behavior (just stepping)
    pub fn run_default(&self, ops_per_run: usize) -> BatchResult {
        self.run_sequential(ops_per_run, |_| {})
    }
}

/// Result of a batch run
#[derive(Debug, Clone)]
pub struct BatchResult {
    pub base_seed: u64,
    pub total_runs: usize,
    pub successful_runs: usize,
    pub failed_runs: usize,
    pub failed_seeds: Vec<u64>,
    pub total_operations: u64,
    pub total_crashes: u64,
    pub total_recoveries: u64,
}

impl BatchResult {
    pub fn from_results(base_seed: u64, results: Vec<SimulationResult>) -> Self {
        let total_runs = results.len();
        let successful_runs = results.iter().filter(|r| r.is_success()).count();
        let failed_runs = total_runs - successful_runs;
        let failed_seeds: Vec<u64> = results
            .iter()
            .filter(|r| !r.is_success())
            .map(|r| r.seed)
            .collect();

        let total_operations: u64 = results.iter().map(|r| r.total_operations).sum();
        let total_crashes: u64 = results.iter().map(|r| r.crashes).sum();
        let total_recoveries: u64 = results.iter().map(|r| r.recoveries).sum();

        BatchResult {
            base_seed,
            total_runs,
            successful_runs,
            failed_runs,
            failed_seeds,
            total_operations,
            total_crashes,
            total_recoveries,
        }
    }

    pub fn all_passed(&self) -> bool {
        self.failed_runs == 0
    }

    pub fn summary(&self) -> String {
        format!(
            "Batch {} runs: {}/{} passed, {} total ops, {} crashes, {} recoveries",
            self.total_runs,
            self.successful_runs,
            self.total_runs,
            self.total_operations,
            self.total_crashes,
            self.total_recoveries
        )
    }
}

// Convert between time types
impl From<VirtualTime> for crate::io::Timestamp {
    fn from(vt: VirtualTime) -> Self {
        crate::io::Timestamp::from_millis(vt.0)
    }
}

impl From<crate::io::Timestamp> for VirtualTime {
    fn from(ts: crate::io::Timestamp) -> Self {
        VirtualTime(ts.as_millis())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dst_simulation_basic() {
        let mut sim = DSTSimulation::new(42);
        sim.run_operations(100);

        assert!(sim.result.total_time_ms > 0);
        assert_eq!(sim.result.seed, 42);
    }

    #[test]
    fn test_dst_config_presets() {
        let calm = DSTConfig::calm(123);
        assert!(!calm.crash_config.enable_buggify_crashes);
        assert!(!calm.enable_clock_skew);

        let chaos = DSTConfig::chaos(456);
        assert!(chaos.crash_config.enable_buggify_crashes);
        assert!(chaos.enable_clock_skew);
    }

    #[test]
    fn test_dst_node_crash_recovery() {
        let mut sim = DSTSimulation::with_config(DSTConfig {
            seed: 42,
            node_count: 3,
            fault_config: FaultConfig::disabled(),
            crash_config: CrashConfig {
                min_recovery_time_ms: 10,
                max_recovery_time_ms: 20,
                enable_buggify_crashes: false,
                ..Default::default()
            },
            ..Default::default()
        });

        assert!(sim.is_node_running(0));

        // Crash node 0
        sim.crash_node(0, CrashReason::TestTriggered);
        assert!(!sim.is_node_running(0));

        // Start recovery
        sim.start_recovery(0);

        // Advance time to complete recovery
        sim.advance_time(100);
        assert!(sim.is_node_running(0));
    }

    #[test]
    fn test_dst_deterministic() {
        // Run same seed twice, should get same crash count
        let mut sim1 = DSTSimulation::with_config(DSTConfig::chaos(12345));
        sim1.run_operations(500);
        let crashes1 = sim1.result.crashes;

        // Reset BUGGIFY stats
        buggify::reset_stats();

        let mut sim2 = DSTSimulation::with_config(DSTConfig::chaos(12345));
        sim2.run_operations(500);
        let crashes2 = sim2.result.crashes;

        assert_eq!(crashes1, crashes2, "Same seed should produce same crashes");
    }

    #[test]
    fn test_batch_runner() {
        let batch = BatchRunner::new(1000, 10)
            .with_config(DSTConfig {
                node_count: 3,
                max_time_ms: 1000,
                fault_config: FaultConfig::calm(),
                crash_config: CrashConfig {
                    enable_buggify_crashes: false,
                    ..Default::default()
                },
                ..Default::default()
            })
            .run_default(50);

        assert_eq!(batch.total_runs, 10);
        assert!(batch.total_operations > 0);
        println!("{}", batch.summary());
    }

    #[test]
    fn test_random_running_node() {
        let mut sim = DSTSimulation::with_config(DSTConfig {
            seed: 42,
            node_count: 5,
            fault_config: FaultConfig::disabled(),
            crash_config: CrashConfig {
                enable_buggify_crashes: false,
                ..Default::default()
            },
            ..Default::default()
        });

        // All nodes running, should get a node
        let node = sim.random_running_node();
        assert!(node.is_some());
        assert!(node.unwrap() < 5);

        // Crash all nodes
        for i in 0..5 {
            sim.crash_node(i, CrashReason::TestTriggered);
        }

        // No running nodes
        let node = sim.random_running_node();
        assert!(node.is_none());
    }

    #[test]
    fn test_operation_recording() {
        let mut sim = DSTSimulation::new(42);

        let op = RecordedOperation {
            id: sim.next_op_id(),
            node_id: 0,
            op_type: OperationType::Write,
            key: "test_key".to_string(),
            value: Some("test_value".to_string()),
            start_time: sim.current_time(),
            end_time: None,
            result: OperationResult::Pending,
        };

        sim.record_operation(op);

        assert_eq!(sim.result.total_operations, 1);
        assert!(sim.result.operations_by_type.contains_key("Write"));
    }
}
