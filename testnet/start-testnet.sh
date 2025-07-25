#!/bin/bash

# Testnet startup script
set -e

TESTNET_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DATA_DIR="$TESTNET_DIR/../testnet-data"
CONFIG_FILE="$TESTNET_DIR/testnet.toml"

echo "🚀 Starting NumiCoin Testnet Node..."

# Check if node is already running
if [ -f "$DATA_DIR/.lock" ]; then
    echo "⚠️ Node appears to be already running. Use --force to override."
    exit 1
fi

# Start the testnet node
../core/target/release/numi-core \
    --config "$CONFIG_FILE" \
    --environment testnet \
    start --enable-mining
