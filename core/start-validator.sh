#!/bin/bash

# NumiCoin Validator Script
# Run this script to start a validator node

set -e

echo "🔐 Starting NumiCoin validator node..."

# Check if binary exists
if [ ! -f "target/release/numi-core" ]; then
    echo "Error: numi-core binary not found. Please run setup-miner.sh first."
    exit 1
fi

# Check if validator wallet exists
if [ ! -f "validator-wallet.json" ]; then
    echo "Error: validator-wallet.json not found. Please copy a validator key first."
    exit 1
fi

# Create validator config
cat > validator.toml << EOF
[network]
enabled = true
listen_address = "0.0.0.0"
listen_port = 8335
max_peers = 10
connection_timeout_secs = 10
bootstrap_nodes = [
    "/ip4/127.0.0.1/tcp/8333",
    "/ip4/127.0.0.1/tcp/8334"
]
enable_upnp = false
enable_mdns = true
peer_discovery_interval_secs = 60
max_message_size = 10485760
ban_duration_secs = 300
rate_limit_per_peer = 1000

[mining]
enabled = false
thread_count = 0

[rpc]
enabled = true
bind_address = "127.0.0.1"
port = 8082
max_connections = 100
request_timeout_secs = 30
max_request_size = 1048576
enable_cors = true
allowed_origins = [
    "http://localhost:3000",
    "http://localhost:3001",
    "http://127.0.0.1:3000",
]
rate_limit_requests_per_minute = 1000
rate_limit_burst_size = 100
enable_authentication = false
admin_endpoints_enabled = true

[storage]
data_directory = "./validator-data"
backup_directory = "./validator-backups"
max_database_size_mb = 1024
cache_size_mb = 128
enable_compression = false
enable_encryption = false
auto_backup = false
backup_interval_hours = 24
retention_days = 30
sync_mode = "Fast"

[consensus]
difficulty_adjustment_interval = 20
max_block_size = 524288
max_transactions_per_block = 100
min_transaction_fee = 1000
max_reorg_depth = 10
checkpoint_interval = 50
finality_depth = 100
genesis_supply = 100000000000000000
mining_reward_halving_interval = 1000000
initial_mining_reward = 10000000000

[consensus.target_block_time]
secs = 10
nanos = 0
EOF

# Start validator node
echo "Starting validator node on port 8335..."
./target/release/numi-core --config validator.toml start 