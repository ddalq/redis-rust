#!/bin/bash
set -e

echo "=========================================="
echo "Redis 7.4 vs Redis 8.0 vs Rust Comparison"
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

# Start containers using the Redis 8 compose file
echo "Starting containers..."
docker compose -f docker-compose.redis8.yml down 2>/dev/null || true
docker compose -f docker-compose.redis8.yml up -d --build

# Wait for servers to be ready
echo "Waiting for servers to start..."
sleep 8

# Check connectivity
echo "Checking server connectivity..."
echo -n "  Redis 7.4 (port 6379): "
docker run --rm --network host redis:7.4-alpine redis-cli -p 6379 PING || { echo "FAILED"; exit 1; }
echo -n "  Redis 8.0 (port 6380): "
docker run --rm --network host redis:8.0 redis-cli -p 6380 PING || { echo "FAILED"; exit 1; }
echo -n "  Rust impl (port 3000): "
docker run --rm --network host redis:7.4-alpine redis-cli -p 3000 PING || { echo "FAILED"; exit 1; }

# Function to run benchmark and capture result
run_bench() {
    local port=$1
    local label=$2
    local cmd=$3
    local pipeline=${4:-1}

    docker run --rm --network host redis:8.0 \
        redis-benchmark -p $port -n $REQUESTS -c $CLIENTS -P $pipeline -d $DATA_SIZE -r 10000 \
        -t $cmd --csv 2>/dev/null
}

# Function to extract throughput from CSV
get_throughput() {
    local csv_output="$1"
    echo "$csv_output" | grep -E "^\"(SET|GET)\"" | head -1 | cut -d',' -f2 | tr -d '"'
}

echo ""
echo "=========================================="
echo "Running Benchmarks"
echo "=========================================="

# Create results file
RESULTS_FILE="$RESULTS_DIR/redis8_comparison_${TIMESTAMP}.md"

cat > "$RESULTS_FILE" << 'HEADER'
# Redis 7.4 vs Redis 8.0 vs Rust Implementation

## Test Configuration
- **Method**: Docker benchmarks (docker-compose.redis8.yml)
- **CPU Limit**: 2 cores per container
- **Memory Limit**: 1GB per container
- **Requests**: 100,000
- **Clients**: 50 concurrent
- **Data Size**: 64 bytes

HEADER

# Run all benchmarks
echo ""
echo "--- Non-Pipelined (P=1) ---"

echo "Running Redis 7.4 SET (P=1)..."
R7_SET_P1=$(run_bench 6379 "Redis_7.4" "set" 1)
R7_SET_P1_TPS=$(get_throughput "$R7_SET_P1")
echo "  Result: $R7_SET_P1_TPS req/sec"

echo "Running Redis 8.0 SET (P=1)..."
R8_SET_P1=$(run_bench 6380 "Redis_8.0" "set" 1)
R8_SET_P1_TPS=$(get_throughput "$R8_SET_P1")
echo "  Result: $R8_SET_P1_TPS req/sec"

echo "Running Rust SET (P=1)..."
RUST_SET_P1=$(run_bench 3000 "Rust" "set" 1)
RUST_SET_P1_TPS=$(get_throughput "$RUST_SET_P1")
echo "  Result: $RUST_SET_P1_TPS req/sec"

echo "Running Redis 7.4 GET (P=1)..."
R7_GET_P1=$(run_bench 6379 "Redis_7.4" "get" 1)
R7_GET_P1_TPS=$(get_throughput "$R7_GET_P1")
echo "  Result: $R7_GET_P1_TPS req/sec"

echo "Running Redis 8.0 GET (P=1)..."
R8_GET_P1=$(run_bench 6380 "Redis_8.0" "get" 1)
R8_GET_P1_TPS=$(get_throughput "$R8_GET_P1")
echo "  Result: $R8_GET_P1_TPS req/sec"

echo "Running Rust GET (P=1)..."
RUST_GET_P1=$(run_bench 3000 "Rust" "get" 1)
RUST_GET_P1_TPS=$(get_throughput "$RUST_GET_P1")
echo "  Result: $RUST_GET_P1_TPS req/sec"

echo ""
echo "--- Pipelined (P=16) ---"

echo "Running Redis 7.4 SET (P=16)..."
R7_SET_P16=$(run_bench 6379 "Redis_7.4" "set" 16)
R7_SET_P16_TPS=$(get_throughput "$R7_SET_P16")
echo "  Result: $R7_SET_P16_TPS req/sec"

echo "Running Redis 8.0 SET (P=16)..."
R8_SET_P16=$(run_bench 6380 "Redis_8.0" "set" 16)
R8_SET_P16_TPS=$(get_throughput "$R8_SET_P16")
echo "  Result: $R8_SET_P16_TPS req/sec"

echo "Running Rust SET (P=16)..."
RUST_SET_P16=$(run_bench 3000 "Rust" "set" 16)
RUST_SET_P16_TPS=$(get_throughput "$RUST_SET_P16")
echo "  Result: $RUST_SET_P16_TPS req/sec"

echo "Running Redis 7.4 GET (P=16)..."
R7_GET_P16=$(run_bench 6379 "Redis_7.4" "get" 16)
R7_GET_P16_TPS=$(get_throughput "$R7_GET_P16")
echo "  Result: $R7_GET_P16_TPS req/sec"

echo "Running Redis 8.0 GET (P=16)..."
R8_GET_P16=$(run_bench 6380 "Redis_8.0" "get" 16)
R8_GET_P16_TPS=$(get_throughput "$R8_GET_P16")
echo "  Result: $R8_GET_P16_TPS req/sec"

echo "Running Rust GET (P=16)..."
RUST_GET_P16=$(run_bench 3000 "Rust" "get" 16)
RUST_GET_P16_TPS=$(get_throughput "$RUST_GET_P16")
echo "  Result: $RUST_GET_P16_TPS req/sec"

# Calculate percentages
calc_pct() {
    echo "scale=1; $1 * 100 / $2" | bc 2>/dev/null || echo "N/A"
}

RUST_VS_R7_SET_P1=$(calc_pct "$RUST_SET_P1_TPS" "$R7_SET_P1_TPS")
RUST_VS_R8_SET_P1=$(calc_pct "$RUST_SET_P1_TPS" "$R8_SET_P1_TPS")
RUST_VS_R7_GET_P1=$(calc_pct "$RUST_GET_P1_TPS" "$R7_GET_P1_TPS")
RUST_VS_R8_GET_P1=$(calc_pct "$RUST_GET_P1_TPS" "$R8_GET_P1_TPS")

RUST_VS_R7_SET_P16=$(calc_pct "$RUST_SET_P16_TPS" "$R7_SET_P16_TPS")
RUST_VS_R8_SET_P16=$(calc_pct "$RUST_SET_P16_TPS" "$R8_SET_P16_TPS")
RUST_VS_R7_GET_P16=$(calc_pct "$RUST_GET_P16_TPS" "$R7_GET_P16_TPS")
RUST_VS_R8_GET_P16=$(calc_pct "$RUST_GET_P16_TPS" "$R8_GET_P16_TPS")

R8_VS_R7_SET_P1=$(calc_pct "$R8_SET_P1_TPS" "$R7_SET_P1_TPS")
R8_VS_R7_GET_P1=$(calc_pct "$R8_GET_P1_TPS" "$R7_GET_P1_TPS")
R8_VS_R7_SET_P16=$(calc_pct "$R8_SET_P16_TPS" "$R7_SET_P16_TPS")
R8_VS_R7_GET_P16=$(calc_pct "$R8_GET_P16_TPS" "$R7_GET_P16_TPS")

# Write results table
cat >> "$RESULTS_FILE" << EOF
## Results

### Non-Pipelined Performance (P=1)

| Operation | Redis 7.4 | Redis 8.0 | Rust | R8 vs R7 | Rust vs R7 | Rust vs R8 |
|-----------|-----------|-----------|------|----------|------------|------------|
| SET | $R7_SET_P1_TPS | $R8_SET_P1_TPS | $RUST_SET_P1_TPS | ${R8_VS_R7_SET_P1}% | ${RUST_VS_R7_SET_P1}% | ${RUST_VS_R8_SET_P1}% |
| GET | $R7_GET_P1_TPS | $R8_GET_P1_TPS | $RUST_GET_P1_TPS | ${R8_VS_R7_GET_P1}% | ${RUST_VS_R7_GET_P1}% | ${RUST_VS_R8_GET_P1}% |

### Pipelined Performance (P=16)

| Operation | Redis 7.4 | Redis 8.0 | Rust | R8 vs R7 | Rust vs R7 | Rust vs R8 |
|-----------|-----------|-----------|------|----------|------------|------------|
| SET | $R7_SET_P16_TPS | $R8_SET_P16_TPS | $RUST_SET_P16_TPS | ${R8_VS_R7_SET_P16}% | ${RUST_VS_R7_SET_P16}% | ${RUST_VS_R8_SET_P16}% |
| GET | $R7_GET_P16_TPS | $R8_GET_P16_TPS | $RUST_GET_P16_TPS | ${R8_VS_R7_GET_P16}% | ${RUST_VS_R7_GET_P16}% | ${RUST_VS_R8_GET_P16}% |

## Summary

### Redis 8.0 vs 7.4 Changes
- SET P=1: ${R8_VS_R7_SET_P1}% of Redis 7.4
- GET P=1: ${R8_VS_R7_GET_P1}% of Redis 7.4
- SET P=16: ${R8_VS_R7_SET_P16}% of Redis 7.4
- GET P=16: ${R8_VS_R7_GET_P16}% of Redis 7.4

### Rust vs Redis 8.0
- SET P=1: ${RUST_VS_R8_SET_P1}% of Redis 8.0
- GET P=1: ${RUST_VS_R8_GET_P1}% of Redis 8.0
- SET P=16: ${RUST_VS_R8_SET_P16}% of Redis 8.0
- GET P=16: ${RUST_VS_R8_GET_P16}% of Redis 8.0

---
Generated: $(date)
EOF

echo ""
echo "=========================================="
echo "Results Summary"
echo "=========================================="
echo ""
echo "Non-Pipelined (P=1):"
echo "  SET: Redis 7.4=$R7_SET_P1_TPS | Redis 8.0=$R8_SET_P1_TPS | Rust=$RUST_SET_P1_TPS"
echo "  GET: Redis 7.4=$R7_GET_P1_TPS | Redis 8.0=$R8_GET_P1_TPS | Rust=$RUST_GET_P1_TPS"
echo ""
echo "Pipelined (P=16):"
echo "  SET: Redis 7.4=$R7_SET_P16_TPS | Redis 8.0=$R8_SET_P16_TPS | Rust=$RUST_SET_P16_TPS"
echo "  GET: Redis 7.4=$R7_GET_P16_TPS | Redis 8.0=$R8_GET_P16_TPS | Rust=$RUST_GET_P16_TPS"
echo ""
echo "Full results saved to: $RESULTS_FILE"

# Cleanup
echo ""
echo "Cleaning up..."
docker compose -f docker-compose.redis8.yml down

echo ""
echo "Benchmark complete!"
