#!/bin/bash

# Testnet faucet script
set -e

FAUCET_KEY="$TESTNET_DIR/../testnet-keys/faucet.json"
RPC_URL="http://localhost:8081"

if [ $# -ne 2 ]; then
    echo "Usage: $0 <recipient_address> <amount_in_numi>"
    echo "Example: $0 000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f 100"
    exit 1
fi

RECIPIENT=$1
AMOUNT=$2
AMOUNT_SMALLEST_UNITS=$((AMOUNT * 1000000000))  # Convert to smallest units

echo "🚰 Sending $AMOUNT NUMI to $RECIPIENT..."

# Submit transaction using RPC
curl -X POST "$RPC_URL/transaction" \
    -H "Content-Type: application/json" \
    -d "{
        \"from\": \"$(cat $FAUCET_KEY | jq -r '.public_key')\",
        \"to\": \"$RECIPIENT\",
        \"amount\": $AMOUNT_SMALLEST_UNITS,
        \"nonce\": 0,
        \"signature\": \"$(./core/target/release/numi-core sign-transaction --key $FAUCET_KEY --to $RECIPIENT --amount $AMOUNT_SMALLEST_UNITS)\"
    }"

echo "✅ Faucet transaction submitted!"
