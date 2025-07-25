#!/bin/bash

# Testnet health check script
RPC_URL="http://localhost:8081"
HEALTH_FILE="../testnet-data/health.json"

# Check if node is responding
if curl -s "$RPC_URL/status" > /dev/null 2>&1; then
    echo "✅ Node is responding"
    
    # Get health metrics
    STATUS=$(curl -s "$RPC_URL/status")
    HEIGHT=$(echo "$STATUS" | jq -r '.data.total_blocks // 0')
    SYNCING=$(echo "$STATUS" | jq -r '.data.is_syncing // false')
    
    # Save health data
    echo "{
        \"timestamp\": \"$(date -u +%Y-%m-%dT%H:%M:%SZ)\",
        \"height\": $HEIGHT,
        \"syncing\": $SYNCING,
        \"healthy\": true
    }" > "$HEALTH_FILE"
    
    echo "📊 Current height: $HEIGHT"
    echo "🔄 Syncing: $SYNCING"
else
    echo "❌ Node is not responding"
    echo "{
        \"timestamp\": \"$(date -u +%Y-%m-%dT%H:%M:%SZ)\",
        \"healthy\": false,
        \"error\": \"Node not responding\"
    }" > "$HEALTH_FILE"
    exit 1
fi
