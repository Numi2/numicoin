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
