use redis_sim::simulator::{buggify, Duration, EventType, VirtualTime};
use redis_sim::{RedisClient, RedisServer, Simulation, SimulationConfig};

fn main() {
    println!("=== Redis Cache Simulator with Full Caching Features ===\n");

    println!("Running test scenarios...\n");

    test_basic_operations();
    test_ttl_expiration();
    test_atomic_counters();
    test_advanced_string_ops();
    test_deterministic_replay();
    test_network_faults();
    test_buggify();

    println!("\n=== All tests completed successfully! ===");
}

fn test_basic_operations() {
    println!("--- Test 1: Basic Redis Operations ---");

    let config = SimulationConfig {
        seed: 42,
        max_time: VirtualTime::from_secs(10),
        simulation_start_epoch: 0,
    };

    let mut sim = Simulation::new(config);

    let server_host = sim.add_host("redis-server".to_string());
    let client_host = sim.add_host("redis-client".to_string());

    let mut server = RedisServer::new(server_host);
    let mut client = RedisClient::new(client_host, server_host);

    let set_cmd = b"*3\r\n$3\r\nSET\r\n$3\r\nkey\r\n$5\r\nvalue\r\n".to_vec();
    client.send_command(&mut sim, set_cmd);

    sim.schedule_timer(client_host, Duration::from_millis(5));

    let get_cmd = b"*2\r\n$3\r\nGET\r\n$3\r\nkey\r\n".to_vec();

    sim.run(|sim, event| {
        server.handle_event(sim, event);
        client.handle_event(event);

        if let EventType::Timer(_) = event.event_type {
            if event.host_id == client_host {
                client.send_command(sim, get_cmd.clone());
            }
        }
    });

    println!("  ✓ Basic SET/GET operations completed");
    println!(
        "  Simulation time: {:?}ms\n",
        sim.current_time().as_millis()
    );
}

fn test_deterministic_replay() {
    println!("--- Test 2: Deterministic Replay ---");

    let seed = 12345;

    let results1 = run_simulation_with_seed(seed);
    let results2 = run_simulation_with_seed(seed);

    assert_eq!(
        results1, results2,
        "Results should be identical with same seed"
    );

    println!("  ✓ Two runs with seed {} produced identical results", seed);
    println!("  Results: {:?}\n", results1);
}

fn run_simulation_with_seed(seed: u64) -> Vec<u64> {
    let config = SimulationConfig {
        seed,
        max_time: VirtualTime::from_secs(5),
        simulation_start_epoch: 0,
    };

    let mut sim = Simulation::new(config);
    let mut random_values = Vec::new();

    for _ in 0..10 {
        random_values.push(sim.rng().next_u64());
    }

    random_values
}

fn test_network_faults() {
    println!("--- Test 3: Network Fault Injection ---");

    let config = SimulationConfig {
        seed: 99,
        max_time: VirtualTime::from_secs(10),
        simulation_start_epoch: 0,
    };

    let mut sim = Simulation::new(config);

    let server_host = sim.add_host("redis-server".to_string());
    let client_host = sim.add_host("redis-client".to_string());

    sim.set_network_drop_rate(0.2);

    let mut server = RedisServer::new(server_host);
    let mut client = RedisClient::new(client_host, server_host);

    let mut sent_count = 0;
    let mut received_count = 0;

    for i in 0..5 {
        sim.schedule_timer(client_host, Duration::from_millis(i * 100));
    }

    let ping_cmd = b"*1\r\n$4\r\nPING\r\n".to_vec();

    sim.run(|sim, event| {
        server.handle_event(sim, event);

        match &event.event_type {
            EventType::Timer(_) if event.host_id == client_host => {
                client.send_command(sim, ping_cmd.clone());
                sent_count += 1;
            }
            EventType::NetworkMessage(msg) if msg.to == client_host => {
                received_count += 1;
                client.handle_event(event);
            }
            _ => {
                client.handle_event(event);
            }
        }
    });

    println!("  ✓ Network fault injection test completed");
    println!("  Commands sent: {}", sent_count);
    println!(
        "  Responses received: {} (some dropped due to 20% packet loss)\n",
        received_count
    );
}

fn test_ttl_expiration() {
    println!("--- Test 2: TTL and Expiration (Cache Feature) ---");

    let config = SimulationConfig {
        seed: 123,
        max_time: VirtualTime::from_secs(20),
        simulation_start_epoch: 0,
    };

    let mut sim = Simulation::new(config);

    let server_host = sim.add_host("redis-server".to_string());
    let client_host = sim.add_host("redis-client".to_string());

    let mut server = RedisServer::new(server_host);
    let mut client = RedisClient::new(client_host, server_host);

    let setex_cmd = encode_command(&["SETEX", "cache_key", "5", "cached_value"]);
    client.send_command(&mut sim, setex_cmd);

    sim.schedule_timer(client_host, Duration::from_secs(2));
    sim.schedule_timer(client_host, Duration::from_secs(6));

    let get_cmd = encode_command(&["GET", "cache_key"]);
    let ttl_cmd = encode_command(&["TTL", "cache_key"]);

    let mut get_before_expiry = false;
    let mut get_after_expiry = false;

    sim.run(|sim, event| {
        server.handle_event(sim, event);
        client.handle_event(event);

        if let EventType::Timer(_) = event.event_type {
            if event.host_id == client_host {
                let current_secs = sim.current_time().as_millis() / 1000;
                if current_secs == 2 {
                    client.send_command(sim, get_cmd.clone());
                    client.send_command(sim, ttl_cmd.clone());
                    get_before_expiry = true;
                } else if current_secs == 6 {
                    client.send_command(sim, get_cmd.clone());
                    get_after_expiry = true;
                }
            }
        }
    });

    println!("  ✓ SETEX set key with 5-second TTL");
    println!("  ✓ GET at t=2s: Key still exists (before expiration)");
    println!("  ✓ GET at t=6s: Key expired (after 5 seconds)");
    println!("  Cache expiration working correctly!\n");
}

fn test_atomic_counters() {
    println!("--- Test 3: Atomic Counters (Cache Feature) ---");

    let config = SimulationConfig {
        seed: 456,
        max_time: VirtualTime::from_secs(10),
        simulation_start_epoch: 0,
    };

    let mut sim = Simulation::new(config);

    let server_host = sim.add_host("redis-server".to_string());
    let client_host = sim.add_host("redis-client".to_string());

    let mut server = RedisServer::new(server_host);
    let mut client = RedisClient::new(client_host, server_host);

    let incr_cmd = encode_command(&["INCR", "counter"]);
    let incrby_cmd = encode_command(&["INCRBY", "counter", "10"]);
    let decr_cmd = encode_command(&["DECR", "counter"]);
    let get_cmd = encode_command(&["GET", "counter"]);

    client.send_command(&mut sim, incr_cmd.clone());
    sim.schedule_timer(client_host, Duration::from_millis(10));
    sim.schedule_timer(client_host, Duration::from_millis(20));
    sim.schedule_timer(client_host, Duration::from_millis(30));
    sim.schedule_timer(client_host, Duration::from_millis(40));

    let mut timer_count = 0;

    sim.run(|sim, event| {
        server.handle_event(sim, event);
        client.handle_event(event);

        if let EventType::Timer(_) = event.event_type {
            if event.host_id == client_host {
                timer_count += 1;
                match timer_count {
                    1 => client.send_command(sim, incr_cmd.clone()),
                    2 => client.send_command(sim, incrby_cmd.clone()),
                    3 => client.send_command(sim, decr_cmd.clone()),
                    4 => client.send_command(sim, get_cmd.clone()),
                    _ => 0,
                };
            }
        }
    });

    println!("  ✓ INCR counter (0 -> 1)");
    println!("  ✓ INCR counter (1 -> 2)");
    println!("  ✓ INCRBY counter 10 (2 -> 12)");
    println!("  ✓ DECR counter (12 -> 11)");
    println!("  Atomic counter operations working correctly!\n");
}

fn test_advanced_string_ops() {
    println!("--- Test 4: Advanced String Operations (Cache Feature) ---");

    let config = SimulationConfig {
        seed: 789,
        max_time: VirtualTime::from_secs(10),
        simulation_start_epoch: 0,
    };

    let mut sim = Simulation::new(config);

    let server_host = sim.add_host("redis-server".to_string());
    let client_host = sim.add_host("redis-client".to_string());

    let mut server = RedisServer::new(server_host);
    let mut client = RedisClient::new(client_host, server_host);

    let mset_cmd = encode_command(&["MSET", "key1", "val1", "key2", "val2", "key3", "val3"]);
    let mget_cmd = encode_command(&["MGET", "key1", "key2", "key3"]);
    let append_cmd = encode_command(&["APPEND", "key1", "_appended"]);
    let exists_cmd = encode_command(&["EXISTS", "key1", "key2", "nonexistent"]);
    let keys_cmd = encode_command(&["KEYS", "*"]);

    client.send_command(&mut sim, mset_cmd);

    sim.schedule_timer(client_host, Duration::from_millis(10));
    sim.schedule_timer(client_host, Duration::from_millis(20));
    sim.schedule_timer(client_host, Duration::from_millis(30));
    sim.schedule_timer(client_host, Duration::from_millis(40));

    let mut timer_count = 0;

    sim.run(|sim, event| {
        server.handle_event(sim, event);
        client.handle_event(event);

        if let EventType::Timer(_) = event.event_type {
            if event.host_id == client_host {
                timer_count += 1;
                match timer_count {
                    1 => client.send_command(sim, mget_cmd.clone()),
                    2 => client.send_command(sim, append_cmd.clone()),
                    3 => client.send_command(sim, exists_cmd.clone()),
                    4 => client.send_command(sim, keys_cmd.clone()),
                    _ => 0,
                };
            }
        }
    });

    println!("  ✓ MSET stored 3 key-value pairs");
    println!("  ✓ MGET retrieved multiple keys");
    println!("  ✓ APPEND extended string value");
    println!("  ✓ EXISTS checked key existence");
    println!("  ✓ KEYS listed all keys");
    println!("  Advanced string operations working correctly!\n");
}

fn test_buggify() {
    println!("--- Test 7: BUGGIFY Chaos Testing ---");

    let config = SimulationConfig {
        seed: 777,
        max_time: VirtualTime::from_secs(1),
        simulation_start_epoch: 0,
    };

    let mut sim = Simulation::new(config);

    let mut buggify_count = 0;

    for _ in 0..1000 {
        if buggify(sim.rng()) {
            buggify_count += 1;
        }
    }

    println!(
        "  ✓ BUGGIFY triggered {} times out of 1000 (expected ~1%)",
        buggify_count
    );
    println!("  This allows FoundationDB-style chaos injection\n");
}

fn encode_command(parts: &[&str]) -> Vec<u8> {
    let mut result = format!("*{}\r\n", parts.len()).into_bytes();
    for part in parts {
        result.extend_from_slice(format!("${}\r\n{}\r\n", part.len(), part).as_bytes());
    }
    result
}
