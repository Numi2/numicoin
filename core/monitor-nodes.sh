#!/bin/bash

# Monitor NumiCoin nodes and their connections
echo "🔍 Monitoring NumiCoin Nodes..."
echo "=================================="

# Check if nodes are running
echo "📊 Node Status:"
ps aux | grep numi-core | grep -v grep | while read line; do
    echo "  ✅ $line"
done

echo ""
echo "🌐 Network Ports:"
lsof -i :8333 -i :8334 -i :8081 -i :8082 2>/dev/null | grep LISTEN | while read line; do
    echo "  🔗 $line"
done

echo ""
echo "📈 Node Statistics:"
echo "  Core Node (Port 8333, RPC 8082):"
curl -s http://127.0.0.1:8082/status 2>/dev/null | head -5 || echo "    ⚠️ RPC not responding"

echo ""
echo "  Testnet Node (Port 8334, RPC 8081):"
curl -s http://127.0.0.1:8081/status 2>/dev/null | head -5 || echo "    ⚠️ RPC not responding"

echo ""
echo "🔗 Bootstrap Configuration:"
echo "  Core node bootstrap nodes:"
grep -A 5 "bootstrap_nodes" numi.toml | grep -v "bootstrap_nodes" | sed 's/^/    /'

echo ""
echo "  Testnet node bootstrap nodes:"
grep -A 5 "bootstrap_nodes" ../testnet/testnet.toml | grep -v "bootstrap_nodes" | sed 's/^/    /' 