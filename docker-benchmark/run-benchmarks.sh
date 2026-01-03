#!/bin/bash
set -e

echo "=========================================="
echo "Redis Benchmark Comparison (Docker)"
echo "=========================================="
echo ""

# Configuration
REQUESTS=100000
CLIENTS=50
PIPELINE=1
DATA_SIZE=64

cd "$(dirname "$0")"

echo "Configuration:"
echo "  Requests: $REQUESTS"
echo "  Clients: $CLIENTS"
echo "  Pipeline: $PIPELINE"
echo "  Data size: $DATA_SIZE bytes"
echo "  CPU limit: 2 cores per container"
echo "  Memory limit: 1GB per container"
echo ""

# Start containers
echo "Starting containers..."
docker compose down 2>/dev/null || true
docker compose up -d --build

# Wait for servers to be ready
echo "Waiting for servers to start..."
sleep 5

# Check if servers are ready with RESP protocol
echo "Checking server connectivity..."
docker run --rm --network host redis:7.4-alpine redis-cli -p 6379 PING || { echo "Official Redis not ready"; exit 1; }
docker run --rm --network host redis:7.4-alpine redis-cli -p 3000 PING || { echo "Rust Redis not ready"; exit 1; }

echo ""
echo "=========================================="
echo "Official Redis 7.4 Benchmark"
echo "=========================================="
# Use -r for random keys to avoid conflicts
docker run --rm --network host redis:7.4-alpine \
    redis-benchmark -p 6379 -n $REQUESTS -c $CLIENTS -P $PIPELINE -d $DATA_SIZE -r 10000 \
    -q SET key:__rand_int__ value

docker run --rm --network host redis:7.4-alpine \
    redis-benchmark -p 6379 -n $REQUESTS -c $CLIENTS -P $PIPELINE -r 10000 \
    -q GET key:__rand_int__

docker run --rm --network host redis:7.4-alpine \
    redis-benchmark -p 6379 -n $REQUESTS -c $CLIENTS -P $PIPELINE -r 10000 \
    -q INCR counter:__rand_int__

echo ""
echo "=========================================="
echo "Rust Redis Implementation Benchmark"
echo "=========================================="
docker run --rm --network host redis:7.4-alpine \
    redis-benchmark -p 3000 -n $REQUESTS -c $CLIENTS -P $PIPELINE -d $DATA_SIZE -r 10000 \
    -q SET key:__rand_int__ value

docker run --rm --network host redis:7.4-alpine \
    redis-benchmark -p 3000 -n $REQUESTS -c $CLIENTS -P $PIPELINE -r 10000 \
    -q GET key:__rand_int__

docker run --rm --network host redis:7.4-alpine \
    redis-benchmark -p 3000 -n $REQUESTS -c $CLIENTS -P $PIPELINE -r 10000 \
    -q INCR counter:__rand_int__

echo ""
echo "=========================================="
echo "Pipelined Benchmark (pipeline=16)"
echo "=========================================="
echo ""
echo "--- Official Redis 7.4 ---"
docker run --rm --network host redis:7.4-alpine \
    redis-benchmark -p 6379 -n $REQUESTS -c $CLIENTS -P 16 -d $DATA_SIZE -r 10000 \
    -q SET pipekey:__rand_int__ value

docker run --rm --network host redis:7.4-alpine \
    redis-benchmark -p 6379 -n $REQUESTS -c $CLIENTS -P 16 -r 10000 \
    -q GET pipekey:__rand_int__

echo ""
echo "--- Rust Implementation ---"
docker run --rm --network host redis:7.4-alpine \
    redis-benchmark -p 3000 -n $REQUESTS -c $CLIENTS -P 16 -d $DATA_SIZE -r 10000 \
    -q SET pipekey:__rand_int__ value

docker run --rm --network host redis:7.4-alpine \
    redis-benchmark -p 3000 -n $REQUESTS -c $CLIENTS -P 16 -r 10000 \
    -q GET pipekey:__rand_int__

# Cleanup
echo ""
echo "Cleaning up..."
docker compose down

echo ""
echo "Benchmark complete!"
