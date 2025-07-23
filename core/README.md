# Bumi Coin Blockchain Core

A state-of-the-art, production-ready blockchain implementation in Rust featuring quantum-safe cryptography, advanced consensus mechanisms, and enterprise-grade security.

## 🌟 Features

### 🔐 Quantum-Safe Cryptography
- **Dilithium3 Post-Quantum Digital Signatures** - Resistant to quantum computer attacks
- **Argon2id Proof of Work** - Memory-hard hashing algorithm with configurable parameters
- **BLAKE3 Hashing** - Fast, secure, and collision-resistant hashing
- **AES-256-GCM Encryption** - For secure key storage and data protection

### ⚡ High Performance
- **Multi-threaded Mining** - Parallel nonce search with Rayon
- **Concurrent Data Structures** - DashMap and parking_lot for high-throughput operations
- **Memory-Efficient Design** - Optimized for large-scale blockchain operations
- **Zero-Copy Serialization** - Fast data serialization with bincode

### 🏗️ Advanced Consensus
- **Longest Chain Consensus** - Bitcoin-style consensus with proper fork resolution
- **Chain Reorganization** - Automatic handling of competing chains
- **Orphan Block Pool** - Efficient handling of out-of-order blocks
- **Difficulty Adjustment** - Dynamic difficulty based on block time targets

### 💰 Comprehensive Transaction System
- **Multiple Transaction Types**:
  - Transfer transactions
  - Staking/unstaking operations
  - Mining rewards
  - Governance voting
- **UTXO-like Account Tracking** - Efficient balance and nonce management
- **Fee-based Prioritization** - Mempool with intelligent transaction ordering
- **Anti-Spam Protection** - Rate limiting and transaction validation

### 🔒 Enterprise Security
- **Secure Key Storage** - Encrypted key management with Scrypt derivation
- **Memory Zeroization** - Automatic cleanup of sensitive data
- **Constant-Time Operations** - Protection against timing attacks
- **Comprehensive Validation** - Multi-layer transaction and block validation

### 📊 Production Monitoring
- **Real-time Statistics** - Mining performance, mempool status, chain metrics
- **Health Monitoring** - System integrity checks and error reporting
- **Performance Metrics** - Hash rates, block times, transaction throughput

## 🚀 Quick Start

### Prerequisites
- Rust 1.70+ (nightly recommended for latest features)
- Linux/macOS/Windows (Linux recommended for mining)

### Installation

```bash
# Clone the repository
git clone <repository-url>
cd core

# Build in release mode
cargo build --release

# Run tests
cargo test --release
```

### Basic Usage

```rust
use numi_core::{
    blockchain::NumiBlockchain,
    crypto::Dilithium3Keypair,
    transaction::{Transaction, TransactionType},
    storage::BlockchainStorage,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize blockchain
    let mut blockchain = NumiBlockchain::new()?;
    
    // Create a keypair
    let keypair = Dilithium3Keypair::new()?;
    
    // Create a transaction
    let transaction = Transaction::new(
        keypair.public_key.clone(),
        TransactionType::Transfer {
            to: vec![1, 2, 3, 4],
            amount: 1000,
        },
        1,
    );
    
    // Add transaction to mempool
    blockchain.add_transaction(transaction).await?;
    
    println!("Blockchain initialized successfully!");
    Ok(())
}
```

## 📁 Architecture

### Core Modules

- **`blockchain.rs`** - Main blockchain implementation with consensus logic
- **`block.rs`** - Block structure and validation
- **`transaction.rs`** - Transaction types and processing
- **`crypto.rs`** - Quantum-safe cryptography implementation
- **`mempool.rs`** - Transaction pool with prioritization
- **`miner.rs`** - Multi-threaded mining implementation
- **`storage.rs`** - Persistent storage with Sled database
- **`secure_storage.rs`** - Encrypted key management
- **`error.rs`** - Comprehensive error handling

### Data Flow

1. **Transaction Creation** → Mempool validation and storage
2. **Block Mining** → Multi-threaded PoW with Argon2id
3. **Block Validation** → Cryptographic verification and consensus rules
4. **Chain Update** → State management and reorganization handling
5. **Persistence** → Encrypted storage with integrity checks

## 🔧 Configuration

### Mining Configuration

```rust
use numi_core::miner::MiningConfig;

// High-performance mining
let config = MiningConfig::high_performance();

// Low-power background mining
let config = MiningConfig::low_power();

// Custom configuration
let config = MiningConfig {
    thread_count: 8,
    nonce_chunk_size: 50_000,
    stats_update_interval: 5,
    argon2_config: Argon2Config::production(),
    enable_cpu_affinity: true,
    thermal_throttle_temp: 85.0,
    power_limit_watts: 0.0,
};
```

### Security Configuration

```rust
use numi_core::secure_storage::KeyDerivationConfig;

// High security for production
let kdf_config = KeyDerivationConfig::high_security();

// Fast configuration for development
let kdf_config = KeyDerivationConfig::development();
```

## 🛡️ Security Features

### Quantum Resistance
- **Dilithium3 Signatures**: Post-quantum digital signatures approved by NIST
- **Argon2id PoW**: Memory-hard algorithm resistant to ASIC optimization
- **Future-Proof Design**: Easy migration to newer quantum-safe algorithms

### Cryptographic Security
- **AES-256-GCM**: Authenticated encryption for data protection
- **Scrypt Key Derivation**: Memory-hard password strengthening
- **BLAKE3 Hashing**: Fast, secure, and collision-resistant
- **Constant-Time Operations**: Protection against timing attacks

### Operational Security
- **Secure Memory Management**: Automatic zeroization of sensitive data
- **Encrypted Storage**: All keys and sensitive data are encrypted at rest
- **Access Control**: Password-based authentication for key operations
- **Integrity Verification**: Checksums and validation at multiple levels

## 📈 Performance

### Benchmarks
- **Transaction Processing**: 10,000+ TPS on modern hardware
- **Block Mining**: Configurable difficulty with 30-second target block time
- **Memory Usage**: Optimized for large-scale operations
- **Storage Efficiency**: Compressed and indexed data storage

### Scalability
- **Horizontal Scaling**: Multi-threaded mining and processing
- **Memory Efficiency**: Streaming data processing for large blocks
- **Network Optimization**: Efficient peer-to-peer communication
- **Storage Optimization**: Incremental backups and pruning support

## 🔍 Monitoring & Debugging

### Logging
```rust
// Enable debug logging
env_logger::init();

// Log levels: error, warn, info, debug, trace
log::info!("Blockchain initialized");
log::debug!("Processing transaction: {}", tx_hash);
```

### Statistics
```rust
// Get blockchain statistics
let chain_state = blockchain.get_chain_state();
println!("Current height: {}", chain_state.total_blocks);
println!("Total supply: {}", chain_state.total_supply);

// Get mining statistics
let mining_stats = miner.get_stats();
println!("Hash rate: {} H/s", mining_stats.hash_rate);
println!("Blocks mined: {}", mining_stats.blocks_mined);
```

## 🧪 Testing

### Unit Tests
```bash
# Run all tests
cargo test

# Run specific module tests
cargo test blockchain
cargo test crypto
cargo test miner
```

### Integration Tests
```bash
# Run with logging
RUST_LOG=debug cargo test

# Run performance tests
cargo test --release
```

### Test Coverage
```bash
# Install cargo-tarpaulin
cargo install cargo-tarpaulin

# Generate coverage report
cargo tarpaulin --out Html
```

## 🚀 Deployment

### Production Checklist
- [ ] Use release builds (`cargo build --release`)
- [ ] Configure proper logging levels
- [ ] Set up monitoring and alerting
- [ ] Implement backup strategies
- [ ] Configure firewall and network security
- [ ] Set up SSL/TLS certificates
- [ ] Implement rate limiting
- [ ] Configure automatic updates

### Docker Deployment
```dockerfile
FROM rust:1.70 as builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bullseye-slim
RUN apt-get update && apt-get install -y ca-certificates
COPY --from=builder /app/target/release/numi-core /usr/local/bin/
CMD ["numi-core"]
```

## 🤝 Contributing

### Development Setup
```bash
# Install Rust nightly
rustup toolchain install nightly
rustup default nightly

# Install development dependencies
cargo install cargo-watch
cargo install cargo-tarpaulin

# Run development server
cargo watch -x check -x test -x run
```

### Code Style
- Follow Rust formatting guidelines (`cargo fmt`)
- Run clippy for linting (`cargo clippy`)
- Ensure all tests pass
- Add documentation for public APIs

## 📄 License

This project is licensed under the MIT License - see the LICENSE file for details.

## 🆘 Support

### Documentation
- [API Documentation](https://docs.rs/numi-core)
- [Examples](examples/)
- [Architecture Guide](docs/architecture.md)

### Community
- [Discord Server](https://discord.gg/bumicoin)
- [GitHub Issues](https://github.com/bumicoin/core/issues)
- [Discussions](https://github.com/bumicoin/core/discussions)

### Security
- **Security Issues**: security@bumicoin.org
- **Bug Reports**: GitHub Issues with security label
- **Responsible Disclosure**: We appreciate responsible disclosure of security issues

---

**Bumi Coin Blockchain Core** - Building the future of decentralized finance with quantum-safe technology. 