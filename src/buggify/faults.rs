//! Fault Catalog for Deterministic Simulation Testing
//!
//! Comprehensive list of injectable faults inspired by FoundationDB and TigerBeetle.
//! Each fault has a unique identifier for tracking and configuration.

/// Network faults - packet-level chaos
pub mod network {
    /// Drop packet entirely (1% default)
    pub const PACKET_DROP: &str = "network.packet_drop";
    /// Corrupt random bytes in packet (0.1% default)
    pub const PACKET_CORRUPT: &str = "network.packet_corrupt";
    /// Truncate packet to partial write (0.5% default)
    pub const PARTIAL_WRITE: &str = "network.partial_write";
    /// Reorder packet delivery (2% default)
    pub const REORDER: &str = "network.reorder";
    /// Reset connection unexpectedly (0.5% default)
    pub const CONNECTION_RESET: &str = "network.connection_reset";
    /// Timeout on connection attempt (1% default)
    pub const CONNECT_TIMEOUT: &str = "network.connect_timeout";
    /// Delay packet delivery significantly (5% default)
    pub const DELAY: &str = "network.delay";
    /// Duplicate packet (0.5% default)
    pub const DUPLICATE: &str = "network.duplicate";
}

/// Timer faults - clock chaos
pub mod timer {
    /// Clock runs faster than real time (drift +1000 ppm)
    pub const DRIFT_FAST: &str = "timer.drift_fast";
    /// Clock runs slower than real time (drift -1000 ppm)
    pub const DRIFT_SLOW: &str = "timer.drift_slow";
    /// Skip timer tick entirely (1% default)
    pub const SKIP: &str = "timer.skip";
    /// Duplicate timer tick (0.5% default)
    pub const DUPLICATE: &str = "timer.duplicate";
    /// Large instant clock jump forward
    pub const JUMP_FORWARD: &str = "timer.jump_forward";
    /// Small clock jump backward (NTP correction)
    pub const JUMP_BACKWARD: &str = "timer.jump_backward";
}

/// Process faults - node-level chaos
pub mod process {
    /// Crash process entirely (0.1% default)
    pub const CRASH: &str = "process.crash";
    /// Pause process execution (simulates GC, swapping) (1% default)
    pub const PAUSE: &str = "process.pause";
    /// Slow down processing significantly (2% default)
    pub const SLOW: &str = "process.slow";
    /// Out of memory error
    pub const OOM: &str = "process.oom";
    /// CPU starvation / scheduling delays
    pub const CPU_STARVATION: &str = "process.cpu_starvation";
}

/// Disk faults - persistence chaos (for future use)
pub mod disk {
    /// Write operation fails
    pub const WRITE_FAIL: &str = "disk.write_fail";
    /// Partial write to disk
    pub const PARTIAL_WRITE: &str = "disk.partial_write";
    /// Data corruption on disk
    pub const CORRUPTION: &str = "disk.corruption";
    /// Slow disk I/O
    pub const SLOW: &str = "disk.slow";
    /// fsync fails
    pub const FSYNC_FAIL: &str = "disk.fsync_fail";
    /// Read returns stale data
    pub const STALE_READ: &str = "disk.stale_read";
    /// Disk full error
    pub const DISK_FULL: &str = "disk.disk_full";
}

/// Object store faults - streaming persistence chaos
pub mod object_store {
    /// Put operation fails
    pub const PUT_FAIL: &str = "object_store.put_fail";
    /// Get operation fails
    pub const GET_FAIL: &str = "object_store.get_fail";
    /// Get returns corrupted data
    pub const GET_CORRUPT: &str = "object_store.get_corrupt";
    /// Operation times out
    pub const TIMEOUT: &str = "object_store.timeout";
    /// Partial write (segment truncated)
    pub const PARTIAL_WRITE: &str = "object_store.partial_write";
    /// Delete operation fails
    pub const DELETE_FAIL: &str = "object_store.delete_fail";
    /// List operation returns incomplete results
    pub const LIST_INCOMPLETE: &str = "object_store.list_incomplete";
    /// Rename/move operation fails (non-atomic)
    pub const RENAME_FAIL: &str = "object_store.rename_fail";
    /// Slow object store response
    pub const SLOW: &str = "object_store.slow";
}

/// Replication faults - distributed system chaos
pub mod replication {
    /// Drop gossip message
    pub const GOSSIP_DROP: &str = "replication.gossip_drop";
    /// Delay gossip significantly
    pub const GOSSIP_DELAY: &str = "replication.gossip_delay";
    /// Corrupt gossip payload
    pub const GOSSIP_CORRUPT: &str = "replication.gossip_corrupt";
    /// Split brain scenario
    pub const SPLIT_BRAIN: &str = "replication.split_brain";
    /// Stale replica response
    pub const STALE_REPLICA: &str = "replication.stale_replica";
}

/// All fault identifiers for iteration
pub const ALL_FAULTS: &[&str] = &[
    // Network
    network::PACKET_DROP,
    network::PACKET_CORRUPT,
    network::PARTIAL_WRITE,
    network::REORDER,
    network::CONNECTION_RESET,
    network::CONNECT_TIMEOUT,
    network::DELAY,
    network::DUPLICATE,
    // Timer
    timer::DRIFT_FAST,
    timer::DRIFT_SLOW,
    timer::SKIP,
    timer::DUPLICATE,
    timer::JUMP_FORWARD,
    timer::JUMP_BACKWARD,
    // Process
    process::CRASH,
    process::PAUSE,
    process::SLOW,
    process::OOM,
    process::CPU_STARVATION,
    // Disk
    disk::WRITE_FAIL,
    disk::PARTIAL_WRITE,
    disk::CORRUPTION,
    disk::SLOW,
    disk::FSYNC_FAIL,
    disk::STALE_READ,
    disk::DISK_FULL,
    // Object Store
    object_store::PUT_FAIL,
    object_store::GET_FAIL,
    object_store::GET_CORRUPT,
    object_store::TIMEOUT,
    object_store::PARTIAL_WRITE,
    object_store::DELETE_FAIL,
    object_store::LIST_INCOMPLETE,
    object_store::RENAME_FAIL,
    object_store::SLOW,
    // Replication
    replication::GOSSIP_DROP,
    replication::GOSSIP_DELAY,
    replication::GOSSIP_CORRUPT,
    replication::SPLIT_BRAIN,
    replication::STALE_REPLICA,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_faults_unique() {
        let mut seen = std::collections::HashSet::new();
        for fault in ALL_FAULTS {
            assert!(seen.insert(*fault), "Duplicate fault: {}", fault);
        }
    }

    #[test]
    fn test_fault_count() {
        // Verify we have 31+ faults as promised
        assert!(ALL_FAULTS.len() >= 31, "Expected 31+ faults, got {}", ALL_FAULTS.len());
    }
}
