#!/bin/bash
set -e

echo "=========================================="
echo "Detailed Redis Benchmark (Latency + Resources)"
echo "=========================================="
echo ""

# Configuration
REQUESTS=100000
CLIENTS=50
DATA_SIZE=64

cd "$(dirname "$0")"

# Output files
RESULTS_DIR="./results"
mkdir -p "$RESULTS_DIR"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)

echo "Configuration:"
echo "  Requests: $REQUESTS"
echo "  Clients: $CLIENTS"
echo "  Data size: $DATA_SIZE bytes"
echo "  CPU limit: 2 cores per container"
echo "  Memory limit: 1GB per container"
echo "  Results dir: $RESULTS_DIR"
echo ""

# Start containers
echo "Starting containers..."
docker compose down 2>/dev/null || true
docker compose up -d --build

# Wait for servers to be ready
echo "Waiting for servers to start..."
sleep 5

# Check connectivity
echo "Checking server connectivity..."
docker run --rm --network host redis:7.4-alpine redis-cli -p 6379 PING || { echo "Official Redis not ready"; exit 1; }
docker run --rm --network host redis:7.4-alpine redis-cli -p 3000 PING || { echo "Rust Redis not ready"; exit 1; }

# Function to get container stats
get_stats() {
    local container=$1
    docker stats --no-stream --format "{{.CPUPerc}},{{.MemUsage}}" "$container" 2>/dev/null || echo "N/A,N/A"
}

# Function to run benchmark with latency histogram
run_latency_benchmark() {
    local port=$1
    local label=$2
    local cmd=$3
    local pipeline=${4:-1}

    echo ""
    echo "--- $label (P=$pipeline) ---"

    # Get baseline memory
    if [ "$port" == "6379" ]; then
        BASELINE_STATS=$(get_stats "docker-benchmark-redis-official-1")
    else
        BASELINE_STATS=$(get_stats "docker-benchmark-redis-rust-1")
    fi
    echo "Baseline stats: $BASELINE_STATS"

    # Run benchmark with --csv for parseable output
    # Note: redis-benchmark doesn't have native latency histogram in older versions
    # We use the standard output which includes avg latency
    docker run --rm --network host redis:7.4-alpine \
        redis-benchmark -p $port -n $REQUESTS -c $CLIENTS -P $pipeline -d $DATA_SIZE -r 10000 \
        -t $cmd --csv 2>/dev/null | tee "$RESULTS_DIR/${label// /_}_P${pipeline}_${TIMESTAMP}.csv"

    # Get stats under load (run a quick load while measuring)
    echo ""
    echo "Resource usage under load:"
    docker run -d --rm --network host --name bench_load_$$ redis:7.4-alpine \
        redis-benchmark -p $port -n 50000 -c $CLIENTS -P $pipeline -d $DATA_SIZE -r 10000 -t $cmd -q &>/dev/null
    sleep 2

    if [ "$port" == "6379" ]; then
        LOAD_STATS=$(get_stats "docker-benchmark-redis-official-1")
    else
        LOAD_STATS=$(get_stats "docker-benchmark-redis-rust-1")
    fi
    echo "Under load: $LOAD_STATS"

    # Wait for background benchmark to finish
    wait 2>/dev/null || true
}

echo ""
echo "=========================================="
echo "Non-Pipelined Benchmarks (P=1)"
echo "=========================================="

# Official Redis P=1
run_latency_benchmark 6379 "Redis_7.4_SET" "set" 1
run_latency_benchmark 6379 "Redis_7.4_GET" "get" 1

# Rust Redis P=1
run_latency_benchmark 3000 "Rust_SET" "set" 1
run_latency_benchmark 3000 "Rust_GET" "get" 1

echo ""
echo "=========================================="
echo "Pipelined Benchmarks (P=16)"
echo "=========================================="

# Official Redis P=16
run_latency_benchmark 6379 "Redis_7.4_SET" "set" 16
run_latency_benchmark 6379 "Redis_7.4_GET" "get" 16

# Rust Redis P=16
run_latency_benchmark 3000 "Rust_SET" "set" 16
run_latency_benchmark 3000 "Rust_GET" "get" 16

echo ""
echo "=========================================="
echo "Memory Usage Comparison"
echo "=========================================="

# Flush and measure idle memory
docker run --rm --network host redis:7.4-alpine redis-cli -p 6379 FLUSHALL >/dev/null
docker run --rm --network host redis:7.4-alpine redis-cli -p 3000 FLUSHALL >/dev/null
sleep 2

echo "Idle memory:"
echo "  Redis 7.4: $(get_stats 'docker-benchmark-redis-official-1')"
echo "  Rust impl: $(get_stats 'docker-benchmark-redis-rust-1')"

# Load 100k keys and measure
echo ""
echo "Loading 100k keys..."
docker run --rm --network host redis:7.4-alpine \
    redis-benchmark -p 6379 -n 100000 -c 50 -r 100000 -t set -q >/dev/null
docker run --rm --network host redis:7.4-alpine \
    redis-benchmark -p 3000 -n 100000 -c 50 -r 100000 -t set -q >/dev/null
sleep 2

echo "With 100k keys:"
echo "  Redis 7.4: $(get_stats 'docker-benchmark-redis-official-1')"
echo "  Rust impl: $(get_stats 'docker-benchmark-redis-rust-1')"

# Cleanup
echo ""
echo "Cleaning up..."
docker compose down

echo ""
echo "=========================================="
echo "Results saved to: $RESULTS_DIR"
echo "=========================================="

# Generate summary
cat > "$RESULTS_DIR/summary_${TIMESTAMP}.md" << 'EOF'
# Benchmark Summary

## Test Configuration
- **Method**: Docker benchmarks
- **CPU Limit**: 2 cores per container
- **Memory Limit**: 1GB per container
- **Requests**: 100,000
- **Clients**: 50 concurrent
- **Data Size**: 64 bytes

## How to Reproduce
```bash
cd docker-benchmark
./run-detailed-benchmarks.sh
```

## Results
See CSV files in this directory for raw data.

## Notes
- Latency percentiles from redis-benchmark CSV output
- Resource usage captured via docker stats
- All tests use random keys (-r flag) to avoid conflicts
EOF

echo "Summary written to: $RESULTS_DIR/summary_${TIMESTAMP}.md"
echo ""
echo "Benchmark complete!"
