#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" &>/dev/null && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." &>/dev/null && pwd)"
CORE_DIR="$ROOT_DIR/core"
FAUCET_KEY="$ROOT_DIR/testnet-keys/faucet.json"

usage() {
  echo "Usage: $0 <recipient_address> <amount_in_numi>"
  exit 1
}

if [ "$#" -ne 2 ]; then
  usage
fi

RECIPIENT="$1"
AMOUNT="$2"
AMOUNT_SMALLEST_UNITS=$((AMOUNT * 100))  # Convert to smallest units (1 NUMI = 100 nano)

echo "🔑 Ensuring faucet key has secret..."
if ! jq -e '.secret_key' "$FAUCET_KEY" >/dev/null 2>&1; then
  echo "📄 Generating new faucet keypair..."
  (cd "$CORE_DIR" && cargo run -- -e testnet generate-key --output "$FAUCET_KEY" --format json)
fi

echo "🚰 Sending $AMOUNT NUMI to $RECIPIENT..."

# Hex-encode public_key
PUBHEX=""
for byte in $(jq -r '.public_key | .[]' "$FAUCET_KEY"); do
  printf -v h "%02x" "$byte"
  PUBHEX+="$h"
done

# Determine faucet address (hex)
FAUCET_ADDR="$PUBHEX"
# Fetch current faucet nonce via RPC
FAUCET_NONCE=$(curl -s http://localhost:8081/balance/$FAUCET_ADDR | jq -r '.data.nonce')
echo "🚰 Sending $AMOUNT NUMI to $RECIPIENT (nonce: $FAUCET_NONCE)..."
# Sign the transaction
SIGNATURE=$(cd "$CORE_DIR" && cargo run -- -e testnet sign-transaction \
  --key "$FAUCET_KEY" --to "$RECIPIENT" --amount "$AMOUNT_SMALLEST_UNITS" --nonce "$FAUCET_NONCE")
# Build JSON payload
PAYLOAD=$(printf '{"from":"%s","to":"%s","amount":%d,"nonce":%d,"signature":"%s"}' \
  "$FAUCET_ADDR" "$RECIPIENT" "$AMOUNT_SMALLEST_UNITS" "$FAUCET_NONCE" "$SIGNATURE")
# Perform HTTP POST to transaction endpoint
RESPONSE=$(curl -s -X POST "http://localhost:8081/transaction" \
  -H "Content-Type: application/json" \
  -d "$PAYLOAD")
echo "✅ Faucet response: $RESPONSE"
