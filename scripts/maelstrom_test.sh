#!/bin/bash
set -e

# Get the project root directory (parent of scripts/)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

MAELSTROM_DIR="$PROJECT_ROOT/maelstrom/maelstrom"
BIN="$PROJECT_ROOT/target/release/maelstrom-kv"
BIN_REPLICATED="$PROJECT_ROOT/target/release/maelstrom-kv-replicated"

echo "Building maelstrom binaries..."
cd "$PROJECT_ROOT"
cargo build --bin maelstrom-kv --release
cargo build --bin maelstrom-kv-replicated --release

if [ ! -f "$MAELSTROM_DIR/maelstrom" ]; then
    echo "Maelstrom not found at $MAELSTROM_DIR"
    echo "Please ensure maelstrom is installed in the maelstrom/ directory"
    exit 1
fi

echo ""
echo "============================================"
echo "  Running Maelstrom Linearizability Tests"
echo "============================================"
echo ""

echo "Test 1: Single-node linearizability (should pass)"
echo "---------------------------------------------------"
cd "$MAELSTROM_DIR"
./maelstrom test -w lin-kv \
    --bin "$BIN" \
    --node-count 1 \
    --time-limit 10 \
    --rate 10 \
    --concurrency 2

echo ""
echo "Test 2: Single-node with higher load (should pass)"
echo "---------------------------------------------------"
./maelstrom test -w lin-kv \
    --bin "$BIN" \
    --node-count 1 \
    --time-limit 15 \
    --rate 50 \
    --concurrency 4

echo ""
echo "Test 3: Multi-node eventual consistency (replicated)"
echo "-----------------------------------------------------"
echo "Note: This tests eventual consistency, not linearizability."
echo "The test may report consistency violations - this is expected."
./maelstrom test -w lin-kv \
    --bin "$BIN_REPLICATED" \
    --node-count 3 \
    --time-limit 20 \
    --rate 10 \
    --concurrency 2 || echo "Multi-node test completed (expected consistency violations for eventual consistency)"

echo ""
echo "============================================"
echo "  All tests completed!"
echo "============================================"
