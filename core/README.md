# Numi Blockchain Core

A quantum-safe blockchain implementation in Rust using Dilithium3 signatures, BLAKE3 hashing, and Argon2id proof-of-work.

## 🚀 Features

- **Quantum-Safe Cryptography**: Uses Dilithium3 for post-quantum signatures
- **Fast Hashing**: BLAKE3 for efficient and secure hashing
- **Custom PoW**: Argon2id + BLAKE3 proof-of-work algorithm
- **P2P Networking**: libp2p-based peer-to-peer communication
- **Persistent Storage**: Sled database for blockchain data
- **CLI Interface**: Command-line tools for node management
- **Mining**: CPU-based mining with difficulty adjustment

## 🏗️ Architecture

### Core Components

- **Block**: Block structure with headers and transactions
- **Transaction**: Various transaction types (transfer, stake, mining reward, governance)
- **Blockchain**: Main chain logic and state management
- **Crypto**: Quantum-safe cryptography implementation
- **Miner**: CPU mining with custom PoW algorithm
- **Network**: P2P networking using libp2p
- **Storage**: Persistent storage using Sled database

### Proof-of-Work Algorithm

```
blake3(argon2id(header_blob + nonce)) < difficulty_target
```

- **Argon2id**: Memory-hard function for PoW
- **BLAKE3**: Fast final hashing
- **Difficulty**: Adjustable based on block time

## 📦 Installation

### Prerequisites

- Rust 1.70+ and Cargo
- liboqs (for quantum-safe cryptography)

### Building

```bash
# Clone the repository
git clone <repository-url>
cd numicoin/core

# Build the project
cargo build --release

# Run tests
cargo test
```

## 🎯 Usage

### Initialize Blockchain

```bash
# Initialize a new blockchain
cargo run -- init --data-dir ./data
```

### Start Node

```bash
# Start a blockchain node
cargo run -- start --port 8080 --data-dir ./data
```

### Mining

```bash
# Mine a new block
cargo run -- mine --data-dir ./data
```

### Submit Transaction

```bash
# Submit a transfer transaction
cargo run -- submit \
  --from <sender-address> \
  --to <recipient-address> \
  --amount 1000000000 \
  --data-dir ./data
```

### Check Status

```bash
# Get blockchain status
cargo run -- status --data-dir ./data

# Get account balance
cargo run -- balance --address <address> --data-dir ./data
```

## 🔧 Configuration

### Environment Variables

- `RUST_LOG`: Set logging level (e.g., `info`, `debug`)
- `NUMI_DATA_DIR`: Default data directory path

### Network Configuration

- Default port: 8080
- P2P discovery: mDNS
- Transport: TCP with Noise encryption

## 🧪 Testing

```bash
# Run all tests
cargo test

# Run specific test module
cargo test crypto

# Run with logging
RUST_LOG=debug cargo test
```

## 📊 Performance

### Benchmarks

```bash
# Run benchmarks
cargo bench
```

### Expected Performance

- **Block Validation**: ~1ms per block
- **Transaction Processing**: ~0.1ms per transaction
- **Mining**: Variable based on difficulty
- **Network Sync**: ~100 blocks/second

## 🔐 Security

### Quantum-Safe Features

- **Dilithium3 Signatures**: Post-quantum secure signatures
- **BLAKE3 Hashing**: Fast and secure hashing
- **Argon2id PoW**: Memory-hard proof-of-work

### Security Considerations

- All cryptographic operations use well-vetted libraries
- Network communication is encrypted with Noise protocol
- Storage is protected against corruption
- Input validation on all public interfaces

## 🌐 Networking

### P2P Protocol

- **Discovery**: mDNS for local peer discovery
- **Messaging**: Floodsub for pub/sub messaging
- **Transport**: TCP with Noise encryption
- **Topics**: Blocks, transactions, chain sync

### Message Types

- `NewBlock`: Broadcast new blocks
- `NewTransaction`: Broadcast new transactions
- `BlockRequest/Response`: Block synchronization
- `ChainRequest/Response`: Full chain sync
- `Ping/Pong`: Peer health checks

## 💾 Storage

### Data Structure

- **Blocks**: Stored by height
- **Transactions**: Stored by transaction ID
- **Accounts**: Stored by public key
- **State**: Chain state and metadata

### Database Operations

- Atomic transactions
- Automatic compaction
- Crash recovery
- Efficient iteration

## 🔄 API Reference

### Blockchain

```rust
// Create new blockchain
let mut blockchain = NumiBlockchain::new()?;

// Add block
blockchain.add_block(block)?;

// Add transaction
blockchain.add_transaction(transaction)?;

// Mine block
let block = blockchain.mine_block(miner_pubkey)?;

// Get state
let state = blockchain.get_chain_state();
```

### Crypto

```rust
// Generate keypair
let keypair = Dilithium3Keypair::new()?;

// Sign message
let signature = keypair.sign(message)?;

// Verify signature
let valid = Dilithium3Keypair::verify(message, &signature)?;

// Hash data
let hash = blake3_hash(data);
```

### Mining

```rust
// Create miner
let miner = Miner::new()?;

// Mine block
let result = miner.mine_block(height, prev_hash, txs, difficulty, start_nonce)?;

// Get stats
let stats = miner.get_stats();
```

## 🚨 Troubleshooting

### Common Issues

1. **liboqs not found**: Install liboqs development libraries
2. **Port already in use**: Change port with `--port` flag
3. **Storage errors**: Check disk space and permissions
4. **Network issues**: Check firewall settings

### Debug Mode

```bash
# Enable debug logging
RUST_LOG=debug cargo run -- start

# Verbose output
RUST_LOG=trace cargo run -- start
```

## 📈 Development

### Adding Features

1. Create feature branch
2. Implement changes
3. Add tests
4. Update documentation
5. Submit pull request

### Code Style

- Follow Rust conventions
- Use `cargo fmt` for formatting
- Use `cargo clippy` for linting
- Write comprehensive tests

## 📄 License

This project is licensed under the MIT License - see the LICENSE file for details.

## 🤝 Contributing

Contributions are welcome! Please read the contributing guidelines before submitting pull requests.

## 📞 Support

For support and questions:
- Create an issue on GitHub
- Join the community Discord
- Check the documentation

---

**Numi Blockchain Core** - Building the future of quantum-safe cryptocurrency 🚀 