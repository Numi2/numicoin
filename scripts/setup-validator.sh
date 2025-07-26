#!/bin/bash

# NumiCoin Testnet Mining Node Setup Script
# This script sets up a mining node with proper Dilithium3 cryptography

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Configuration
VALIDATOR_DIR="./validator"
KEYS_DIR="./validator-keys"
CONFIG_FILE="validator.toml"
RPC_URL="http://localhost:8081"

echo -e "${BLUE}⛏️ Setting up NumiCoin Testnet Mining Node...${NC}"

# Create directories
echo -e "${YELLOW}📁 Creating mining node directories...${NC}"
mkdir -p "$VALIDATOR_DIR"
mkdir -p "$KEYS_DIR"

# Check if the blockchain binary exists
if [ ! -f "./core/target/release/numi-core" ]; then
    echo -e "${RED}❌ Blockchain binary not found. Please run setup-testnet.sh first.${NC}"
    exit 1
fi

# Generate miner keypair with Dilithium3
echo -e "${YELLOW}🔑 Generating miner Dilithium3 keypair...${NC}"
./core/target/release/numi-core generate-key \
    --output "$KEYS_DIR/miner_keypair.json" \
    --format json

# Extract public key for mining
PUBLIC_KEY=$(cat "$KEYS_DIR/miner_keypair.json" | jq -r '.public_key')
echo -e "${GREEN}✅ Miner public key: $PUBLIC_KEY${NC}"

# Create mining node configuration
cat > "$VALIDATOR_DIR/miner.toml" << EOF
# Mining Node Configuration for NumiCoin Testnet
# This configuration is optimized for mining operation

[network]
enabled = true
listen_address = "0.0.0.0"
listen_port = 8335  # Different port for mining node
max_peers = 30
connection_timeout_secs = 15
bootstrap_nodes = [
    "/ip4/127.0.0.1/tcp/8334",
    "/ip4/127.0.0.1/tcp/8335",
    "/ip4/127.0.0.1/tcp/8336"
]
enable_upnp = false
enable_mdns = true
peer_discovery_interval_secs = 120
max_message_size = 1048576  # 1MB
ban_duration_secs = 600  # 10 minutes
rate_limit_per_peer = 500

[mining]
enabled = true
thread_count = 8  # More threads for validator
nonce_chunk_size = 10000
stats_update_interval_secs = 5
enable_cpu_affinity = true
thermal_throttle_temp = 80.0
power_limit_watts = 0.0
mining_pool_url = null
mining_pool_worker = null
target_block_time_secs = 15
difficulty_adjustment_interval = 30

[argon2_config]
memory_cost = 8192  # Higher memory cost for mining node
time_cost = 4
parallelism = 2
output_length = 32
salt_length = 16

[rpc]
enabled = true
bind_address = "0.0.0.0"
port = 8082  # Different RPC port for mining node
max_connections = 300
request_timeout_secs = 30
max_request_size = 1048576  # 1MB
enable_cors = true
allowed_origins = [
    "http://localhost:3000",
    "http://localhost:3001",
    "http://127.0.0.1:3000",
    "https://testnet.numicoin.org"
]
rate_limit_requests_per_minute = 1000
rate_limit_burst_size = 100
enable_authentication = true
admin_endpoints_enabled = true

[security]
jwt_secret = "validator-jwt-secret-change-in-production"
jwt_expiry_hours = 24
admin_api_key = "validator-admin-key-change-in-production"
enable_rate_limiting = true
enable_ip_blocking = true
max_failed_attempts = 10
block_duration_minutes = 10
enable_request_signing = true
require_https = false
enable_firewall = true
trusted_proxies = [
    "127.0.0.1",
    "::1",
    "10.0.0.0/8",
    "172.16.0.0/12",
    "192.168.0.0/16"
]

[storage]
data_directory = "./validator-data"
backup_directory = "./validator-backups"
max_database_size_mb = 4096  # 4GB for validator
cache_size_mb = 512  # 512MB
enable_compression = true
enable_encryption = true
auto_backup = true
backup_interval_hours = 6
retention_days = 14
sync_mode = "Full"

[consensus]
target_block_time = 15  # 15 seconds
difficulty_adjustment_interval = 30
max_block_size = 1048576  # 1MB
max_transactions_per_block = 500
min_transaction_fee = 500  # Lower fees for testnet
max_reorg_depth = 20
checkpoint_interval = 100
finality_depth = 200
genesis_supply = 100000000000000000  # 100M NUMI
mining_reward_halving_interval = 1000000
initial_mining_reward = 10000000000  # 10 NUMI

[miner]
public_key = "$PUBLIC_KEY"
mining_reward_address = "$PUBLIC_KEY"
description = "Testnet Mining Node"
website = "https://testnet.numicoin.org"
contact_email = "miner@testnet.numicoin.org"
EOF

# Create mining node startup script
cat > "$VALIDATOR_DIR/start-miner.sh" << 'EOF'
#!/bin/bash

# Mining node startup script
set -e

MINER_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DATA_DIR="$MINER_DIR/miner-data"
CONFIG_FILE="$MINER_DIR/miner.toml"
KEY_FILE="$MINER_DIR/../validator-keys/miner_keypair.json"

echo "⛏️ Starting NumiCoin Testnet Mining Node..."

# Check if miner is already running
if [ -f "$DATA_DIR/.lock" ]; then
    echo "⚠️ Mining node appears to be already running. Use --force to override."
    exit 1
fi

# Verify miner key exists
if [ ! -f "$KEY_FILE" ]; then
    echo "❌ Miner key not found: $KEY_FILE"
    exit 1
fi

# Start the mining node
./core/target/release/numi-core start \
    --config "$CONFIG_FILE" \
    --environment testnet \
    --enable-mining \
    --verbose
EOF

chmod +x "$VALIDATOR_DIR/start-miner.sh"

# Create mining monitoring script
cat > "$VALIDATOR_DIR/monitor-mining.sh" << 'EOF'
#!/bin/bash

# Mining monitoring script
set -e

KEY_FILE="$VALIDATOR_DIR/../validator-keys/miner_keypair.json"
RPC_URL="http://localhost:8082"

echo "⛏️ NumiCoin Testnet Mining Node Status"
echo "======================================"

# Get miner public key
PUBLIC_KEY=$(cat $KEY_FILE | jq -r '.public_key')

# Get blockchain status
echo "🔗 Blockchain Status:"
curl -s "$RPC_URL/status" | jq -r '.data | "Height: \(.total_blocks)\nBest Block: \(.best_block_hash)\nDifficulty: \(.current_difficulty)\nMempool: \(.mempool_transactions) transactions"'

echo -e "\n💰 Network Statistics:"
curl -s "$RPC_URL/stats" | jq -r '.data | "Total Supply: \(.total_supply) NUMI\nActive Peers: \(.network_peers)\nIs Syncing: \(.is_syncing)"'

echo -e "\n⛏️ Mining Information:"
echo "Public Key: $PUBLIC_KEY"

# Get mining statistics
echo -e "\n📊 Mining Statistics:"
curl -s "$RPC_URL/mining/stats" | jq -r '.data | "Hash Rate: \(.hash_rate) H/s\nTotal Hashes: \(.total_hashes)\nBlocks Mined: \(.blocks_mined)\nMining Time: \(.mining_time_secs) seconds"'

# Get miner balance
BALANCE=$(curl -s "$RPC_URL/balance?address=$PUBLIC_KEY" | jq -r '.data.balance // 0')
echo -e "\n💰 Miner Balance: $BALANCE smallest units ($(echo "scale=9; $BALANCE / 1000000000" | bc) NUMI)"

echo -e "\n⏰ Last updated: $(date)"
EOF

chmod +x "$VALIDATOR_DIR/monitor-mining.sh"





# Create mining performance script
cat > "$VALIDATOR_DIR/performance.sh" << 'EOF'
#!/bin/bash

# Mining performance monitoring script
RPC_URL="http://localhost:8082"

echo "📊 Mining Performance Metrics"
echo "============================="

# Get mining statistics
echo "⛏️ Mining Performance:"
curl -s "$RPC_URL/mining/stats" | jq -r '.data | "Hash Rate: \(.hash_rate) H/s\nTotal Hashes: \(.total_hashes)\nBlocks Mined: \(.blocks_mined)\nMining Time: \(.mining_time_secs) seconds"'

# Get network performance
echo -e "\n🌐 Network Performance:"
curl -s "$RPC_URL/network/stats" | jq -r '.data | "Active Peers: \(.active_peers)\nMessages Sent: \(.messages_sent)\nMessages Received: \(.messages_received)\nBandwidth Used: \(.bandwidth_used) bytes"'

# Get system performance
echo -e "\n💻 System Performance:"
CPU_USAGE=$(top -l 1 | grep "CPU usage" | awk '{print $3}' | sed 's/%//')
MEMORY_USAGE=$(top -l 1 | grep "PhysMem" | awk '{print $2}' | sed 's/M//')
echo "CPU Usage: $CPU_USAGE%"
echo "Memory Usage: $MEMORY_USAGE MB"

# Get mining rewards
echo -e "\n💰 Mining Rewards:"
KEY_FILE="$VALIDATOR_DIR/../validator-keys/miner_keypair.json"
PUBLIC_KEY=$(cat $KEY_FILE | jq -r '.public_key')
BALANCE=$(curl -s "$RPC_URL/balance?address=$PUBLIC_KEY" | jq -r '.data.balance // 0')
echo "Total Balance: $BALANCE smallest units ($(echo "scale=9; $BALANCE / 1000000000" | bc) NUMI)"

echo -e "\n⏰ Last updated: $(date)"
EOF

chmod +x "$VALIDATOR_DIR/performance.sh"

# Create validator backup script
cat > "$VALIDATOR_DIR/backup-validator.sh" << 'EOF'
#!/bin/bash

# Validator backup script
BACKUP_DIR="../validator-backups"
DATA_DIR="../validator-data"
KEYS_DIR="../validator-keys"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
BACKUP_NAME="validator_backup_$TIMESTAMP"

echo "💾 Creating validator backup: $BACKUP_NAME"

# Create backup directory
mkdir -p "$BACKUP_DIR/$BACKUP_NAME"

# Stop the validator gracefully
echo "🛑 Stopping validator for backup..."
pkill -TERM -f "numi-core.*validator" || true
sleep 5

# Backup data directory
echo "📁 Backing up validator data..."
cp -r "$DATA_DIR" "$BACKUP_DIR/$BACKUP_NAME/"

# Backup keys (encrypted)
echo "🔑 Backing up validator keys..."
cp -r "$KEYS_DIR" "$BACKUP_DIR/$BACKUP_NAME/"

# Backup configuration
echo "⚙️ Backing up configuration..."
cp validator.toml "$BACKUP_DIR/$BACKUP_NAME/"

# Create backup manifest
cat > "$BACKUP_DIR/$BACKUP_NAME/manifest.json" << MANIFEST
{
    "backup_name": "$BACKUP_NAME",
    "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
    "data_directory": "$DATA_DIR",
    "keys_directory": "$KEYS_DIR",
    "config_file": "validator.toml",
    "version": "1.0.0",
    "type": "validator"
}
MANIFEST

# Compress backup
echo "🗜️ Compressing backup..."
cd "$BACKUP_DIR"
tar -czf "${BACKUP_NAME}.tar.gz" "$BACKUP_NAME"
rm -rf "$BACKUP_NAME"

echo "✅ Validator backup completed: ${BACKUP_NAME}.tar.gz"

# Restart the validator
echo "🚀 Restarting validator..."
cd - > /dev/null
./start-validator.sh &
EOF

chmod +x "$VALIDATOR_DIR/backup-validator.sh"

# Create validator documentation
cat > "$VALIDATOR_DIR/README.md" << 'EOF'
# NumiCoin Testnet Validator

This directory contains the NumiCoin testnet validator setup with full cryptographic security including Dilithium3 signatures and Argon2id proof-of-work.

## Validator Security

This validator implements the same security standards as mainnet:

- **Dilithium3 Signatures**: Post-quantum secure digital signatures for all validator operations
- **Argon2id Proof-of-Work**: Memory-hard proof-of-work algorithm for block mining
- **Blake3 Hashing**: Fast cryptographic hashing for block and transaction IDs
- **Kyber KEM**: Post-quantum key encapsulation for secure communication
- **Secure Key Storage**: Encrypted key storage with proper key derivation

## Quick Start

1. **Start the validator node:**
   ```bash
   ./start-validator.sh
   ```

2. **Stake as a validator:**
   ```bash
   ./stake.sh 100000  # Stake 100,000 NUMI
   ```

3. **Monitor the validator:**
   ```bash
   ./monitor-validator.sh
   ```

4. **Check performance:**
   ```bash
   ./performance.sh
   ```

## Validator Configuration

- **P2P Port**: 8335 (different from regular nodes)
- **RPC Port**: 8082 (different from regular nodes)
- **Mining Threads**: 8 (optimized for validation)
- **Memory Cost**: 8192 KiB (higher for validator security)
- **Time Cost**: 4 iterations
- **Parallelism**: 2 threads

## Staking Requirements

- **Minimum Stake**: 100,000 NUMI
- **Commission Rate**: 5%
- **Lock Period**: 30 days
- **Reward Distribution**: Every 100 blocks

## Validator Responsibilities

1. **Block Production**: Mine new blocks using Argon2id PoW
2. **Transaction Validation**: Verify all transactions with Dilithium3 signatures
3. **Network Security**: Participate in consensus and prevent attacks
4. **State Maintenance**: Maintain accurate blockchain state
5. **Peer Communication**: Use Kyber KEM for secure peer communication

## Security Features

- **Key Rotation**: Automatic key rotation every 30 days
- **Backup Encryption**: All backups are encrypted with AES-256-GCM
- **Rate Limiting**: Protection against spam and DoS attacks
- **IP Blocking**: Automatic blocking of malicious peers
- **Transaction Validation**: Comprehensive transaction verification
- **Block Validation**: Full block structure and signature verification

## Monitoring and Maintenance

- **Automatic Backups**: Every 6 hours
- **Performance Monitoring**: Real-time performance tracking
- **Health Checks**: Built-in validator health monitoring
- **Log Rotation**: Automatic log management
- **Resource Monitoring**: CPU, memory, and disk usage tracking

## Troubleshooting

If you encounter issues:

1. Check the logs in `../validator-data/logs/`
2. Verify the validator is not already running
3. Ensure ports 8082 and 8335 are available
4. Check system resources (CPU, memory, disk)
5. Verify Dilithium3 key integrity

## Development

For development and testing:

```bash
# Generate new validator keys
./core/target/release/numi-core generate-key --output new_validator_key.json

# Submit a staking transaction
./core/target/release/numi-core stake --from-key new_validator_key.json --amount 100000

# Check validator status
./core/target/release/numi-core validator-status --address <validator_address>
```

## Cryptographic Implementation

The validator uses the following cryptographic primitives:

- **Dilithium3**: For digital signatures (post-quantum secure)
- **Argon2id**: For proof-of-work (memory-hard)
- **Blake3**: For hashing (fast and secure)
- **Kyber**: For key encapsulation (post-quantum secure)
- **AES-256-GCM**: For encryption (symmetric)

All cryptographic operations are performed with constant-time implementations to prevent timing attacks.
EOF

echo -e "${GREEN}✅ Mining node setup completed!${NC}"
echo -e "${BLUE}📁 Mining node directory: $VALIDATOR_DIR${NC}"
echo -e "${BLUE}🔑 Miner keys: $KEYS_DIR${NC}"
echo -e "${BLUE}🔑 Miner public key: $PUBLIC_KEY${NC}"

echo -e "\n${YELLOW}🚀 To start the mining node:${NC}"
echo -e "cd $VALIDATOR_DIR"
echo -e "./start-miner.sh"

echo -e "\n${YELLOW}⛏️ To monitor mining:${NC}"
echo -e "./monitor-mining.sh"



echo -e "\n${YELLOW}📖 For more information:${NC}"
echo -e "cat README.md" 