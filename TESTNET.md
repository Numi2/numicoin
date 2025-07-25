# NumiCoin Testnet Documentation

## Overview

The NumiCoin testnet is a fully functional blockchain network that implements the same cryptographic security standards as the mainnet. It provides a safe environment for testing, development, and experimentation with the NumiCoin blockchain technology.

## Cryptographic Security

The testnet implements the same post-quantum cryptographic standards as the mainnet:

### 1. Dilithium3 Digital Signatures
- **Purpose**: Post-quantum secure digital signatures for all transactions and blocks
- **Security Level**: 128-bit post-quantum security
- **Usage**: 
  - Transaction signing and verification
  - Block signing by miners
  - Peer authentication
  - Message integrity

### 2. Argon2id Proof-of-Work
- **Purpose**: Memory-hard proof-of-work algorithm for block mining
- **Security Level**: Resistant to ASIC and GPU attacks
- **Parameters**:
  - Memory Cost: 4096 KiB (8192 KiB for validators)
  - Time Cost: 3 iterations (4 for validators)
  - Parallelism: 1 thread (2 for validators)
  - Output Length: 32 bytes
  - Salt Length: 16 bytes

### 3. Blake3 Hashing
- **Purpose**: Fast cryptographic hashing for block and transaction IDs
- **Features**:
  - Parallelizable
  - Tree-structured
  - Constant-time operations
  - 256-bit output

### 4. Kyber Key Encapsulation
- **Purpose**: Post-quantum secure key exchange for peer communication
- **Security Level**: 128-bit post-quantum security
- **Usage**: Secure peer-to-peer communication

## Testnet Architecture

### Network Structure
```
Testnet Node (Port 8334) ←→ Validator Node (Port 8335) ←→ User Node (Port 8336)
        ↓                        ↓                        ↓
   RPC Port 8081           RPC Port 8082            RPC Port 8083
```

### Consensus Mechanism
- **Algorithm**: Proof-of-Work with Argon2id
- **Block Creation**: Miners solve Argon2id puzzles to create blocks
- **Block Time**: 15 seconds
- **Difficulty Adjustment**: Every 30 blocks
- **Finality**: 200 blocks (50 minutes)

### Transaction Types
1. **Transfer**: Standard NUMI transfers
2. **Mining Reward**: Block mining rewards

## Getting Started

### Prerequisites
- Rust 1.70+ installed
- 4GB+ RAM available
- 10GB+ disk space
- Network connectivity

### Quick Setup

1. **Clone and build the project:**
   ```bash
   git clone <repository-url>
   cd numicoin
   cd core
   cargo build --release --features "temporary-pqcrypto"
   cd ..
   ```

2. **Run the testnet setup:**
   ```bash
   chmod +x scripts/setup-testnet.sh
   ./scripts/setup-testnet.sh
   ```

3. **Start the testnet:**
   ```bash
   cd testnet
   ./start-testnet.sh
   ```

4. **Monitor the testnet:**
   ```bash
   ./monitor.sh
   ```

### Setting Up a Mining Node

1. **Run the mining node setup:**
   ```bash
   chmod +x scripts/setup-validator.sh
   ./scripts/setup-validator.sh
   ```

2. **Start the mining node:**
   ```bash
   cd validator
   ./start-miner.sh
   ```

3. **Monitor mining:**
   ```bash
   ./monitor-mining.sh
   ```



## Testnet Configuration

### Network Parameters
- **P2P Port**: 8334 (testnet), 8335 (validator), 8336 (user)
- **RPC Port**: 8081 (testnet), 8082 (validator), 8083 (user)
- **Max Peers**: 20 (testnet), 30 (validator)
- **Block Time**: 15 seconds
- **Max Block Size**: 1MB
- **Max Transactions per Block**: 500

### Economic Parameters
- **Genesis Supply**: 1,000,000 NUMI
- **Initial Mining Reward**: 100 NUMI per block
- **Halving Interval**: 100,000 blocks
- **Min Transaction Fee**: 0.0000005 NUMI


### Security Parameters
- **Rate Limiting**: 500 requests/minute per peer
- **Ban Duration**: 10 minutes for malicious peers
- **Max Reorg Depth**: 20 blocks
- **Checkpoint Interval**: 100 blocks
- **Finality Depth**: 200 blocks

## Using the Testnet

### Generating Keys
```bash
# Generate a new keypair
./core/target/release/numi-core generate-key --output my_key.json

# View public key
cat my_key.json | jq -r '.public_key'
```

### Using the Faucet
```bash
# Get test NUMI from faucet
cd testnet
./faucet.sh <your_public_key> 1000  # Get 1000 NUMI
```

### Submitting Transactions
```bash
# Transfer NUMI
./core/target/release/numi-core submit \
    --from-key my_key.json \
    --to <recipient_public_key> \
    --amount 1000000000  # 1 NUMI in smallest units

# Stake for governance participation
./core/target/release/numi-core stake \
    --from-key my_key.json \
    --amount 100000000000000  # 100,000 NUMI for governance
```

### Checking Balances
```bash
# Check account balance
./core/target/release/numi-core balance \
    --address <public_key>

# Check staking info
./core/target/release/numi-core balance \
    --address <public_key> \
    --show-staking
```

### Mining Blocks
```bash
# Mine a single block
./core/target/release/numi-core mine \
    --threads 4 \
    --timeout 300
```

## API Endpoints

### RPC API (Port 8081/8082/8083)

#### Blockchain Information
```bash
# Get blockchain status
curl http://localhost:8081/status

# Get block by height
curl http://localhost:8081/block/123

# Get block by hash
curl http://localhost:8081/block/0x1234...
```

#### Account Information
```bash
# Get account balance
curl "http://localhost:8081/balance?address=<public_key>"

# Get transaction history
curl "http://localhost:8081/transactions?address=<public_key>"
```

#### Transaction Operations
```bash
# Submit transaction
curl -X POST http://localhost:8081/transaction \
    -H "Content-Type: application/json" \
    -d '{
        "from": "<sender_public_key>",
        "to": "<recipient_public_key>",
        "amount": 1000000000,
        "nonce": 0,
        "signature": "<dilithium3_signature>"
    }'
```

#### Mining Operations
```bash
# Get mining statistics
curl http://localhost:8081/mining/stats

# Start mining
curl -X POST http://localhost:8081/mining/start \
    -H "Content-Type: application/json" \
    -d '{"threads": 4}'

# Stop mining
curl -X POST http://localhost:8081/mining/stop
```

## Security Features

### Cryptographic Implementation
- **Constant-time Operations**: All cryptographic operations are implemented with constant-time algorithms to prevent timing attacks
- **Secure Random Number Generation**: Uses cryptographically secure random number generators
- **Key Derivation**: Proper key derivation using Argon2id
- **Memory Protection**: Sensitive data is zeroized after use

### Network Security
- **Peer Authentication**: All peers are authenticated using Dilithium3 signatures
- **Message Encryption**: Peer-to-peer messages are encrypted using Kyber KEM
- **Rate Limiting**: Protection against spam and DoS attacks
- **IP Blocking**: Automatic blocking of malicious peers

### Transaction Security
- **Signature Verification**: All transactions are verified using Dilithium3 signatures
- **Nonce Protection**: Replay protection using account nonces
- **Balance Validation**: Comprehensive balance and fee validation
- **Structure Validation**: Full transaction structure validation

## Monitoring and Maintenance

### Health Checks
```bash
# Check node health
cd testnet
./health-check.sh

# Monitor performance
cd validator
./performance.sh
```

### Backups
```bash
# Create backup
cd testnet
./backup.sh

# Restore from backup
./restore.sh backup_file.tar.gz
```

### Logs
- **Location**: `testnet-data/logs/` or `validator-data/logs/`
- **Rotation**: Automatic log rotation
- **Level**: Configurable log levels (DEBUG, INFO, WARN, ERROR)

## Development and Testing

### Running Tests
```bash
# Run all tests
cd core
cargo test

# Run specific test modules
cargo test crypto
cargo test blockchain
cargo test network
```

### Performance Testing
```bash
# Benchmark cryptographic operations
cargo bench

# Stress test the network
./scripts/stress-test.sh
```

### Integration Testing
```bash
# Run integration tests
./scripts/integration-test.sh

# Test network connectivity
./scripts/network-test.sh
```

## Troubleshooting

### Common Issues

1. **Node won't start**
   - Check if ports are available
   - Verify configuration file syntax
   - Check system resources

2. **Can't connect to peers**
   - Verify network connectivity
   - Check firewall settings
   - Ensure bootstrap nodes are reachable

3. **Mining not working**
   - Check CPU resources
   - Verify mining configuration
   - Check difficulty settings

4. **Transaction failures**
   - Verify Dilithium3 key integrity
   - Check account balance and nonce
   - Validate transaction structure

### Debug Mode
```bash
# Enable debug logging
RUST_LOG=debug ./start-testnet.sh

# Enable trace logging
RUST_LOG=trace ./start-testnet.sh
```

### Performance Optimization
- **CPU Affinity**: Enable for better mining performance
- **Memory Tuning**: Adjust Argon2id memory cost based on available RAM
- **Network Tuning**: Optimize peer limits and timeouts
- **Storage Tuning**: Use SSD for better performance

## Network Statistics

### Current Testnet Status
- **Network Hash Rate**: ~1,000 H/s
- **Active Peers**: 5-10 nodes
- **Block Height**: Varies
- **Total Supply**: 1,000,000 NUMI
- **Active Validators**: 2-3 nodes

### Performance Metrics
- **Average Block Time**: 15 seconds
- **Transaction Throughput**: ~30 TPS
- **Network Latency**: <100ms
- **Storage Usage**: ~100MB per node

## Contributing

### Reporting Issues
- Use GitHub issues for bug reports
- Include logs and configuration details
- Provide steps to reproduce

### Feature Requests
- Submit feature requests via GitHub issues
- Include use case and requirements
- Consider security implications

### Code Contributions
- Follow Rust coding standards
- Include tests for new features
- Ensure cryptographic security
- Update documentation

## Support

### Documentation
- [API Reference](docs/api.md)
- [Cryptographic Implementation](docs/crypto.md)
- [Network Protocol](docs/protocol.md)

### Community
- [Discord Server](https://discord.gg/numicoin)
- [Telegram Group](https://t.me/numicoin)
- [GitHub Discussions](https://github.com/numicoin/numicoin/discussions)

### Security
- [Security Policy](SECURITY.md)
- [Vulnerability Reporting](security@numicoin.org)
- [Audit Reports](docs/audits.md)

---

**Note**: This testnet is for development and testing purposes only. NUMI tokens on the testnet have no real value and should not be used for any financial transactions. 