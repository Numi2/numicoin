#!/bin/bash

# Script to connect to external NumiCoin nodes
# Usage: ./connect-external.sh <node_ip> <node_port>

set -e

if [ $# -ne 2 ]; then
    echo "Usage: $0 <node_ip> <node_port>"
    echo "Example: $0 192.168.1.100 8333"
    exit 1
fi

NODE_IP=$1
NODE_PORT=$2
NODE_ADDRESS="/ip4/$NODE_IP/tcp/$NODE_PORT"

echo "🔗 Connecting to external node: $NODE_ADDRESS"

# Add the external node to bootstrap nodes
if ! grep -q "$NODE_ADDRESS" numi.toml; then
    echo "Adding $NODE_ADDRESS to bootstrap nodes..."
    
    # Create backup
    cp numi.toml numi.toml.backup
    
    # Add the node to bootstrap_nodes array
    sed -i.bak "s|bootstrap_nodes = \[|bootstrap_nodes = [\n    \"$NODE_ADDRESS\",|" numi.toml
    
    echo "✅ External node added to configuration"
    echo "Restart your mining node to connect: ./start-mining.sh"
else
    echo "⚠️ Node $NODE_ADDRESS is already in bootstrap nodes"
fi 