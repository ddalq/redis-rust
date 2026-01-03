//! Skewed workload test for hot key detection
//!
//! Tests that the adaptive replication system correctly identifies
//! hot keys under Zipfian-like access patterns.

use redis_sim::production::{
    AdaptiveConfig, AdaptiveReplicationManager, HotKeyConfig, HotKeyDetector,
    LoadBalancerConfig, ScalingDecision, ShardLoadBalancer, ShardMetrics,
};

/// Zipfian distribution generator (simplified)
/// Returns key index where lower indices are much more frequent
fn zipfian_key(rng: &mut u64, num_keys: usize, skew: f64) -> usize {
    // Simple LCG for deterministic testing
    *rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
    let u = (*rng as f64) / (u64::MAX as f64);

    // Zipf: P(k) proportional to 1/k^s
    // Inverse transform sampling approximation
    let k = ((1.0 - u).powf(-1.0 / skew) - 1.0).floor() as usize;
    k.min(num_keys - 1)
}

#[test]
fn test_skewed_workload_hot_key_detection() {
    // Configure for testing - lower thresholds
    let config = HotKeyConfig {
        window_ms: 10_000,
        hot_threshold: 50.0, // 50 ops/sec to be "hot"
        cleanup_interval_ms: 5_000,
        max_tracked_keys: 1000,
    };
    let mut detector = HotKeyDetector::new(config);

    let num_keys = 100;
    let num_operations = 10_000;
    let skew = 1.5; // Higher = more skewed

    // Simulate skewed workload over 1 second
    let mut rng = 12345u64;
    for i in 0..num_operations {
        let key_idx = zipfian_key(&mut rng, num_keys, skew);
        let key = format!("key:{}", key_idx);
        let timestamp_ms = (i as u64 * 1000) / num_operations as u64; // Spread over 1 second
        detector.record_access(&key, false, timestamp_ms);
    }

    let now_ms = 1000;
    let hot_keys = detector.get_hot_keys(now_ms);
    let top_10 = detector.get_top_keys(10, now_ms);

    println!("\n=== Skewed Workload Results ===");
    println!("Total operations: {}", num_operations);
    println!("Total unique keys: {}", num_keys);
    println!("Keys tracked: {}", detector.tracked_key_count());
    println!("Hot keys detected: {}", hot_keys.len());

    println!("\nTop 10 hottest keys:");
    for (i, (key, rate)) in top_10.iter().enumerate() {
        let is_hot = rate >= &50.0;
        println!(
            "  {}. {} - {:.1} ops/sec {}",
            i + 1,
            key,
            rate,
            if is_hot { "[HOT]" } else { "" }
        );
    }

    // Verify hot keys are detected
    assert!(
        !hot_keys.is_empty(),
        "Should detect at least one hot key with skewed workload"
    );

    // Verify key:0 is the hottest (most frequent in Zipfian)
    assert_eq!(
        top_10[0].0, "key:0",
        "key:0 should be the hottest with Zipfian distribution"
    );

    // Verify hot keys have significantly higher access rate than cold keys
    let hottest_rate = top_10[0].1;
    let coldest_rate = detector
        .get_metrics(&format!("key:{}", num_keys - 1))
        .map(|m| m.access_rate(now_ms))
        .unwrap_or(0.0);

    println!("\nHottest rate: {:.1} ops/sec", hottest_rate);
    println!("Coldest rate: {:.1} ops/sec", coldest_rate);
    println!(
        "Skew ratio: {:.1}x",
        if coldest_rate > 0.0 {
            hottest_rate / coldest_rate
        } else {
            f64::INFINITY
        }
    );

    assert!(
        hottest_rate > coldest_rate * 5.0,
        "Hot keys should have much higher access rate than cold keys"
    );
}

#[test]
fn test_adaptive_replication_with_skewed_workload() {
    let config = AdaptiveConfig {
        base_rf: 3,
        hot_key_rf: 5,
        recalc_interval_ms: 100,
        hotkey_config: HotKeyConfig {
            window_ms: 5000,
            hot_threshold: 30.0, // 30 ops/sec for testing
            cleanup_interval_ms: 1000,
            max_tracked_keys: 500,
        },
    };
    let mut manager = AdaptiveReplicationManager::new(config);

    let num_keys = 50;
    let num_operations = 5000;
    let skew = 1.8;

    // Simulate skewed workload
    let mut rng = 54321u64;
    for i in 0..num_operations {
        let key_idx = zipfian_key(&mut rng, num_keys, skew);
        let key = format!("user:{}", key_idx);
        let timestamp_ms = (i as u64 * 1000) / num_operations as u64;
        let is_write = (i % 5) == 0; // 20% writes
        manager.observe(&key, is_write, timestamp_ms);
    }

    // Force recalculation
    manager.force_recalculate(1000);

    let stats = manager.stats();
    let hot_updates = manager.get_hot_key_updates();

    println!("\n=== Adaptive Replication Results ===");
    println!("Base RF: {}", stats.base_rf);
    println!("Hot Key RF: {}", stats.hot_rf);
    println!("Current hot keys: {}", stats.current_hot_keys);
    println!("Total promotions: {}", stats.total_promotions);
    println!("Tracked keys: {}", stats.tracked_keys);

    println!("\nHot key RF assignments:");
    for (key, rf) in &hot_updates {
        println!("  {} -> RF={}", key, rf);
    }

    // Verify cold keys get base RF
    let cold_key = format!("user:{}", num_keys - 1);
    assert_eq!(
        manager.get_rf_for_key(&cold_key),
        3,
        "Cold key should have base RF=3"
    );

    // Verify hot keys get elevated RF
    if !hot_updates.is_empty() {
        let (hot_key, rf) = &hot_updates[0];
        assert_eq!(*rf, 5, "Hot key {} should have RF=5", hot_key);
        assert_eq!(
            manager.get_rf_for_key(hot_key),
            5,
            "get_rf_for_key should return elevated RF for hot key"
        );
    }

    // Verify some keys were promoted
    assert!(
        stats.current_hot_keys > 0,
        "Should have detected hot keys with skewed workload"
    );
}

#[test]
fn test_load_balancer_with_skewed_shard_load() {
    let config = LoadBalancerConfig {
        max_imbalance: 2.0,
        min_keys_for_split: 1000,
        min_ops_for_scale: 5000.0,
        scaling_cooldown_ms: 0, // Disable for testing
        min_shards: 1,
        max_shards: 16,
    };
    let mut balancer = ShardLoadBalancer::new(4, config);

    // Simulate skewed load across shards (shard 0 gets most load)
    let shard_loads = [8000.0, 2000.0, 1500.0, 500.0]; // ops/sec per shard

    for (shard_id, &ops) in shard_loads.iter().enumerate() {
        balancer.update_metrics(
            shard_id,
            ShardMetrics {
                shard_id,
                key_count: (ops as usize) * 10,
                ops_per_second: ops,
                memory_bytes: (ops as usize) * 100,
                last_update_ms: 1000,
            },
        );
    }

    let decision = balancer.analyze(1000);
    let stats = balancer.get_stats();
    let distribution = balancer.get_distribution();

    println!("\n=== Load Balancer Results ===");
    println!("Shards: {}", stats.num_shards);
    println!("Total ops/sec: {:.0}", stats.total_ops_per_sec);
    println!("Imbalance ratio: {:.2}x", stats.imbalance_ratio);
    println!("Min load: {:.0}", stats.min_load);
    println!("Max load: {:.0}", stats.max_load);

    println!("\nLoad distribution:");
    for (shard_id, pct) in &distribution {
        println!("  Shard {}: {:.1}%", shard_id, pct);
    }

    println!("\nScaling decision: {:?}", decision);

    // Verify imbalance is detected
    assert!(
        stats.imbalance_ratio > 2.0,
        "Should detect high imbalance ratio"
    );

    // Verify rebalancing is recommended
    match decision {
        ScalingDecision::RebalanceRecommended {
            from_shard,
            imbalance_ratio,
            ..
        } => {
            assert_eq!(from_shard, 0, "Should recommend moving load from shard 0");
            assert!(imbalance_ratio > 2.0);
        }
        ScalingDecision::AddShard { .. } => {
            // Also acceptable if total load triggers scale-up
            println!("Scale-up recommended due to high total load");
        }
        _ => panic!("Expected RebalanceRecommended or AddShard, got {:?}", decision),
    }
}

#[test]
fn test_hot_key_demotion_after_cooldown() {
    let config = AdaptiveConfig {
        base_rf: 3,
        hot_key_rf: 5,
        recalc_interval_ms: 100,
        hotkey_config: HotKeyConfig {
            window_ms: 500, // Short window for testing
            hot_threshold: 20.0,
            cleanup_interval_ms: 100,
            max_tracked_keys: 100,
        },
    };
    let mut manager = AdaptiveReplicationManager::new(config);

    // Make key hot with accesses over 500ms
    for i in 0..50 {
        manager.observe("hot_key", false, i * 10);
    }
    manager.force_recalculate(500);

    let initial_rf = manager.get_rf_for_key("hot_key");
    println!("\n=== Demotion Test ===");
    println!("Initial RF for hot_key: {}", initial_rf);
    assert_eq!(initial_rf, 5, "Key should be hot initially");

    // Time passes with no accesses - key rate drops below threshold
    // Access rate = 50 ops / 500ms = 100 ops/sec initially
    // After 2000ms with no new accesses, rate = 50 ops / 2000ms = 25 ops/sec
    // But this is still above threshold of 20.0...

    // Let's check if the key is still "hot" by its access rate at time 5000ms
    // At t=5000ms, rate = 50 ops / 5000ms = 10 ops/sec (below 20 threshold)
    let is_hot_at_5000 = manager.is_hot("hot_key", 5000);
    println!("Is hot at t=5000ms: {}", is_hot_at_5000);

    // Force recalculation at a later time when rate drops below threshold
    manager.force_recalculate(5000);

    let stats = manager.stats();
    let final_rf = manager.get_rf_for_key("hot_key");
    println!("Hot keys after cooldown: {}", stats.current_hot_keys);
    println!("Total demotions: {}", stats.total_demotions);
    println!("Final RF for hot_key: {}", final_rf);

    // Key should now be demoted (rate fell below threshold)
    assert!(
        final_rf == 3 || stats.total_demotions > 0,
        "Key should be demoted after rate drops below threshold"
    );
}

#[test]
fn test_realistic_workload_simulation() {
    // Simulate a realistic 10-second workload with:
    // - 80% reads, 20% writes
    // - Zipfian key distribution
    // - 10000 ops/second throughput

    let config = AdaptiveConfig {
        base_rf: 3,
        hot_key_rf: 5,
        recalc_interval_ms: 1000,
        hotkey_config: HotKeyConfig {
            window_ms: 10_000,
            hot_threshold: 100.0, // Realistic threshold
            cleanup_interval_ms: 2000,
            max_tracked_keys: 5000,
        },
    };
    let mut manager = AdaptiveReplicationManager::new(config);

    let num_keys = 10_000;
    let ops_per_second = 10_000;
    let duration_seconds = 10;
    let total_ops = ops_per_second * duration_seconds;
    let skew = 1.2; // Moderate skew

    println!("\n=== Realistic Workload Simulation ===");
    println!("Keys: {}", num_keys);
    println!("Throughput: {} ops/sec", ops_per_second);
    println!("Duration: {} seconds", duration_seconds);
    println!("Total ops: {}", total_ops);

    let mut rng = 99999u64;
    let mut key_access_counts: std::collections::HashMap<String, u64> =
        std::collections::HashMap::new();

    for i in 0..total_ops {
        let key_idx = zipfian_key(&mut rng, num_keys, skew);
        let key = format!("item:{}", key_idx);
        let timestamp_ms = (i as u64 * 1000) / ops_per_second as u64;
        let is_write = (i % 5) == 0;

        manager.observe(&key, is_write, timestamp_ms);
        *key_access_counts.entry(key).or_insert(0) += 1;
    }

    manager.force_recalculate(duration_seconds as u64 * 1000);

    let stats = manager.stats();
    let top_keys = manager.get_top_hot_keys(10, duration_seconds as u64 * 1000);

    println!("\nResults:");
    println!("  Tracked keys: {}", stats.tracked_keys);
    println!("  Hot keys detected: {}", stats.current_hot_keys);
    println!("  Promotions: {}", stats.total_promotions);

    println!("\nTop 10 keys by access rate:");
    for (i, (key, rate)) in top_keys.iter().enumerate() {
        let actual_count = key_access_counts.get(key).unwrap_or(&0);
        println!(
            "  {}. {} - {:.1} ops/sec (actual: {} total accesses)",
            i + 1,
            key,
            rate,
            actual_count
        );
    }

    // Calculate actual distribution
    let mut sorted_counts: Vec<_> = key_access_counts.iter().collect();
    sorted_counts.sort_by(|a, b| b.1.cmp(a.1));

    let top_10_accesses: u64 = sorted_counts.iter().take(10).map(|(_, &c)| c).sum();
    let total_accesses: u64 = sorted_counts.iter().map(|(_, &c)| c).sum();
    let top_10_pct = (top_10_accesses as f64 / total_accesses as f64) * 100.0;

    println!("\nAccess distribution:");
    println!(
        "  Top 10 keys: {:.1}% of all accesses",
        top_10_pct
    );
    println!(
        "  Top 1% keys: {:.1}% of all accesses",
        sorted_counts
            .iter()
            .take(num_keys / 100)
            .map(|(_, &c)| c)
            .sum::<u64>() as f64
            / total_accesses as f64
            * 100.0
    );

    // Verify detection worked
    assert!(
        stats.current_hot_keys > 0,
        "Should detect hot keys in realistic workload"
    );

    // Verify detected hot keys match actual high-access keys
    if let Some((top_detected, _)) = top_keys.first() {
        let (top_actual, _) = sorted_counts[0];
        println!("\nValidation:");
        println!("  Most accessed key: {}", top_actual);
        println!("  Detected hottest: {}", top_detected);

        // The detected hottest should be in the top 5 actual
        let top_5_actual: Vec<_> = sorted_counts.iter().take(5).map(|(k, _)| *k).collect();
        assert!(
            top_5_actual.contains(&top_detected),
            "Detected hot key should be among actual top keys"
        );
    }
}
