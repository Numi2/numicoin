#!/bin/bash

# Testnet monitoring script
RPC_URL="http://localhost:8081"

echo "📊 NumiCoin Testnet Status"
echo "=========================="

# Get blockchain status
echo "🔗 Blockchain Status:"
curl -s "$RPC_URL/status" | jq -r '.data | "Height: \(.total_blocks)\nBest Block: \(.best_block_hash)\nDifficulty: \(.current_difficulty)\nMempool: \(.mempool_transactions) transactions"'

echo -e "\n💰 Network Statistics:"
curl -s "$RPC_URL/stats" | jq -r '.data | "Total Supply: \(.total_supply) NUMI\nActive Peers: \(.network_peers)\nIs Syncing: \(.is_syncing)"'

echo -e "\n⏰ Last updated: $(date)"
