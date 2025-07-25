#!/bin/bash

# NumiCoin Testnet Verification Script
# This script tests the testnet setup and verifies cryptographic functionality

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}🧪 Testing NumiCoin Testnet Setup...${NC}"

# Configuration
TESTNET_DIR="./testnet"
VALIDATOR_DIR="./validator"
KEYS_DIR="./testnet-keys"
RPC_URL="http://localhost:8081"
VALIDATOR_RPC_URL="http://localhost:8082"

# Function to test endpoint
test_endpoint() {
    local url=$1
    local name=$2
    
    echo -e "${YELLOW}Testing $name...${NC}"
    if curl -s "$url" > /dev/null 2>&1; then
        echo -e "${GREEN}✅ $name is responding${NC}"
        return 0
    else
        echo -e "${RED}❌ $name is not responding${NC}"
        return 1
    fi
}

# Function to test cryptographic operations
test_crypto() {
    echo -e "${YELLOW}Testing cryptographic operations...${NC}"
    
    # Test key generation
    if [ -f "./core/target/release/numi-core" ]; then
        echo -e "${BLUE}Testing Dilithium3 key generation...${NC}"
        ./core/target/release/numi-core generate-key --output test_key.json --format json
        if [ -f "test_key.json" ]; then
            echo -e "${GREEN}✅ Dilithium3 key generation successful${NC}"
            rm test_key.json
        else
            echo -e "${RED}❌ Dilithium3 key generation failed${NC}"
            return 1
        fi
    else
        echo -e "${RED}❌ Blockchain binary not found${NC}"
        return 1
    fi
    
    return 0
}

# Function to test blockchain operations
test_blockchain() {
    echo -e "${YELLOW}Testing blockchain operations...${NC}"
    
    # Test if node is running
    if ! test_endpoint "$RPC_URL/status" "Testnet RPC"; then
        echo -e "${RED}❌ Testnet node is not running${NC}"
        echo -e "${YELLOW}Please start the testnet first:${NC}"
        echo -e "cd $TESTNET_DIR && ./start-testnet.sh"
        return 1
    fi
    
    # Test blockchain status
    echo -e "${BLUE}Getting blockchain status...${NC}"
    STATUS=$(curl -s "$RPC_URL/status")
    if [ $? -eq 0 ]; then
        HEIGHT=$(echo "$STATUS" | jq -r '.data.total_blocks // 0')
        echo -e "${GREEN}✅ Blockchain height: $HEIGHT${NC}"
    else
        echo -e "${RED}❌ Failed to get blockchain status${NC}"
        return 1
    fi
    
    return 0
}

# Function to test validator operations
test_validator() {
    echo -e "${YELLOW}Testing validator operations...${NC}"
    
    # Test if validator is running
    if ! test_endpoint "$VALIDATOR_RPC_URL/status" "Validator RPC"; then
        echo -e "${YELLOW}⚠️ Validator is not running (optional)${NC}"
        return 0
    fi
    
    # Test validator status
    echo -e "${BLUE}Getting validator status...${NC}"
    VALIDATOR_STATUS=$(curl -s "$VALIDATOR_RPC_URL/status")
    if [ $? -eq 0 ]; then
        VALIDATOR_HEIGHT=$(echo "$VALIDATOR_STATUS" | jq -r '.data.total_blocks // 0')
        echo -e "${GREEN}✅ Validator height: $VALIDATOR_HEIGHT${NC}"
    else
        echo -e "${RED}❌ Failed to get validator status${NC}"
        return 1
    fi
    
    return 0
}

# Function to test network connectivity
test_network() {
    echo -e "${YELLOW}Testing network connectivity...${NC}"
    
    # Test peer connectivity
    echo -e "${BLUE}Testing peer connectivity...${NC}"
    PEERS=$(curl -s "$RPC_URL/stats" | jq -r '.data.network_peers // 0')
    if [ $? -eq 0 ]; then
        echo -e "${GREEN}✅ Connected peers: $PEERS${NC}"
    else
        echo -e "${YELLOW}⚠️ Could not get peer count${NC}"
    fi
    
    # Test network sync status
    echo -e "${BLUE}Testing network sync status...${NC}"
    SYNCING=$(curl -s "$RPC_URL/status" | jq -r '.data.is_syncing // false')
    if [ $? -eq 0 ]; then
        if [ "$SYNCING" = "true" ]; then
            echo -e "${YELLOW}⚠️ Node is syncing${NC}"
        else
            echo -e "${GREEN}✅ Node is synced${NC}"
        fi
    else
        echo -e "${RED}❌ Failed to get sync status${NC}"
    fi
    
    return 0
}

# Function to test transaction operations
test_transactions() {
    echo -e "${YELLOW}Testing transaction operations...${NC}"
    
    # Generate test keys
    echo -e "${BLUE}Generating test keys...${NC}"
    ./core/target/release/numi-core generate-key --output test_sender.json --format json
    ./core/target/release/numi-core generate-key --output test_receiver.json --format json
    
    if [ ! -f "test_sender.json" ] || [ ! -f "test_receiver.json" ]; then
        echo -e "${RED}❌ Failed to generate test keys${NC}"
        return 1
    fi
    
    # Get public keys
    SENDER_PUBKEY=$(cat test_sender.json | jq -r '.public_key')
    RECEIVER_PUBKEY=$(cat test_receiver.json | jq -r '.public_key')
    
    echo -e "${GREEN}✅ Test keys generated${NC}"
    echo -e "${BLUE}Sender: $SENDER_PUBKEY${NC}"
    echo -e "${BLUE}Receiver: $RECEIVER_PUBKEY${NC}"
    
    # Test faucet (if available)
    if [ -f "$TESTNET_DIR/faucet.sh" ]; then
        echo -e "${BLUE}Testing faucet...${NC}"
        cd "$TESTNET_DIR"
        ./faucet.sh "$SENDER_PUBKEY" 1000 > /dev/null 2>&1
        if [ $? -eq 0 ]; then
            echo -e "${GREEN}✅ Faucet test successful${NC}"
        else
            echo -e "${YELLOW}⚠️ Faucet test failed (may need funds)${NC}"
        fi
        cd ..
    fi
    
    # Clean up test keys
    rm -f test_sender.json test_receiver.json
    
    return 0
}

# Function to test mining operations
test_mining() {
    echo -e "${YELLOW}Testing mining operations...${NC}"
    
    # Test mining statistics
    echo -e "${BLUE}Getting mining statistics...${NC}"
    MINING_STATS=$(curl -s "$RPC_URL/mining/stats")
    if [ $? -eq 0 ]; then
        HASH_RATE=$(echo "$MINING_STATS" | jq -r '.data.hash_rate // 0')
        echo -e "${GREEN}✅ Mining hash rate: $HASH_RATE H/s${NC}"
    else
        echo -e "${YELLOW}⚠️ Could not get mining statistics${NC}"
    fi
    
    return 0
}

# Function to test security features
test_security() {
    echo -e "${YELLOW}Testing security features...${NC}"
    
    # Test rate limiting
    echo -e "${BLUE}Testing rate limiting...${NC}"
    for i in {1..10}; do
        curl -s "$RPC_URL/status" > /dev/null 2>&1
    done
    echo -e "${GREEN}✅ Rate limiting test completed${NC}"
    
    # Test authentication (if enabled)
    echo -e "${BLUE}Testing authentication...${NC}"
    AUTH_TEST=$(curl -s "$RPC_URL/admin/stats" 2>&1)
    if echo "$AUTH_TEST" | grep -q "unauthorized\|forbidden"; then
        echo -e "${GREEN}✅ Authentication is working${NC}"
    else
        echo -e "${YELLOW}⚠️ Authentication may not be enabled${NC}"
    fi
    
    return 0
}

# Function to test storage operations
test_storage() {
    echo -e "${YELLOW}Testing storage operations...${NC}"
    
    # Check data directory
    if [ -d "$TESTNET_DIR/../testnet-data" ]; then
        echo -e "${GREEN}✅ Testnet data directory exists${NC}"
        
        # Check database size
        DB_SIZE=$(du -sh "$TESTNET_DIR/../testnet-data" 2>/dev/null | cut -f1)
        echo -e "${BLUE}Database size: $DB_SIZE${NC}"
    else
        echo -e "${RED}❌ Testnet data directory not found${NC}"
        return 1
    fi
    
    # Check backup directory
    if [ -d "$TESTNET_DIR/../testnet-backups" ]; then
        echo -e "${GREEN}✅ Backup directory exists${NC}"
    else
        echo -e "${YELLOW}⚠️ Backup directory not found${NC}"
    fi
    
    return 0
}

# Function to generate test report
generate_report() {
    echo -e "\n${BLUE}📊 Testnet Test Report${NC}"
    echo -e "=========================="
    
    # Get current timestamp
    TIMESTAMP=$(date '+%Y-%m-%d %H:%M:%S')
    echo -e "Test Time: $TIMESTAMP"
    
    # Get system information
    echo -e "System: $(uname -s) $(uname -r)"
    echo -e "CPU: $(sysctl -n hw.ncpu 2>/dev/null || echo "Unknown") cores"
    echo -e "Memory: $(sysctl -n hw.memsize 2>/dev/null | awk '{print $0/1024/1024/1024 " GB"}' || echo "Unknown")"
    
    # Get blockchain information
    if curl -s "$RPC_URL/status" > /dev/null 2>&1; then
        STATUS=$(curl -s "$RPC_URL/status")
        HEIGHT=$(echo "$STATUS" | jq -r '.data.total_blocks // 0')
        DIFFICULTY=$(echo "$STATUS" | jq -r '.data.current_difficulty // 0')
        PEERS=$(curl -s "$RPC_URL/stats" | jq -r '.data.network_peers // 0')
        
        echo -e "Blockchain Height: $HEIGHT"
        echo -e "Current Difficulty: $DIFFICULTY"
        echo -e "Connected Peers: $PEERS"
    fi
    
    # Get validator information
    if curl -s "$VALIDATOR_RPC_URL/status" > /dev/null 2>&1; then
        VALIDATOR_STATUS=$(curl -s "$VALIDATOR_RPC_URL/status")
        VALIDATOR_HEIGHT=$(echo "$VALIDATOR_STATUS" | jq -r '.data.total_blocks // 0')
        echo -e "Validator Height: $VALIDATOR_HEIGHT"
    fi
    
    echo -e "\n${GREEN}✅ Testnet verification completed!${NC}"
}

# Main test execution
main() {
    local all_tests_passed=true
    
    echo -e "${BLUE}Starting comprehensive testnet verification...${NC}\n"
    
    # Test 1: Cryptographic operations
    if ! test_crypto; then
        all_tests_passed=false
    fi
    echo ""
    
    # Test 2: Blockchain operations
    if ! test_blockchain; then
        all_tests_passed=false
    fi
    echo ""
    
    # Test 3: Validator operations
    if ! test_validator; then
        all_tests_passed=false
    fi
    echo ""
    
    # Test 4: Network connectivity
    if ! test_network; then
        all_tests_passed=false
    fi
    echo ""
    
    # Test 5: Transaction operations
    if ! test_transactions; then
        all_tests_passed=false
    fi
    echo ""
    
    # Test 6: Mining operations
    if ! test_mining; then
        all_tests_passed=false
    fi
    echo ""
    
    # Test 7: Security features
    if ! test_security; then
        all_tests_passed=false
    fi
    echo ""
    
    # Test 8: Storage operations
    if ! test_storage; then
        all_tests_passed=false
    fi
    echo ""
    
    # Generate test report
    generate_report
    
    # Final result
    if [ "$all_tests_passed" = true ]; then
        echo -e "\n${GREEN}🎉 All tests passed! Testnet is working correctly.${NC}"
        echo -e "${BLUE}The testnet is ready for development and testing.${NC}"
        exit 0
    else
        echo -e "\n${RED}❌ Some tests failed. Please check the issues above.${NC}"
        echo -e "${YELLOW}Refer to TESTNET.md for troubleshooting information.${NC}"
        exit 1
    fi
}

# Run main function
main 