#!/bin/bash

# NumiCoin One-Click Installer
# Run this script to install and start mining NumiCoin

set -e

echo "🚀 NumiCoin One-Click Installer"
echo "================================"

# Check if we're in the right directory
if [ ! -f "Cargo.toml" ]; then
    echo "❌ Error: Please run this script from the core directory"
    echo "   cd numicoin/core && ./install.sh"
    exit 1
fi

# Check if Rust is installed
if ! command -v rustc &> /dev/null; then
    echo "📦 Installing Rust..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source ~/.cargo/env
    echo "✅ Rust installed"
else
    echo "✅ Rust already installed"
fi

# Build the project
echo "🔨 Building NumiCoin..."
cargo build --release

# Initialize blockchain
echo "🌱 Initializing blockchain..."
if [ -d "dev-data" ]; then
    echo "⚠️  Blockchain data exists, reinitializing..."
    rm -rf dev-data
fi
./target/release/numi-core init --force

# Generate wallet
echo "🔑 Generating wallet..."
if [ -f "miner-wallet.json" ]; then
    echo "⚠️  Wallet exists, creating backup..."
    cp miner-wallet.json miner-wallet-backup.json
fi
./target/release/numi-core generate-key --output miner-wallet.json

# Display wallet address
if command -v python3 &> /dev/null; then
    # Use Python (more reliable than jq)
    WALLET_ADDRESS=$(python3 -c "import json; import sys; data=json.load(open('miner-wallet.json')); print(''.join([f'{x:02x}' for x in data['public_key'][:64]]))")
    echo "💰 Your wallet address: $WALLET_ADDRESS"
elif command -v jq &> /dev/null; then
    # Try jq as fallback
    WALLET_ADDRESS=$(cat miner-wallet.json | jq -r '.public_key | map(sprintf("%02x"; .)) | join("")' | head -c 128)
    echo "💰 Your wallet address: $WALLET_ADDRESS"
else
    echo "💰 Wallet generated: miner-wallet.json"
fi

# Create start script
echo "📝 Creating start script..."
cat > start-mining.sh << 'EOF'
#!/bin/bash
echo "⛏️ Starting NumiCoin mining..."

# Get number of CPU cores (works on Linux and macOS)
if command -v nproc &> /dev/null; then
    CPU_CORES=$(nproc)
else
    CPU_CORES=$(sysctl -n hw.ncpu)
fi
THREADS=$((CPU_CORES / 2))
echo "Using $THREADS threads on $CPU_CORES cores"
./target/release/numi-core start --enable-mining --mining-threads $THREADS
EOF
chmod +x start-mining.sh

# Create stop script
echo "📝 Creating stop script..."
cat > stop-mining.sh << 'EOF'
#!/bin/bash
echo "🛑 Stopping NumiCoin mining..."
pkill -f numi-core
echo "✅ Mining stopped"
EOF
chmod +x stop-mining.sh

# Create status script
echo "📝 Creating status script..."
cat > check-status.sh << 'EOF'
#!/bin/bash
echo "📊 NumiCoin Node Status"
echo "======================="
if pgrep -f "numi-core" > /dev/null; then
    echo "✅ Node is running"
    PID=$(pgrep -f "numi-core")
    echo "Process ID: $PID"
    MEMORY=$(ps -o rss= -p $PID | awk '{print $1/1024 " MB"}')
    echo "Memory usage: $MEMORY"
    CPU=$(ps -o %cpu= -p $PID)
    echo "CPU usage: $CPU%"
    echo ""
    ./target/release/numi-core status
else
    echo "❌ Node is not running"
    echo "Run ./start-mining.sh to start mining"
fi
EOF
chmod +x check-status.sh

echo ""
echo "🎉 Installation completed successfully!"
echo "======================================"
echo ""
echo "Next steps:"
echo "1. Start mining: ./start-mining.sh"
echo "2. Check status: ./check-status.sh"
echo "3. Stop mining: ./stop-mining.sh"
echo "4. View dashboard: open dashboard.html"
echo ""
echo "📚 For more information:"
echo "- Full guide: MINING_GUIDE.md"
echo "- Quick start: README_MINING.md"
echo "- Deployment: DEPLOYMENT_GUIDE.md"
echo ""
echo "Happy mining! 🚀⛏️" 