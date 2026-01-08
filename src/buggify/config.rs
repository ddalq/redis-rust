//! BUGGIFY Configuration
//!
//! Defines fault probabilities and provides preset configurations for different
//! testing scenarios (calm, moderate, chaos).

use super::faults;
use std::collections::HashMap;

/// Configuration for fault injection probabilities
#[derive(Debug, Clone)]
pub struct FaultConfig {
    /// Whether BUGGIFY is enabled at all
    pub enabled: bool,
    /// Per-fault probabilities (0.0 to 1.0)
    pub probabilities: HashMap<&'static str, f64>,
    /// Global probability multiplier
    pub global_multiplier: f64,
}

impl Default for FaultConfig {
    fn default() -> Self {
        Self::moderate()
    }
}

impl FaultConfig {
    /// Create a new empty config (all faults disabled)
    pub fn new() -> Self {
        FaultConfig {
            enabled: true,
            probabilities: HashMap::new(),
            global_multiplier: 1.0,
        }
    }

    /// Disabled - no fault injection
    pub fn disabled() -> Self {
        FaultConfig {
            enabled: false,
            probabilities: HashMap::new(),
            global_multiplier: 0.0,
        }
    }

    /// Calm - very low fault rates for basic testing
    pub fn calm() -> Self {
        let mut config = Self::new();
        config.global_multiplier = 0.1;

        // Network (very rare)
        config.set(faults::network::PACKET_DROP, 0.001);
        config.set(faults::network::DELAY, 0.01);

        // Timer (minimal)
        config.set(faults::timer::DRIFT_FAST, 0.001);
        config.set(faults::timer::DRIFT_SLOW, 0.001);

        config
    }

    /// Moderate - balanced fault injection for regular testing
    pub fn moderate() -> Self {
        let mut config = Self::new();
        config.global_multiplier = 1.0;

        // Network faults
        config.set(faults::network::PACKET_DROP, 0.01); // 1%
        config.set(faults::network::PACKET_CORRUPT, 0.001); // 0.1%
        config.set(faults::network::PARTIAL_WRITE, 0.005); // 0.5%
        config.set(faults::network::REORDER, 0.02); // 2%
        config.set(faults::network::CONNECTION_RESET, 0.005); // 0.5%
        config.set(faults::network::CONNECT_TIMEOUT, 0.01); // 1%
        config.set(faults::network::DELAY, 0.05); // 5%
        config.set(faults::network::DUPLICATE, 0.005); // 0.5%

        // Timer faults
        config.set(faults::timer::DRIFT_FAST, 0.01); // 1%
        config.set(faults::timer::DRIFT_SLOW, 0.01); // 1%
        config.set(faults::timer::SKIP, 0.01); // 1%
        config.set(faults::timer::DUPLICATE, 0.005); // 0.5%
        config.set(faults::timer::JUMP_FORWARD, 0.001); // 0.1%
        config.set(faults::timer::JUMP_BACKWARD, 0.0005); // 0.05%

        // Process faults
        config.set(faults::process::CRASH, 0.001); // 0.1%
        config.set(faults::process::PAUSE, 0.01); // 1%
        config.set(faults::process::SLOW, 0.02); // 2%
        config.set(faults::process::OOM, 0.0001); // 0.01%
        config.set(faults::process::CPU_STARVATION, 0.01); // 1%

        // Disk faults (for future persistence)
        config.set(faults::disk::WRITE_FAIL, 0.001); // 0.1%
        config.set(faults::disk::PARTIAL_WRITE, 0.001); // 0.1%
        config.set(faults::disk::CORRUPTION, 0.0001); // 0.01%
        config.set(faults::disk::SLOW, 0.02); // 2%
        config.set(faults::disk::FSYNC_FAIL, 0.0005); // 0.05%
        config.set(faults::disk::STALE_READ, 0.001); // 0.1%
        config.set(faults::disk::DISK_FULL, 0.0001); // 0.01%

        // Replication faults
        config.set(faults::replication::GOSSIP_DROP, 0.02); // 2%
        config.set(faults::replication::GOSSIP_DELAY, 0.05); // 5%
        config.set(faults::replication::GOSSIP_CORRUPT, 0.001); // 0.1%
        config.set(faults::replication::SPLIT_BRAIN, 0.0001); // 0.01%
        config.set(faults::replication::STALE_REPLICA, 0.01); // 1%

        config
    }

    /// Chaos - aggressive fault injection for stress testing
    pub fn chaos() -> Self {
        let mut config = Self::new();
        config.global_multiplier = 3.0;

        // Network faults (high)
        config.set(faults::network::PACKET_DROP, 0.05); // 5%
        config.set(faults::network::PACKET_CORRUPT, 0.01); // 1%
        config.set(faults::network::PARTIAL_WRITE, 0.02); // 2%
        config.set(faults::network::REORDER, 0.10); // 10%
        config.set(faults::network::CONNECTION_RESET, 0.02); // 2%
        config.set(faults::network::CONNECT_TIMEOUT, 0.05); // 5%
        config.set(faults::network::DELAY, 0.15); // 15%
        config.set(faults::network::DUPLICATE, 0.02); // 2%

        // Timer faults (high)
        config.set(faults::timer::DRIFT_FAST, 0.05); // 5%
        config.set(faults::timer::DRIFT_SLOW, 0.05); // 5%
        config.set(faults::timer::SKIP, 0.05); // 5%
        config.set(faults::timer::DUPLICATE, 0.02); // 2%
        config.set(faults::timer::JUMP_FORWARD, 0.01); // 1%
        config.set(faults::timer::JUMP_BACKWARD, 0.005); // 0.5%

        // Process faults (elevated)
        config.set(faults::process::CRASH, 0.005); // 0.5%
        config.set(faults::process::PAUSE, 0.05); // 5%
        config.set(faults::process::SLOW, 0.10); // 10%
        config.set(faults::process::OOM, 0.001); // 0.1%
        config.set(faults::process::CPU_STARVATION, 0.05); // 5%

        // Disk faults (elevated)
        config.set(faults::disk::WRITE_FAIL, 0.005); // 0.5%
        config.set(faults::disk::PARTIAL_WRITE, 0.005); // 0.5%
        config.set(faults::disk::CORRUPTION, 0.001); // 0.1%
        config.set(faults::disk::SLOW, 0.10); // 10%
        config.set(faults::disk::FSYNC_FAIL, 0.002); // 0.2%
        config.set(faults::disk::STALE_READ, 0.005); // 0.5%
        config.set(faults::disk::DISK_FULL, 0.001); // 0.1%

        // Replication faults (high)
        config.set(faults::replication::GOSSIP_DROP, 0.10); // 10%
        config.set(faults::replication::GOSSIP_DELAY, 0.15); // 15%
        config.set(faults::replication::GOSSIP_CORRUPT, 0.005); // 0.5%
        config.set(faults::replication::SPLIT_BRAIN, 0.001); // 0.1%
        config.set(faults::replication::STALE_REPLICA, 0.05); // 5%

        config
    }

    /// Set probability for a specific fault
    pub fn set(&mut self, fault_id: &'static str, probability: f64) -> &mut Self {
        self.probabilities
            .insert(fault_id, probability.clamp(0.0, 1.0));
        self
    }

    /// Get probability for a fault (returns 0.0 if not set)
    pub fn get(&self, fault_id: &str) -> f64 {
        if !self.enabled {
            return 0.0;
        }
        let base = self.probabilities.get(fault_id).copied().unwrap_or(0.0);
        (base * self.global_multiplier).clamp(0.0, 1.0)
    }

    /// Check if a fault should trigger given its probability
    pub fn should_trigger(&self, fault_id: &str, random_value: f64) -> bool {
        random_value < self.get(fault_id)
    }

    /// Builder pattern - enable specific fault category
    pub fn with_network_faults(mut self) -> Self {
        self.set(faults::network::PACKET_DROP, 0.01);
        self.set(faults::network::PACKET_CORRUPT, 0.001);
        self.set(faults::network::REORDER, 0.02);
        self.set(faults::network::DELAY, 0.05);
        self
    }

    /// Builder pattern - enable timer faults
    pub fn with_timer_faults(mut self) -> Self {
        self.set(faults::timer::DRIFT_FAST, 0.01);
        self.set(faults::timer::DRIFT_SLOW, 0.01);
        self.set(faults::timer::SKIP, 0.01);
        self
    }

    /// Builder pattern - enable process faults
    pub fn with_process_faults(mut self) -> Self {
        self.set(faults::process::CRASH, 0.001);
        self.set(faults::process::PAUSE, 0.01);
        self.set(faults::process::SLOW, 0.02);
        self
    }

    /// Builder pattern - set global multiplier
    pub fn with_multiplier(mut self, multiplier: f64) -> Self {
        self.global_multiplier = multiplier.max(0.0);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_disabled_config() {
        let config = FaultConfig::disabled();
        assert_eq!(config.get(faults::network::PACKET_DROP), 0.0);
        assert!(!config.should_trigger(faults::network::PACKET_DROP, 0.0));
    }

    #[test]
    fn test_moderate_config() {
        let config = FaultConfig::moderate();
        assert!(config.get(faults::network::PACKET_DROP) > 0.0);
        assert!(config.get(faults::network::PACKET_DROP) <= 1.0);
    }

    #[test]
    fn test_chaos_higher_than_moderate() {
        let moderate = FaultConfig::moderate();
        let chaos = FaultConfig::chaos();

        assert!(
            chaos.get(faults::network::PACKET_DROP) > moderate.get(faults::network::PACKET_DROP)
        );
    }

    #[test]
    fn test_should_trigger() {
        let config = FaultConfig::moderate();
        let prob = config.get(faults::network::PACKET_DROP);

        // Value below probability should trigger
        assert!(config.should_trigger(faults::network::PACKET_DROP, prob - 0.001));
        // Value above probability should not trigger
        assert!(!config.should_trigger(faults::network::PACKET_DROP, prob + 0.001));
    }

    #[test]
    fn test_builder_pattern() {
        let config = FaultConfig::new()
            .with_network_faults()
            .with_multiplier(2.0);

        assert!(config.get(faults::network::PACKET_DROP) > 0.0);
        assert_eq!(config.global_multiplier, 2.0);
    }
}
