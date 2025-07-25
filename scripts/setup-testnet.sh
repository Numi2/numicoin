#!/bin/bash

# NumiCoin Testnet Setup Script
# This script sets up a complete testnet environment with proper cryptographic security

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Configuration
TESTNET_DIR="./testnet"
DATA_DIR="./testnet-data"
BACKUP_DIR="./testnet-backups"
KEYS_DIR="./testnet-keys"
CONFIG_FILE="testnet.toml"
GENESIS_FILE="testnet-genesis.toml"
LOG_FILE="./testnet-setup.log"

echo -e "${BLUE}🚀 Setting up NumiCoin Testnet...${NC}"

# Create directories
echo -e "${YELLOW}📁 Creating testnet directories...${NC}"
mkdir -p "$TESTNET_DIR"
mkdir -p "$DATA_DIR"
mkdir -p "$BACKUP_DIR"
mkdir -p "$KEYS_DIR"

# Function to log messages
log() {
    echo "$(date '+%Y-%m-%d %H:%M:%S') - $1" | tee -a "$LOG_FILE"
}

log "Starting testnet setup"

# Check if Rust and Cargo are installed
if ! command -v cargo &> /dev/null; then
    echo -e "${RED}❌ Cargo not found. Please install Rust first.${NC}"
    exit 1
fi

# Check if the blockchain binary exists
if [ ! -f "./core/target/release/numi-core" ]; then
    echo -e "${YELLOW}🔨 Building blockchain binary...${NC}"
    cd core
    cargo build --release --features "temporary-pqcrypto"
    cd ..
fi

# Generate testnet keys with proper Dilithium3 cryptography
echo -e "${YELLOW}🔑 Generating testnet cryptographic keys...${NC}"

# Generate validator keys
for i in {1..3}; do
    echo -e "${BLUE}Generating validator key $i...${NC}"
    ./core/target/release/numi-core generate-key \
        --output "$KEYS_DIR/validator_$i.json" \
        --format json
done

# Generate faucet key
echo -e "${BLUE}Generating faucet key...${NC}"
./core/target/release/numi-core generate-key \
    --output "$KEYS_DIR/faucet.json" \
    --format json

# Generate user test keys
for i in {1..5}; do
    echo -e "${BLUE}Generating user key $i...${NC}"
    ./core/target/release/numi-core generate-key \
        --output "$KEYS_DIR/user_$i.json" \
        --format json
done

# Create testnet configuration
echo -e "${YELLOW}⚙️ Creating testnet configuration...${NC}"
cp testnet.toml "$TESTNET_DIR/"

# Initialize testnet blockchain with proper genesis
echo -e "${YELLOW}🌱 Initializing testnet blockchain...${NC}"
./core/target/release/numi-core init \
    --config "$TESTNET_DIR/$CONFIG_FILE" \
    --environment testnet \
    --force

# Create testnet startup script
cat > "$TESTNET_DIR/start-testnet.sh" << 'EOF'
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
./core/target/release/numi-core start \
    --config "$CONFIG_FILE" \
    --environment testnet \
    --enable-mining \
    --verbose
EOF

chmod +x "$TESTNET_DIR/start-testnet.sh"

# Create testnet faucet script
cat > "$TESTNET_DIR/faucet.sh" << 'EOF'
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
EOF

chmod +x "$TESTNET_DIR/faucet.sh"

# Create testnet monitoring script
cat > "$TESTNET_DIR/monitor.sh" << 'EOF'
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
EOF

chmod +x "$TESTNET_DIR/monitor.sh"

# Create testnet cleanup script
cat > "$TESTNET_DIR/cleanup.sh" << 'EOF'
#!/bin/bash

# Testnet cleanup script
echo "🧹 Cleaning up testnet data..."

# Stop any running nodes
pkill -f "numi-core" || true

# Remove data directories
rm -rf ../testnet-data
rm -rf ../testnet-backups

# Recreate directories
mkdir -p ../testnet-data
mkdir -p ../testnet-backups

echo "✅ Testnet cleanup completed!"
EOF

chmod +x "$TESTNET_DIR/cleanup.sh"

# Create testnet documentation
cat > "$TESTNET_DIR/README.md" << 'EOF'
# NumiCoin Testnet

This directory contains the NumiCoin testnet setup with full cryptographic security including Dilithium3 signatures and Argon2id proof-of-work.

## Quick Start

1. **Start the testnet node:**
   ```bash
   ./start-testnet.sh
   ```

2. **Monitor the testnet:**
   ```bash
   ./monitor.sh
   ```

3. **Use the faucet:**
   ```bash
   ./faucet.sh <recipient_address> <amount>
   ```

## Cryptographic Security

This testnet implements the same security standards as mainnet:

- **Dilithium3 Signatures**: Post-quantum secure digital signatures for all transactions
- **Argon2id Proof-of-Work**: Memory-hard proof-of-work algorithm
- **Blake3 Hashing**: Fast cryptographic hashing for block and transaction IDs
- **Kyber KEM**: Post-quantum key encapsulation for secure communication

## Testnet Configuration

- **Block Time**: 15 seconds
- **Difficulty Adjustment**: Every 30 blocks
- **Max Block Size**: 1MB
- **Max Transactions per Block**: 500
- **Min Transaction Fee**: 500 smallest units (0.0000005 NUMI)
- **RPC Port**: 8081
- **P2P Port**: 8334

## Pre-funded Accounts

The testnet includes several pre-funded accounts for testing:

1. **Developer Account**: 100,000 NUMI
2. **Faucet Account**: 500,000 NUMI
3. **Validator Account**: 200,000 NUMI
4. **User Account**: 50,000 NUMI

## Network Features

- **P2P Networking**: libp2p-based peer-to-peer communication
- **RPC API**: RESTful API for blockchain interaction
- **Mempool**: Transaction pool with fee-based prioritization
- **Mining**: CPU-based mining with configurable threads


## Security Features

- **Rate Limiting**: Protection against spam and DoS attacks
- **IP Blocking**: Automatic blocking of malicious peers
- **Transaction Validation**: Comprehensive transaction verification
- **Block Validation**: Full block structure and signature verification
- **Replay Protection**: Nonce-based transaction replay protection

## Monitoring and Maintenance

- **Automatic Backups**: Every 12 hours
- **Log Rotation**: Automatic log management
- **Health Checks**: Built-in node health monitoring
- **Performance Metrics**: Real-time performance tracking

## Troubleshooting

If you encounter issues:

1. Check the logs in `../testnet-data/logs/`
2. Verify the node is not already running
3. Ensure ports 8081 and 8334 are available
4. Check system resources (CPU, memory, disk)

## Development

For development and testing:

```bash
# Generate new keys
./core/target/release/numi-core generate-key --output new_key.json

# Submit a transaction
./core/target/release/numi-core submit --from-key new_key.json --to <recipient> --amount <amount>

# Check balance
./core/target/release/numi-core balance --address <address>
```
EOF

# Create testnet health check script
cat > "$TESTNET_DIR/health-check.sh" << 'EOF'
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
EOF

chmod +x "$TESTNET_DIR/health-check.sh"

# Create testnet backup script
cat > "$TESTNET_DIR/backup.sh" << 'EOF'
#!/bin/bash

# Testnet backup script
BACKUP_DIR="../testnet-backups"
DATA_DIR="../testnet-data"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
BACKUP_NAME="testnet_backup_$TIMESTAMP"

echo "💾 Creating testnet backup: $BACKUP_NAME"

# Create backup directory
mkdir -p "$BACKUP_DIR/$BACKUP_NAME"

# Stop the node gracefully
echo "🛑 Stopping node for backup..."
pkill -TERM -f "numi-core" || true
sleep 5

# Backup data directory
echo "📁 Backing up blockchain data..."
cp -r "$DATA_DIR" "$BACKUP_DIR/$BACKUP_NAME/"

# Backup configuration
echo "⚙️ Backing up configuration..."
cp testnet.toml "$BACKUP_DIR/$BACKUP_NAME/"

# Create backup manifest
cat > "$BACKUP_DIR/$BACKUP_NAME/manifest.json" << MANIFEST
{
    "backup_name": "$BACKUP_NAME",
    "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
    "data_directory": "$DATA_DIR",
    "config_file": "testnet.toml",
    "version": "1.0.0"
}
MANIFEST

# Compress backup
echo "🗜️ Compressing backup..."
cd "$BACKUP_DIR"
tar -czf "${BACKUP_NAME}.tar.gz" "$BACKUP_NAME"
rm -rf "$BACKUP_NAME"

echo "✅ Backup completed: ${BACKUP_NAME}.tar.gz"

# Restart the node
echo "🚀 Restarting node..."
cd - > /dev/null
./start-testnet.sh &
EOF

chmod +x "$TESTNET_DIR/backup.sh"

# Create testnet restore script
cat > "$TESTNET_DIR/restore.sh" << 'EOF'
#!/bin/bash

# Testnet restore script
if [ $# -ne 1 ]; then
    echo "Usage: $0 <backup_file.tar.gz>"
    exit 1
fi

BACKUP_FILE="$1"
BACKUP_DIR="../testnet-backups"
DATA_DIR="../testnet-data"

if [ ! -f "$BACKUP_FILE" ]; then
    echo "❌ Backup file not found: $BACKUP_FILE"
    exit 1
fi

echo "🔄 Restoring from backup: $BACKUP_FILE"

# Stop the node
echo "🛑 Stopping node..."
pkill -f "numi-core" || true
sleep 5

# Backup current data
echo "💾 Backing up current data..."
if [ -d "$DATA_DIR" ]; then
    mv "$DATA_DIR" "${DATA_DIR}_backup_$(date +%Y%m%d_%H%M%S)"
fi

# Extract backup
echo "📁 Extracting backup..."
tar -xzf "$BACKUP_FILE" -C "$BACKUP_DIR"

# Find the extracted directory
EXTRACTED_DIR=$(find "$BACKUP_DIR" -maxdepth 1 -type d -name "testnet_backup_*" | head -1)

if [ -z "$EXTRACTED_DIR" ]; then
    echo "❌ Failed to extract backup"
    exit 1
fi

# Restore data
echo "🔄 Restoring data..."
cp -r "$EXTRACTED_DIR/testnet-data" "$DATA_DIR"

# Restore configuration if needed
if [ -f "$EXTRACTED_DIR/testnet.toml" ]; then
    cp "$EXTRACTED_DIR/testnet.toml" .
fi

# Clean up
rm -rf "$EXTRACTED_DIR"

echo "✅ Restore completed!"

# Restart the node
echo "🚀 Restarting node..."
./start-testnet.sh &
EOF

chmod +x "$TESTNET_DIR/restore.sh"

# Create testnet network configuration
cat > "$TESTNET_DIR/network-config.json" << 'EOF'
{
    "testnet": {
        "name": "numi-testnet",
        "version": "1.0.0",
        "chain_id": "testnet-2024",
        "ports": {
            "p2p": 8334,
            "rpc": 8081
        },
        "bootstrap_nodes": [
            "/ip4/127.0.0.1/tcp/8334",
            "/ip4/127.0.0.1/tcp/8335",
            "/ip4/127.0.0.1/tcp/8336"
        ],
        "consensus": {
            "algorithm": "proof-of-work",
            "pow_algorithm": "argon2id",
            "signature_algorithm": "dilithium3",
            "kem_algorithm": "kyber",
            "block_time": 15,
            "difficulty_adjustment_interval": 30
        },
        "cryptography": {
            "hash_function": "blake3",
            "signature_scheme": "dilithium3",
            "kem_scheme": "kyber",
            "pow_function": "argon2id"
        }
    }
}
EOF

echo -e "${GREEN}✅ Testnet setup completed!${NC}"
echo -e "${BLUE}📁 Testnet directory: $TESTNET_DIR${NC}"
echo -e "${BLUE}🔑 Keys directory: $KEYS_DIR${NC}"
echo -e "${BLUE}💾 Data directory: $DATA_DIR${NC}"

echo -e "\n${YELLOW}🚀 To start the testnet:${NC}"
echo -e "cd $TESTNET_DIR"
echo -e "./start-testnet.sh"

echo -e "\n${YELLOW}📊 To monitor the testnet:${NC}"
echo -e "./monitor.sh"

echo -e "\n${YELLOW}🚰 To use the faucet:${NC}"
echo -e "./faucet.sh <recipient_address> <amount>"

echo -e "\n${YELLOW}📖 For more information:${NC}"
echo -e "cat README.md"

log "Testnet setup completed successfully" 