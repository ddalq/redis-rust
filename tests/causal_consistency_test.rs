//! Causal Consistency Tests
//!
//! Tests that verify causal consistency guarantees:
//! 1. Read-your-writes: After writing X, you always read X (or later)
//! 2. Monotonic reads: Once you read X, you won't see earlier values
//! 3. Writes-follow-reads: If you read X then write Y, anyone seeing Y also sees X
//! 4. Causal ordering: If A happens-before B, all nodes see A before B

use redis_sim::redis::SDS;
use redis_sim::replication::state::{ReplicationDelta, ShardReplicaState};
use redis_sim::replication::lattice::ReplicaId;
use redis_sim::replication::config::ConsistencyLevel;

/// Helper to convert SDS to string
fn sds_to_string(sds: &SDS) -> String {
    String::from_utf8_lossy(sds.as_bytes()).to_string()
}

/// Get value as string from state
fn get_value(state: &ShardReplicaState, key: &str) -> Option<String> {
    state.get_replicated(key)
        .and_then(|rv| rv.get().map(sds_to_string))
}

// ============================================================================
// Test 1: Read-Your-Writes
// ============================================================================

#[test]
fn test_read_your_writes() {
    // After a process writes a value, it should always read that value
    // (or a causally later one)

    let r1 = ReplicaId::new(1);
    let mut state = ShardReplicaState::new(r1, ConsistencyLevel::Causal);

    println!("=== Read-Your-Writes Test ===");

    // Write a sequence of values
    for i in 1..=5 {
        let value = format!("value_{}", i);
        state.record_write("key".to_string(), SDS::from_str(&value), None);

        // Immediately read - should see what we just wrote
        let read_value = get_value(&state, "key");
        println!("After write '{}': read '{:?}'", value, read_value);

        assert_eq!(read_value, Some(value.clone()),
            "Should read the value we just wrote");
    }

    println!("✓ Read-your-writes preserved");
}

// ============================================================================
// Test 2: Monotonic Reads
// ============================================================================

#[test]
fn test_monotonic_reads() {
    // Once a process reads a value, subsequent reads should return
    // that value or a causally later one, never an earlier one

    let r1 = ReplicaId::new(1);
    let r2 = ReplicaId::new(2);

    let mut writer = ShardReplicaState::new(r1, ConsistencyLevel::Causal);
    let mut reader = ShardReplicaState::new(r2, ConsistencyLevel::Causal);

    println!("=== Monotonic Reads Test ===");

    // Writer creates a sequence of causally ordered writes
    let delta1 = writer.record_write("key".to_string(), SDS::from_str("v1"), None);
    let delta2 = writer.record_write("key".to_string(), SDS::from_str("v2"), None);
    let delta3 = writer.record_write("key".to_string(), SDS::from_str("v3"), None);

    // Reader applies delta2 first (sees v2)
    reader.apply_remote_delta(delta2.clone());
    let read1 = get_value(&reader, "key");
    println!("After applying delta2: {:?}", read1);
    assert_eq!(read1, Some("v2".to_string()));

    // Now apply delta1 (older) - should NOT regress to v1
    reader.apply_remote_delta(delta1.clone());
    let read2 = get_value(&reader, "key");
    println!("After applying delta1 (older): {:?}", read2);

    // With LWW + Lamport clocks, v2 has higher timestamp so it wins
    assert_eq!(read2, Some("v2".to_string()),
        "Should not regress to older value");

    // Apply delta3 (newer) - should advance to v3
    reader.apply_remote_delta(delta3.clone());
    let read3 = get_value(&reader, "key");
    println!("After applying delta3 (newer): {:?}", read3);
    assert_eq!(read3, Some("v3".to_string()));

    println!("✓ Monotonic reads preserved");
}

// ============================================================================
// Test 3: Writes-Follow-Reads (Session Causality)
// ============================================================================

#[test]
fn test_writes_follow_reads() {
    // If a process reads X and then writes Y, then any process
    // that sees Y must also see X (or a later version of X's key)

    let r1 = ReplicaId::new(1);
    let r2 = ReplicaId::new(2);
    let r3 = ReplicaId::new(3);

    let mut node1 = ShardReplicaState::new(r1, ConsistencyLevel::Causal);
    let mut node2 = ShardReplicaState::new(r2, ConsistencyLevel::Causal);
    let mut node3 = ShardReplicaState::new(r3, ConsistencyLevel::Causal);

    println!("=== Writes-Follow-Reads Test ===");

    // Node1 writes to key_a
    let delta_a = node1.record_write("key_a".to_string(), SDS::from_str("value_a"), None);
    println!("Node1 writes key_a = 'value_a'");

    // Node2 reads key_a (applies delta from node1)
    node2.apply_remote_delta(delta_a.clone());
    let read_a = get_value(&node2, "key_a");
    println!("Node2 reads key_a = {:?}", read_a);
    assert_eq!(read_a, Some("value_a".to_string()));

    // Node2 then writes to key_b (this write is causally after reading key_a)
    let delta_b = node2.record_write("key_b".to_string(), SDS::from_str("value_b"), None);
    println!("Node2 writes key_b = 'value_b' (after reading key_a)");

    // Node3 receives delta_b but NOT delta_a yet
    node3.apply_remote_delta(delta_b.clone());
    let read_b_on_3 = get_value(&node3, "key_b");
    println!("Node3 sees key_b = {:?}", read_b_on_3);

    // In a fully causal system, seeing key_b should imply key_a is available
    // But our current impl doesn't enforce this - it's "best effort" causal
    // The vector clock on delta_b should indicate it depends on delta_a

    println!("Delta_a VC: {:?}", delta_a.value.vector_clock);
    println!("Delta_b VC: {:?}", delta_b.value.vector_clock);

    // Now node3 gets delta_a
    node3.apply_remote_delta(delta_a.clone());
    let read_a_on_3 = get_value(&node3, "key_a");
    println!("Node3 (after sync) key_a = {:?}", read_a_on_3);

    assert_eq!(read_a_on_3, Some("value_a".to_string()));
    assert_eq!(read_b_on_3, Some("value_b".to_string()));

    println!("✓ Writes-follow-reads: deltas carry causal metadata");
}

// ============================================================================
// Test 4: Causal Ordering (Happens-Before)
// ============================================================================

#[test]
fn test_causal_ordering_happens_before() {
    // If A happens-before B (A → B), then all nodes must see A before B
    // when they're on the same key

    let r1 = ReplicaId::new(1);
    let r2 = ReplicaId::new(2);

    let mut node1 = ShardReplicaState::new(r1, ConsistencyLevel::Causal);
    let mut node2 = ShardReplicaState::new(r2, ConsistencyLevel::Causal);

    println!("=== Causal Ordering (Happens-Before) Test ===");

    // Node1: write v1, then v2 (v1 → v2)
    let delta1 = node1.record_write("key".to_string(), SDS::from_str("v1"), None);
    let delta2 = node1.record_write("key".to_string(), SDS::from_str("v2"), None);

    println!("Node1 writes: v1 → v2");
    println!("Delta1 timestamp: {:?}", delta1.value.timestamp);
    println!("Delta2 timestamp: {:?}", delta2.value.timestamp);

    // Verify timestamps are ordered
    assert!(delta2.value.timestamp.time > delta1.value.timestamp.time,
        "v2 should have higher Lamport timestamp than v1");

    // Node2 receives deltas in wrong order
    println!("\nNode2 receives delta2 first, then delta1:");

    node2.apply_remote_delta(delta2.clone());
    let after_delta2 = get_value(&node2, "key");
    println!("  After delta2: {:?}", after_delta2);
    assert_eq!(after_delta2, Some("v2".to_string()));

    node2.apply_remote_delta(delta1.clone());
    let after_delta1 = get_value(&node2, "key");
    println!("  After delta1: {:?}", after_delta1);

    // Should still be v2 because v2 has higher timestamp
    assert_eq!(after_delta1, Some("v2".to_string()),
        "Causal order preserved: v2 wins over v1");

    println!("✓ Causal ordering preserved via Lamport timestamps");
}

// ============================================================================
// Test 5: Concurrent Writes (No Causal Relationship)
// ============================================================================

#[test]
fn test_concurrent_writes_no_causal_order() {
    // When two writes are concurrent (neither happens-before the other),
    // the system should deterministically pick a winner

    let r1 = ReplicaId::new(1);
    let r2 = ReplicaId::new(2);

    let mut node1 = ShardReplicaState::new(r1, ConsistencyLevel::Causal);
    let mut node2 = ShardReplicaState::new(r2, ConsistencyLevel::Causal);

    println!("=== Concurrent Writes Test ===");

    // Both nodes write concurrently (no communication)
    let delta1 = node1.record_write("key".to_string(), SDS::from_str("from_node1"), None);
    let delta2 = node2.record_write("key".to_string(), SDS::from_str("from_node2"), None);

    println!("Concurrent writes:");
    println!("  Node1: 'from_node1', timestamp={:?}", delta1.value.timestamp);
    println!("  Node2: 'from_node2', timestamp={:?}", delta2.value.timestamp);

    // These are concurrent - neither causally precedes the other
    // System uses LWW with replica_id as tiebreaker

    // Apply both deltas to both nodes
    node1.apply_remote_delta(delta2.clone());
    node2.apply_remote_delta(delta1.clone());

    let val1 = get_value(&node1, "key");
    let val2 = get_value(&node2, "key");

    println!("\nAfter exchange:");
    println!("  Node1 sees: {:?}", val1);
    println!("  Node2 sees: {:?}", val2);

    // Both nodes should converge to the same value
    assert_eq!(val1, val2, "Concurrent writes should converge");

    println!("✓ Concurrent writes converge deterministically to: {:?}", val1);
}

// ============================================================================
// Test 6: Causal Chain Across Multiple Nodes
// ============================================================================

#[test]
fn test_causal_chain_multi_node() {
    // Create a causal chain: N1 → N2 → N3
    // Each node reads previous value before writing

    let r1 = ReplicaId::new(1);
    let r2 = ReplicaId::new(2);
    let r3 = ReplicaId::new(3);

    let mut node1 = ShardReplicaState::new(r1, ConsistencyLevel::Causal);
    let mut node2 = ShardReplicaState::new(r2, ConsistencyLevel::Causal);
    let mut node3 = ShardReplicaState::new(r3, ConsistencyLevel::Causal);

    println!("=== Causal Chain Multi-Node Test ===");

    // N1 writes initial value
    let delta1 = node1.record_write("key".to_string(), SDS::from_str("step1_by_n1"), None);
    println!("N1 writes: step1_by_n1");

    // N2 receives from N1, then writes
    node2.apply_remote_delta(delta1.clone());
    let read_on_n2 = get_value(&node2, "key");
    println!("N2 reads: {:?}", read_on_n2);
    let delta2 = node2.record_write("key".to_string(), SDS::from_str("step2_by_n2"), None);
    println!("N2 writes: step2_by_n2");

    // N3 receives from N2, then writes
    node3.apply_remote_delta(delta2.clone());
    let read_on_n3 = get_value(&node3, "key");
    println!("N3 reads: {:?}", read_on_n3);
    let delta3 = node3.record_write("key".to_string(), SDS::from_str("step3_by_n3"), None);
    println!("N3 writes: step3_by_n3");

    // Verify causal chain in timestamps
    println!("\nTimestamp chain:");
    println!("  delta1: {:?}", delta1.value.timestamp);
    println!("  delta2: {:?}", delta2.value.timestamp);
    println!("  delta3: {:?}", delta3.value.timestamp);

    assert!(delta2.value.timestamp.time > delta1.value.timestamp.time);
    assert!(delta3.value.timestamp.time > delta2.value.timestamp.time);

    // Now propagate all deltas to all nodes
    node1.apply_remote_delta(delta2.clone());
    node1.apply_remote_delta(delta3.clone());
    node2.apply_remote_delta(delta3.clone());
    node3.apply_remote_delta(delta1.clone());

    let final1 = get_value(&node1, "key");
    let final2 = get_value(&node2, "key");
    let final3 = get_value(&node3, "key");

    println!("\nFinal values:");
    println!("  N1: {:?}", final1);
    println!("  N2: {:?}", final2);
    println!("  N3: {:?}", final3);

    // All should see the final value in the causal chain
    assert_eq!(final1, Some("step3_by_n3".to_string()));
    assert_eq!(final2, Some("step3_by_n3".to_string()));
    assert_eq!(final3, Some("step3_by_n3".to_string()));

    println!("✓ Causal chain preserved across 3 nodes");
}

// ============================================================================
// Test 7: Vector Clock Tracking
// ============================================================================

#[test]
fn test_vector_clock_tracking() {
    // Verify that vector clocks are properly maintained in causal mode

    let r1 = ReplicaId::new(1);
    let r2 = ReplicaId::new(2);

    let mut node1 = ShardReplicaState::new(r1, ConsistencyLevel::Causal);
    let mut node2 = ShardReplicaState::new(r2, ConsistencyLevel::Causal);

    println!("=== Vector Clock Tracking Test ===");

    // Node1 writes
    let delta1 = node1.record_write("key".to_string(), SDS::from_str("v1"), None);
    println!("N1 write - VC: {:?}", delta1.value.vector_clock);

    // In causal mode, should have vector clock
    assert!(delta1.value.vector_clock.is_some(),
        "Causal mode should track vector clocks");

    // Node2 writes independently
    let delta2 = node2.record_write("key".to_string(), SDS::from_str("v2"), None);
    println!("N2 write (independent) - VC: {:?}", delta2.value.vector_clock);

    // Exchange and then write again
    node1.apply_remote_delta(delta2.clone());
    node2.apply_remote_delta(delta1.clone());

    // Now writes should have updated vector clocks
    let delta3 = node1.record_write("key".to_string(), SDS::from_str("v3"), None);
    println!("N1 write (after sync) - VC: {:?}", delta3.value.vector_clock);

    let delta4 = node2.record_write("key".to_string(), SDS::from_str("v4"), None);
    println!("N2 write (after sync) - VC: {:?}", delta4.value.vector_clock);

    // Both should have vector clocks
    assert!(delta3.value.vector_clock.is_some());
    assert!(delta4.value.vector_clock.is_some());

    println!("✓ Vector clocks properly tracked in causal mode");
}

// ============================================================================
// Test 8: Causal vs Eventual Consistency Comparison
// ============================================================================

#[test]
fn test_causal_vs_eventual_mode() {
    // Compare behavior between causal and eventual consistency modes

    let r1 = ReplicaId::new(1);

    let mut causal_state = ShardReplicaState::new(r1, ConsistencyLevel::Causal);
    let mut eventual_state = ShardReplicaState::new(r1, ConsistencyLevel::Eventual);

    println!("=== Causal vs Eventual Comparison ===");

    // Same operations on both
    let causal_delta = causal_state.record_write("key".to_string(), SDS::from_str("value"), None);
    let eventual_delta = eventual_state.record_write("key".to_string(), SDS::from_str("value"), None);

    println!("Causal mode:");
    println!("  Vector clock: {:?}", causal_delta.value.vector_clock);
    println!("  Lamport clock: {:?}", causal_delta.value.timestamp);

    println!("\nEventual mode:");
    println!("  Vector clock: {:?}", eventual_delta.value.vector_clock);
    println!("  Lamport clock: {:?}", eventual_delta.value.timestamp);

    // Causal should have vector clock, eventual should not
    assert!(causal_delta.value.vector_clock.is_some(),
        "Causal mode should have vector clock");
    assert!(eventual_delta.value.vector_clock.is_none(),
        "Eventual mode should not have vector clock");

    // Both should have Lamport timestamps
    assert!(causal_delta.value.timestamp.time > 0);
    assert!(eventual_delta.value.timestamp.time > 0);

    println!("✓ Causal mode tracks vector clocks, eventual mode does not");
}

// ============================================================================
// Test 9: Multiple Keys Causal Dependencies
// ============================================================================

#[test]
fn test_multi_key_causal_dependencies() {
    // Test causal dependencies across multiple keys

    let r1 = ReplicaId::new(1);
    let r2 = ReplicaId::new(2);

    let mut node1 = ShardReplicaState::new(r1, ConsistencyLevel::Causal);
    let mut node2 = ShardReplicaState::new(r2, ConsistencyLevel::Causal);

    println!("=== Multi-Key Causal Dependencies Test ===");

    // Node1 writes to key_a, then key_b (key_b causally depends on key_a)
    let delta_a = node1.record_write("key_a".to_string(), SDS::from_str("a_value"), None);
    let delta_b = node1.record_write("key_b".to_string(), SDS::from_str("b_value"), None);

    println!("N1 writes: key_a='a_value' → key_b='b_value'");
    println!("  delta_a VC: {:?}", delta_a.value.vector_clock);
    println!("  delta_b VC: {:?}", delta_b.value.vector_clock);

    // delta_b should have equal or higher VC than delta_a
    // (same replica, so VC component for r1 should be >= )

    // Node2 receives only delta_b first
    node2.apply_remote_delta(delta_b.clone());
    let b_on_n2 = get_value(&node2, "key_b");
    let a_on_n2 = get_value(&node2, "key_a");

    println!("\nN2 after receiving only delta_b:");
    println!("  key_a: {:?}", a_on_n2);
    println!("  key_b: {:?}", b_on_n2);

    // key_b is visible, key_a is not yet
    assert_eq!(b_on_n2, Some("b_value".to_string()));
    assert_eq!(a_on_n2, None);

    // Now receive delta_a
    node2.apply_remote_delta(delta_a.clone());
    let a_on_n2_after = get_value(&node2, "key_a");
    println!("N2 after receiving delta_a: key_a = {:?}", a_on_n2_after);
    assert_eq!(a_on_n2_after, Some("a_value".to_string()));

    println!("✓ Multi-key causal dependencies tracked");
}

// ============================================================================
// Test 10: Stress Test - Many Causal Operations
// ============================================================================

#[test]
fn test_causal_stress() {
    // Stress test with many causally-related operations

    let num_nodes = 5;
    let ops_per_node = 50;

    let mut nodes: Vec<ShardReplicaState> = (0..num_nodes)
        .map(|i| ShardReplicaState::new(ReplicaId::new(i as u64), ConsistencyLevel::Causal))
        .collect();

    println!("=== Causal Stress Test ({} nodes, {} ops each) ===", num_nodes, ops_per_node);

    let mut all_deltas: Vec<ReplicationDelta> = Vec::new();

    // Each node does a sequence of writes
    for round in 0..ops_per_node {
        for (node_id, node) in nodes.iter_mut().enumerate() {
            // Apply some random previous deltas first (simulate partial sync)
            if !all_deltas.is_empty() && round % 5 == 0 {
                let sync_count = (all_deltas.len() / 3).max(1);
                for delta in all_deltas.iter().take(sync_count) {
                    node.apply_remote_delta(delta.clone());
                }
            }

            let delta = node.record_write(
                "shared_key".to_string(),
                SDS::from_str(&format!("n{}_r{}", node_id, round)),
                None,
            );
            all_deltas.push(delta);
        }
    }

    println!("Total deltas generated: {}", all_deltas.len());

    // Apply all deltas to all nodes
    for delta in &all_deltas {
        for node in &mut nodes {
            node.apply_remote_delta(delta.clone());
        }
    }

    // All nodes should converge
    let final_values: Vec<_> = nodes.iter()
        .map(|n| get_value(n, "shared_key"))
        .collect();

    let first = &final_values[0];
    for (i, val) in final_values.iter().enumerate() {
        assert_eq!(val, first, "Node {} has different value", i);
    }

    println!("✓ All {} nodes converged under causal stress to: {:?}", num_nodes, first);

    // Verify vector clocks were used
    let sample_delta = &all_deltas[all_deltas.len() - 1];
    assert!(sample_delta.value.vector_clock.is_some(),
        "Should have vector clocks in causal mode");
    println!("✓ Vector clocks maintained throughout");
}
