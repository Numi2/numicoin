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
