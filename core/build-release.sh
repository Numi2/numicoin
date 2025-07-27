#!/bin/bash
set -e

echo "🏗️ Building NumiCoin One-Click Miner for all platforms..."
echo "==========================================================="

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Check if cross is installed
if ! command -v cross &> /dev/null; then
    echo -e "${YELLOW}⚠️ 'cross' not found. Installing...${NC}"
    cargo install cross
fi

# Clean previous builds
echo -e "${YELLOW}🧹 Cleaning previous builds...${NC}"
cargo clean

# Create releases directory
mkdir -p releases
rm -rf releases/*

# Function to build for a target
build_target() {
    local target=$1
    local output_name=$2
    local extension=$3
    
    echo -e "${YELLOW}🔨 Building for $target...${NC}"
    
    if cross build --release --target $target --bin numi-one-click; then
        cp target/$target/release/numi-one-click$extension releases/$output_name$extension
        echo -e "${GREEN}✅ $target build successful${NC}"
    else
        echo -e "${RED}❌ $target build failed${NC}"
        return 1
    fi
}

# Build for all platforms
echo ""
echo "Building executables..."

# Windows
build_target "x86_64-pc-windows-gnu" "NumiCoin-Miner-Windows" ".exe"

# Linux
build_target "x86_64-unknown-linux-gnu" "NumiCoin-Miner-Linux" ""

# macOS Intel
if [[ "$OSTYPE" == "darwin"* ]]; then
    build_target "x86_64-apple-darwin" "NumiCoin-Miner-macOS-Intel" ""
    
    # macOS ARM (M1/M2) - only on macOS
    build_target "aarch64-apple-darwin" "NumiCoin-Miner-macOS-ARM" ""
else
    echo -e "${YELLOW}⚠️ Skipping macOS builds (not on macOS system)${NC}"
fi

# Make all executables executable
chmod +x releases/*

# Get file sizes
echo ""
echo -e "${GREEN}📦 Build Results:${NC}"
echo "================="

for file in releases/*; do
    if [ -f "$file" ]; then
        size=$(du -h "$file" | cut -f1)
        echo "$(basename "$file"): $size"
    fi
done

# Create checksums
echo ""
echo -e "${YELLOW}🔐 Generating checksums...${NC}"
cd releases
sha256sum * > checksums.txt
cd ..

# Create README for releases
cat > releases/README.txt << EOF
NumiCoin One-Click Miner - Release Files
========================================

DOWNLOAD INSTRUCTIONS:
1. Choose the file for your operating system:
   - Windows: NumiCoin-Miner-Windows.exe
   - Linux: NumiCoin-Miner-Linux  
   - macOS Intel: NumiCoin-Miner-macOS-Intel
   - macOS ARM: NumiCoin-Miner-macOS-ARM

2. Download and run immediately - no installation needed!

SECURITY:
- Verify checksums using checksums.txt
- All executables are built from the same source code
- Keep your generated wallet file safe!

REQUIREMENTS:
- Modern CPU (more cores = faster mining)
- ~100MB free disk space
- Optional: Internet connection for P2P mining

SUPPORT:
- GitHub: https://github.com/your-username/numicoin
- Issues: Report bugs via GitHub Issues

Build Date: $(date)
Version: $(git rev-parse --short HEAD 2>/dev/null || echo "unknown")
EOF

echo ""
echo -e "${GREEN}🎉 All builds complete!${NC}"
echo -e "${GREEN}📁 Release files are in: releases/${NC}"
echo ""
echo "Next steps:"
echo "1. Test each executable on target platforms"
echo "2. Upload to GitHub Releases or your distribution platform"
echo "3. Share download links with users!"
echo ""
echo "Files created:"
ls -la releases/ 