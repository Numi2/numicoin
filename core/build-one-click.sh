#!/bin/bash

echo "🏗️  Building NumiCoin One-Click Miner..."
echo "========================================="

# Exit on any error
set -e

# Clean previous builds
echo "🧹 Cleaning previous builds..."
cargo clean

# Build with maximum optimizations for distribution
echo "⚡ Building optimized release binary..."
cargo build --release --bin numi-one-click

# Check if build was successful
if [ ! -f "target/release/numi-one-click" ]; then
    echo "❌ Build failed - executable not found"
    exit 1
fi

# Get executable size
SIZE=$(du -h target/release/numi-one-click | cut -f1)
echo "📦 Built executable size: $SIZE"

# Strip debug symbols for smaller size (optional)
if command -v strip >/dev/null 2>&1; then
    echo "🔧 Stripping debug symbols..."
    strip target/release/numi-one-click
    STRIPPED_SIZE=$(du -h target/release/numi-one-click | cut -f1)
    echo "📦 Stripped executable size: $STRIPPED_SIZE"
fi

# Create distribution directory
DIST_DIR="dist"
mkdir -p "$DIST_DIR"

# Copy executable with a user-friendly name
if [[ "$OSTYPE" == "msys" || "$OSTYPE" == "win32" ]]; then
    # Windows
    cp target/release/numi-one-click.exe "$DIST_DIR/NumiCoin-Miner.exe"
    echo "✅ Windows executable created: $DIST_DIR/NumiCoin-Miner.exe"
elif [[ "$OSTYPE" == "darwin"* ]]; then
    # macOS
    cp target/release/numi-one-click "$DIST_DIR/NumiCoin-Miner-macOS"
    chmod +x "$DIST_DIR/NumiCoin-Miner-macOS"
    echo "✅ macOS executable created: $DIST_DIR/NumiCoin-Miner-macOS"
else
    # Linux and others
    cp target/release/numi-one-click "$DIST_DIR/NumiCoin-Miner-Linux"
    chmod +x "$DIST_DIR/NumiCoin-Miner-Linux"
    echo "✅ Linux executable created: $DIST_DIR/NumiCoin-Miner-Linux"
fi

# Create README for distribution
cat > "$DIST_DIR/README.txt" << 'EOF'
NumiCoin One-Click Miner
========================

This is a simple, one-click cryptocurrency miner for NumiCoin.

HOW TO USE:
1. Double-click the executable to start mining
2. A wallet will be automatically created for you
3. Mining will begin immediately
4. Press Ctrl+C to stop and exit

WHAT IT DOES:
- Creates your personal wallet automatically
- Starts mining NumiCoin with your CPU
- Shows real-time mining progress
- Saves all data in the same folder

FILES CREATED:
- my-wallet.json: Your wallet (KEEP THIS SAFE!)
- numi-data/: Blockchain data directory

SECURITY NOTE:
Keep your wallet file safe! It contains your private keys.
Back it up somewhere secure.

SYSTEM REQUIREMENTS:
- Modern CPU (more cores = faster mining)
- ~100MB free disk space
- Internet connection (optional for solo mining)

For questions, visit: https://github.com/numicoin/numicoin
EOF

echo ""
echo "🎉 Build complete!"
echo "📁 Distribution files in: $DIST_DIR/"
echo "🚀 Users can now simply download and run the executable!"
echo ""
echo "Next steps:"
echo "1. Test the executable: ./$DIST_DIR/NumiCoin-Miner-*"
echo "2. Distribute the files in the $DIST_DIR/ folder"
echo "3. Users just need to double-click to start mining!" 