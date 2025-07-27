#!/bin/bash

echo "🚀 Starting NumiCoin Peer Node..."
echo "=================================="

# Check if numi-core binary exists
if [ ! -f "../core/target/release/numi-core" ]; then
    echo "❌ numi-core binary not found. Please build it first:"
    echo "   cd ../core && cargo build --release"
    exit 1
fi

# Create data directory if it doesn't exist
mkdir -p ./peer-data

# Start the peer node
echo "📡 Peer Node Configuration:"
echo "   - Network Port: 8334"
echo "   - RPC Port: 8083"
echo "   - Data Directory: ./peer-data"
echo "   - Mining Threads: 4"
echo "   - Bootstrap Node: 127.0.0.1:8333"
echo ""

# Start the peer node in the background
../core/target/release/numi-core start --enable-mining --data-dir ./peer-data > peer.log 2>&1 &

PEER_PID=$!
echo "✅ Peer node started with PID: $PEER_PID"
echo "📋 Logs will be written to: peer.log"
echo ""
echo "To monitor the peer node:"
echo "   tail -f peer.log"
echo ""
echo "To stop the peer node:"
echo "   kill $PEER_PID"
