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
