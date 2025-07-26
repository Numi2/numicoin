#!/bin/bash

# Add test transactions to the mempool for mining
echo "💰 Adding test transactions to mempool..."

# Wait for RPC to be available
echo "Waiting for RPC server to be ready..."
for i in {1..30}; do
    if curl -s http://127.0.0.1:8082/status >/dev/null 2>&1; then
        echo "✅ RPC server is ready!"
        break
    fi
    echo "⏳ Waiting for RPC... ($i/30)"
    sleep 2
done

# Add some test transactions
echo "📝 Adding test transactions..."

# Transaction 1: Simple transfer
curl -X POST http://127.0.0.1:8082/transaction \
  -H "Content-Type: application/json" \
  -d '{
    "from": "test_wallet_1",
    "to": "test_wallet_2", 
    "amount": 1000000,
    "fee": 100
  }' 2>/dev/null || echo "⚠️ Failed to add transaction 1"

# Transaction 2: Another transfer
curl -X POST http://127.0.0.1:8082/transaction \
  -H "Content-Type: application/json" \
  -d '{
    "from": "test_wallet_2",
    "to": "test_wallet_3",
    "amount": 500000,
    "fee": 50
  }' 2>/dev/null || echo "⚠️ Failed to add transaction 2"

# Transaction 3: Small transfer
curl -X POST http://127.0.0.1:8082/transaction \
  -H "Content-Type: application/json" \
  -d '{
    "from": "test_wallet_3", 
    "to": "test_wallet_1",
    "amount": 250000,
    "fee": 25
  }' 2>/dev/null || echo "⚠️ Failed to add transaction 3"

echo "✅ Test transactions added to mempool!"
echo "🔍 Check mempool status: curl http://127.0.0.1:8082/status" 