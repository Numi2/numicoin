#!/bin/bash

# Test script to check if blockchain issues have been fixed
# Issues to test:
# 1. Database lock issues
# 2. RPC server binding failures
# 3. Nodes stuck at genesis block (no new blocks being mined)

set -e

echo "🔍 Testing NumiCoin Blockchain Issues"
echo "====================================="

CORE_DIR="/Users/home/numicoin/core"
TESTNET_DIR="/Users/home/numicoin/testnet"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Function to print colored output
print_status() {
    local status=$1
    local message=$2
    case $status in
        "PASS")
            echo -e "${GREEN}✅ PASS${NC}: $message"
            ;;
        "FAIL")
            echo -e "${RED}❌ FAIL${NC}: $message"
            ;;
        "WARN")
            echo -e "${YELLOW}⚠️  WARN${NC}: $message"
            ;;
        "INFO")
            echo -e "${BLUE}ℹ️  INFO${NC}: $message"
            ;;
    esac
}

# Function to cleanup processes and data
cleanup() {
    print_status "INFO" "Cleaning up processes and data..."
    pkill -f "numi-core" || true
    sleep 2
    rm -f "$CORE_DIR/core-data/.lock" || true
    rm -f "$TESTNET_DIR/testnet-data/.lock" || true
    print_status "INFO" "Cleanup completed"
}

# Function to check if port is in use
check_port() {
    local port=$1
    if lsof -i :$port >/dev/null 2>&1; then
        return 0  # Port is in use
    else
        return 1  # Port is free
    fi
}

# Function to wait for RPC server to be ready
wait_for_rpc() {
    local port=$1
    local max_attempts=30
    local attempt=1
    
    print_status "INFO" "Waiting for RPC server on port $port..."
    
    while [ $attempt -le $max_attempts ]; do
        if curl -s "http://localhost:$port/status" >/dev/null 2>&1; then
            print_status "PASS" "RPC server is responding on port $port"
            return 0
        fi
        sleep 1
        attempt=$((attempt + 1))
    done
    
    print_status "FAIL" "RPC server failed to respond on port $port after $max_attempts seconds"
    return 1
}

# Test 1: Database Lock Issues
test_database_locks() {
    print_status "INFO" "Testing database lock issues..."
    
    # Clean start
    cleanup
    
    # Try to start multiple instances simultaneously
    print_status "INFO" "Attempting to start multiple blockchain instances..."
    
    # Start first instance
    cd "$CORE_DIR"
    ./target/release/numi-core start --enable-mining --mining-threads 2 >/dev/null 2>&1 &
    local pid1=$!
    sleep 3
    
    # Check if first instance is running
    if ps -p $pid1 >/dev/null 2>&1; then
        print_status "PASS" "First blockchain instance started successfully"
    else
        print_status "FAIL" "First blockchain instance failed to start"
        return 1
    fi
    
    # Try to start second core instance (should fail due to database lock)
    cd "$CORE_DIR"
    if ./target/release/numi-core start --enable-mining --mining-threads 2 >/dev/null 2>&1; then
        print_status "FAIL" "Second core instance started despite database lock on core-data"
        kill $pid1 2>/dev/null || true
        return 1
    else
        print_status "PASS" "Database lock prevented second core instance from starting"
    fi
    
    # Cleanup
    kill $pid1 2>/dev/null || true
    sleep 2
    cleanup
    
    print_status "PASS" "Database lock test completed successfully"
}

# Test 2: RPC Server Binding Issues
test_rpc_server() {
    print_status "INFO" "Testing RPC server binding issues..."
    
    cleanup
    
    # Check if ports are free
    if check_port 8082; then
        print_status "WARN" "Port 8082 is already in use"
    else
        print_status "INFO" "Port 8082 is free"
    fi
    
    # Start only the RPC server
    cd "$CORE_DIR"
    print_status "INFO" "Starting RPC server only..."
    ./target/release/numi-core rpc --public >/dev/null 2>&1 &
    local pid=$!
    
    # Wait for RPC server to start
    if wait_for_rpc 8082; then
        print_status "PASS" "RPC server started and is responding"
        
        # Test RPC endpoints
        local status_response=$(curl -s "http://localhost:8082/status")
        if [ -n "$status_response" ]; then
            print_status "PASS" "RPC status endpoint is working"
            
            # Check if we have more than just genesis block
            local total_blocks=$(echo "$status_response" | grep -o '"total_blocks":[0-9]*' | cut -d':' -f2)
            if [ -n "$total_blocks" ] && [ "$total_blocks" -gt 1 ]; then
                print_status "PASS" "Blockchain has $total_blocks blocks (not stuck at genesis)"
            else
                print_status "WARN" "Blockchain has only $total_blocks block(s) - may be stuck at genesis"
            fi
        else
            print_status "FAIL" "RPC status endpoint returned empty response"
        fi
    else
        print_status "FAIL" "RPC server failed to start or respond"
    fi
    
    # Cleanup
    kill $pid 2>/dev/null || true
    sleep 2
    cleanup
}

# Test 3: Mining and Block Production
test_mining_and_blocks() {
    print_status "INFO" "Testing mining and block production..."
    
    cleanup
    
    # Start blockchain with mining enabled
    cd "$CORE_DIR"
    print_status "INFO" "Starting blockchain with mining enabled..."
    ./target/release/numi-core start --enable-mining --mining-threads 4 >/dev/null 2>&1 &
    local pid=$!
    
    # Wait for RPC server
    if ! wait_for_rpc 8082; then
        print_status "FAIL" "Cannot test mining - RPC server not available"
        kill $pid 2>/dev/null || true
        return 1
    fi
    
    # Get initial block count
    local initial_blocks=$(curl -s "http://localhost:8082/status" | grep -o '"total_blocks":[0-9]*' | cut -d':' -f2)
    print_status "INFO" "Initial block count: $initial_blocks"
    
    # Wait for mining to produce new blocks
    print_status "INFO" "Waiting for mining to produce new blocks (30 seconds)..."
    local start_time=$(date +%s)
    local new_blocks=0
    
    while [ $(($(date +%s) - start_time)) -lt 30 ]; do
        sleep 5
        local current_blocks=$(curl -s "http://localhost:8082/status" | grep -o '"total_blocks":[0-9]*' | cut -d':' -f2)
        if [ -n "$current_blocks" ] && [ "$current_blocks" -gt "$initial_blocks" ]; then
            new_blocks=$((current_blocks - initial_blocks))
            print_status "PASS" "Mining produced $new_blocks new block(s) in $(($(date +%s) - start_time)) seconds"
            break
        fi
    done
    
    if [ $new_blocks -eq 0 ]; then
        print_status "FAIL" "No new blocks were mined in 30 seconds - mining may not be working"
    fi
    
    # Test manual mining endpoint
    print_status "INFO" "Testing manual mining endpoint..."
    local mine_response=$(curl -s -X POST "http://localhost:8082/mine" -H "Content-Type: application/json" -d '{"threads": 2, "timeout_seconds": 10}')
    if [ -n "$mine_response" ]; then
        print_status "PASS" "Manual mining endpoint is working"
    else
        print_status "WARN" "Manual mining endpoint may not be working"
    fi
    
    # Cleanup
    kill $pid 2>/dev/null || true
    sleep 2
    cleanup
}

# Test 4: Node Connectivity
test_node_connectivity() {
    print_status "INFO" "Testing node connectivity..."
    
    cleanup
    
    # Start first node
    cd "$CORE_DIR"
    print_status "INFO" "Starting first node..."
    ./target/release/numi-core start --enable-mining --mining-threads 2 >/dev/null 2>&1 &
    local pid1=$!
    
    # Wait for first node to be ready
    if ! wait_for_rpc 8082; then
        print_status "FAIL" "First node failed to start"
        kill $pid1 2>/dev/null || true
        return 1
    fi
    
    # Get peer count
    local peer_count=$(curl -s "http://localhost:8082/status" | grep -o '"network_peers":[0-9]*' | cut -d':' -f2)
    print_status "INFO" "First node peer count: $peer_count"
    
    # Start second node on different port
    cd "$TESTNET_DIR"
    print_status "INFO" "Starting second node..."
    # Modify config to use different ports
    sed -i.bak 's/port = 8081/port = 8083/' testnet.toml
    sed -i.bak 's/listen_port = 8334/listen_port = 8335/' testnet.toml
    
    ./start-testnet.sh >/dev/null 2>&1 &
    local pid2=$!
    
    # Wait for second node
    if wait_for_rpc 8083; then
        print_status "PASS" "Second node started successfully"
        
        # Check if nodes can see each other
        sleep 10
        local peer_count2=$(curl -s "http://localhost:8083/status" | grep -o '"network_peers":[0-9]*' | cut -d':' -f2)
        print_status "INFO" "Second node peer count: $peer_count2"
        
        if [ "$peer_count2" -gt 0 ]; then
            print_status "PASS" "Nodes are connecting to each other"
        else
            print_status "WARN" "Nodes may not be connecting to each other"
        fi
    else
        print_status "FAIL" "Second node failed to start"
    fi
    
    # Cleanup
    kill $pid1 $pid2 2>/dev/null || true
    sleep 2
    cleanup
    
    # Restore original config
    cd "$TESTNET_DIR"
    mv testnet.toml.bak testnet.toml 2>/dev/null || true
}

# Main test execution
main() {
    echo "Starting comprehensive blockchain issue tests..."
    echo ""
    
    # Run all tests
    test_database_locks
    echo ""
    
    test_rpc_server
    echo ""
    
    test_mining_and_blocks
    echo ""
    
    test_node_connectivity
    echo ""
    
    # Final cleanup
    cleanup
    
    echo "🎉 All tests completed!"
    echo ""
    echo "Summary:"
    echo "- Database lock issues: Checked"
    echo "- RPC server binding issues: Checked"
    echo "- Mining and block production: Checked"
    echo "- Node connectivity: Checked"
}

# Run main function
main "$@" 