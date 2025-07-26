#!/bin/bash

# NumiCoin Testnet Miner Setup Script
# This script automates the setup process for new miners

set -e

echo "🚀 Welcome to NumiCoin Testnet Miner Setup!"
echo "=============================================="

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Function to print colored output
print_status() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

print_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

print_header() {
    echo -e "${BLUE}[SETUP]${NC} $1"
}

# Check if Rust is installed
check_rust() {
    print_header "Checking Rust installation..."
    if command -v rustc &> /dev/null; then
        RUST_VERSION=$(rustc --version | cut -d' ' -f2)
        print_status "Rust is installed: $RUST_VERSION"
    else
        print_warning "Rust is not installed. Installing now..."
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        source ~/.cargo/env
        print_status "Rust installed successfully"
    fi
}

# Build the project
build_project() {
    print_header "Building NumiCoin core..."
    
    if [ ! -f "Cargo.toml" ]; then
        print_error "Cargo.toml not found. Please run this script from the core directory."
        exit 1
    fi
    
    print_status "Building release version (this may take a few minutes)..."
    cargo build --release
    
    if [ -f "target/release/numi-core" ]; then
        print_status "Build completed successfully!"
    else
        print_error "Build failed. Please check the error messages above."
        exit 1
    fi
}

# Initialize blockchain
init_blockchain() {
    print_header "Initializing blockchain..."
    
    if [ -d "dev-data" ]; then
        print_warning "Blockchain data already exists. Reinitializing..."
        rm -rf dev-data
    fi
    
    ./target/release/numi-core init --force
    print_status "Blockchain initialized successfully!"
}

# Generate wallet
generate_wallet() {
    print_header "Generating wallet..."
    
    if [ -f "miner-wallet.json" ]; then
        print_warning "Wallet already exists. Creating backup..."
        cp miner-wallet.json miner-wallet-backup.json
    fi
    
    ./target/release/numi-core generate-key --output miner-wallet.json
    print_status "Wallet generated: miner-wallet.json"
    
    # Extract and display wallet address
    if command -v python3 &> /dev/null; then
        # Use Python (more reliable than jq)
        WALLET_ADDRESS=$(python3 -c "import json; import sys; data=json.load(open('miner-wallet.json')); print(''.join([f'{x:02x}' for x in data['public_key'][:64]]))")
        print_status "Your wallet address: $WALLET_ADDRESS"
    elif command -v jq &> /dev/null; then
        # Try jq as fallback
        WALLET_ADDRESS=$(cat miner-wallet.json | jq -r '.public_key | map(sprintf("%02x"; .)) | join("")' | head -c 128)
        print_status "Your wallet address: $WALLET_ADDRESS"
    else
        print_warning "Neither python3 nor jq found. Please install one to view your wallet address."
    fi
}

# Create mining script
create_mining_script() {
    print_header "Creating mining script..."
    
    cat > start-mining.sh << 'EOF'
#!/bin/bash

# NumiCoin Mining Script
# Run this script to start mining

set -e

echo "⛏️ Starting NumiCoin mining..."

# Check if binary exists
if [ ! -f "target/release/numi-core" ]; then
    echo "Error: numi-core binary not found. Please run setup-miner.sh first."
    exit 1
fi

# Check if wallet exists
if [ ! -f "miner-wallet.json" ]; then
    echo "Error: miner-wallet.json not found. Please run setup-miner.sh first."
    exit 1
fi

# Get number of CPU cores
CPU_CORES=$(nproc)
RECOMMENDED_THREADS=$((CPU_CORES / 2))

echo "Detected $CPU_CORES CPU cores"
echo "Recommended mining threads: $RECOMMENDED_THREADS"

# Start mining
echo "Starting mining with $RECOMMENDED_THREADS threads..."
./target/release/numi-core start --enable-mining --mining-threads $RECOMMENDED_THREADS
EOF

    chmod +x start-mining.sh
    print_status "Mining script created: start-mining.sh"
}

# Create monitoring script
create_monitoring_script() {
    print_header "Creating monitoring script..."
    
    cat > monitor.sh << 'EOF'
#!/bin/bash

# NumiCoin Monitoring Script
# Run this script to monitor your mining node

echo "📊 NumiCoin Node Monitor"
echo "========================"

# Check if node is running
if pgrep -f "numi-core" > /dev/null; then
    echo "✅ Node is running"
    
    # Get process info
    PID=$(pgrep -f "numi-core")
    echo "Process ID: $PID"
    
    # Get memory usage
    MEMORY=$(ps -o rss= -p $PID | awk '{print $1/1024 " MB"}')
    echo "Memory usage: $MEMORY"
    
    # Get CPU usage
    CPU=$(ps -o %cpu= -p $PID)
    echo "CPU usage: $CPU%"
    
    # Check blockchain status
    echo ""
    echo "📈 Blockchain Status:"
    ./target/release/numi-core status
    
else
    echo "❌ Node is not running"
    echo "Run ./start-mining.sh to start mining"
fi
EOF

    chmod +x monitor.sh
    print_status "Monitoring script created: monitor.sh"
}

# Create configuration files
create_configs() {
    print_header "Creating configuration files..."
    
    # Create mobile config
    cat > mobile.toml << 'EOF'
[network]
enabled = true
listen_address = "0.0.0.0"
listen_port = 8333
max_peers = 10
connection_timeout_secs = 15
bootstrap_nodes = []
enable_upnp = false
enable_mdns = true
peer_discovery_interval_secs = 120
max_message_size = 1048576
ban_duration_secs = 600
rate_limit_per_peer = 500

[mining]
enabled = true
thread_count = 1
nonce_chunk_size = 1000
stats_update_interval_secs = 10
enable_cpu_affinity = false
thermal_throttle_temp = 70.0
power_limit_watts = 0.0
target_block_time_secs = 15
difficulty_adjustment_interval = 30

[mining.argon2_config]
memory_cost = 2048
time_cost = 1
parallelism = 1
output_length = 32
salt_length = 16

[rpc]
enabled = true
bind_address = "127.0.0.1"
port = 8080
max_connections = 50
request_timeout_secs = 30
max_request_size = 1048576
enable_cors = true
allowed_origins = ["http://localhost:3000"]
rate_limit_requests_per_minute = 100
rate_limit_burst_size = 10
enable_authentication = false
admin_endpoints_enabled = true

[storage]
data_directory = "./dev-data"
backup_directory = "./backups"
max_database_size_mb = 512
cache_size_mb = 64
enable_compression = true
enable_encryption = false
auto_backup = false
backup_interval_hours = 24
retention_days = 7
sync_mode = "Fast"

[consensus]
difficulty_adjustment_interval = 30
max_block_size = 524288
max_transactions_per_block = 100
min_transaction_fee = 500
max_reorg_depth = 10
checkpoint_interval = 50
finality_depth = 100
genesis_supply = 1000000000000000
mining_reward_halving_interval = 100000
initial_mining_reward = 100000000000
EOF

    print_status "Mobile configuration created: mobile.toml"
}

# Display final instructions
show_final_instructions() {
    echo ""
    echo "🎉 Setup completed successfully!"
    echo "================================"
    echo ""
    echo "Next steps:"
    echo "1. Start mining: ./start-mining.sh"
    echo "2. Monitor your node: ./monitor.sh"
    echo "3. Check your balance: ./target/release/numi-core balance --address YOUR_ADDRESS"
    echo ""
    echo "Useful commands:"
    echo "- View status: ./target/release/numi-core status"
    echo "- Stop mining: pkill -f numi-core"
    echo "- Backup wallet: cp miner-wallet.json miner-wallet-backup.json"
    echo ""
    echo "📚 For more information, see MINING_GUIDE.md"
    echo ""
    echo "Happy mining! 🚀⛏️"
}

# Main setup process
main() {
    print_header "Starting NumiCoin miner setup..."
    
    check_rust
    build_project
    init_blockchain
    generate_wallet
    create_mining_script
    create_monitoring_script
    create_configs
    show_final_instructions
}

# Run main function
main "$@" 