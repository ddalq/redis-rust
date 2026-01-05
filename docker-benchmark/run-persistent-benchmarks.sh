#!/bin/bash
set -e

echo "=========================================="
echo "Persistent Server Benchmark (Docker)"
echo "Redis AOF vs Rust S3 (MinIO)"
echo "=========================================="
echo ""

# Configuration
REQUESTS=50000
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
echo "Persistence:"
echo "  Redis: AOF with appendfsync=everysec"
echo "  Rust:  S3 streaming to MinIO"
echo ""

# Start containers
echo "Starting containers (this may take a while for first build)..."
docker compose -f docker-compose.persistent.yml down 2>/dev/null || true
docker compose -f docker-compose.persistent.yml up -d --build

# Wait for servers to be ready
echo "Waiting for servers to start..."
sleep 10

# Check MinIO is ready
echo "Checking MinIO..."
docker run --rm --network host curlimages/curl:latest \
    curl -sf http://localhost:9000/minio/health/live || { echo "MinIO not ready"; exit 1; }
echo "MinIO ready"

# Check if servers are ready with RESP protocol
echo "Checking server connectivity..."
for i in {1..10}; do
    if docker run --rm --network host redis:7.4-alpine redis-cli -p 6379 PING 2>/dev/null | grep -q PONG; then
        echo "Redis Official ready"
        break
    fi
    echo "Waiting for Redis Official... ($i/10)"
    sleep 2
done

for i in {1..10}; do
    if docker run --rm --network host redis:7.4-alpine redis-cli -p 6380 PING 2>/dev/null | grep -q PONG; then
        echo "Rust Persistent ready"
        break
    fi
    echo "Waiting for Rust Persistent... ($i/10)"
    sleep 2
done

# Verify both are responding
docker run --rm --network host redis:7.4-alpine redis-cli -p 6379 PING || { echo "Redis not ready"; exit 1; }
docker run --rm --network host redis:7.4-alpine redis-cli -p 6380 PING || { echo "Rust not ready"; exit 1; }

echo ""
echo "=========================================="
echo "Redis 7.4 with AOF Persistence"
echo "=========================================="
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
echo "Rust with S3 Persistence (MinIO)"
echo "=========================================="
docker run --rm --network host redis:7.4-alpine \
    redis-benchmark -p 6380 -n $REQUESTS -c $CLIENTS -P $PIPELINE -d $DATA_SIZE -r 10000 \
    -q SET key:__rand_int__ value

docker run --rm --network host redis:7.4-alpine \
    redis-benchmark -p 6380 -n $REQUESTS -c $CLIENTS -P $PIPELINE -r 10000 \
    -q GET key:__rand_int__

docker run --rm --network host redis:7.4-alpine \
    redis-benchmark -p 6380 -n $REQUESTS -c $CLIENTS -P $PIPELINE -r 10000 \
    -q INCR counter:__rand_int__

echo ""
echo "=========================================="
echo "Pipelined Benchmark (pipeline=16)"
echo "=========================================="
echo ""
echo "--- Redis 7.4 AOF ---"
docker run --rm --network host redis:7.4-alpine \
    redis-benchmark -p 6379 -n $REQUESTS -c $CLIENTS -P 16 -d $DATA_SIZE -r 10000 \
    -q SET pipekey:__rand_int__ value

docker run --rm --network host redis:7.4-alpine \
    redis-benchmark -p 6379 -n $REQUESTS -c $CLIENTS -P 16 -r 10000 \
    -q GET pipekey:__rand_int__

echo ""
echo "--- Rust S3 Persistent ---"
docker run --rm --network host redis:7.4-alpine \
    redis-benchmark -p 6380 -n $REQUESTS -c $CLIENTS -P 16 -d $DATA_SIZE -r 10000 \
    -q SET pipekey:__rand_int__ value

docker run --rm --network host redis:7.4-alpine \
    redis-benchmark -p 6380 -n $REQUESTS -c $CLIENTS -P 16 -r 10000 \
    -q GET pipekey:__rand_int__

# Show MinIO stats
echo ""
echo "=========================================="
echo "MinIO Storage Stats"
echo "=========================================="
docker run --rm --network host --entrypoint /bin/sh minio/mc:latest -c "
mc alias set myminio http://localhost:9000 minioadmin minioadmin 2>/dev/null
mc ls myminio/redis-persistent/ 2>/dev/null | head -10
mc du myminio/redis-persistent/ 2>/dev/null || echo 'Unable to get storage stats'
"

# Cleanup
echo ""
echo "Cleaning up..."
docker compose -f docker-compose.persistent.yml down

echo ""
echo "Benchmark complete!"
