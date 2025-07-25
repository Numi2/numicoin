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
