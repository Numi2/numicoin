#!/bin/bash

# NumiCoin Mining Script
# Run this script to start mining

set -e

echo "⛏️ Starting NumiCoin mining..."

# Check if binary exists
if [ ! -f "target/release/numi-core" ]; then
    echo "Error: numi-core binary not found. Please run setup-miner.sh first."
    exit 1
fi

# Check if wallet exists
if [ ! -f "miner-wallet.json" ]; then
    echo "Error: miner-wallet.json not found. Please run setup-miner.sh first."
    exit 1
fi

# Get number of CPU cores (compatible with both Linux and macOS)
if command -v nproc >/dev/null 2>&1; then
    # Linux
    CPU_CORES=$(nproc)
else
    # macOS
    CPU_CORES=$(sysctl -n hw.ncpu)
fi
RECOMMENDED_THREADS=$((CPU_CORES / 2))

echo "Detected $CPU_CORES CPU cores"
echo "Recommended mining threads: $RECOMMENDED_THREADS"

# Start mining
echo "Starting mining with $RECOMMENDED_THREADS threads..."
./target/release/numi-core start --enable-mining --mining-threads $RECOMMENDED_THREADS
