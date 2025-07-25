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
