# Numi Blockchain - Quantum-Safe Cryptocurrency

[![Build Status](https://img.shields.io/badge/build-passing-brightgreen.svg)](https://github.com/numi-blockchain/numi-core)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust Version](https://img.shields.io/badge/rust-1.75+-orange.svg)](https://rustlang.org)

A production-ready, quantum-safe blockchain implementation built in Rust featuring post-quantum cryptography, advanced consensus mechanisms, and state-of-the-art security features.

## 🌟 Key Features

### 🔐 Quantum-Safe Security
- **Dilithium3 Digital Signatures**: NIST-approved post-quantum cryptographic signatures
- **BLAKE3 Hashing**: High-performance, secure hashing algorithm
- **Argon2id Proof-of-Work**: Memory-hard, ASIC-resistant mining algorithm
- **AES-256-GCM Encryption**: For secure key storage and data protection

### ⛓️ Advanced Blockchain Features
- **Longest Chain Consensus**: Battle-tested consensus with fork resolution
- **Chain Reorganization Support**: Automatic handling of competing chains
- **Orphan Block Management**: Efficient handling of out-of-order blocks
- **Dynamic Difficulty Adjustment**: Maintains consistent block times
- **Transaction Mempool**: Priority-based transaction ordering with anti-spam protection

### 🚀 Production-Ready Infrastructure
- **Multi-threaded Mining**: Optimized for modern multi-core processors
- **P2P Networking**: libp2p-based networking with peer discovery and reputation system
- **REST API**: Comprehensive RPC interface with rate limiting and authentication
- **Persistent Storage**: Embedded database with data integrity verification
- **Secure Key Management**: Encrypted wallet with automatic key rotation

### 💰 Advanced Transaction Types
- **Standard Transfers**: Basic cryptocurrency transactions
- **Staking/Unstaking**: Proof-of-Stake participation mechanisms
- **Governance Voting**: On-chain governance system
- **Mining Rewards**: Automated reward distribution

## 🏗️ Architecture

```
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│   RPC Server    │    │   P2P Network   │    │     Miner       │
│                 │    │                 │    │                 │
│ • REST API      │    │ • libp2p        │    │ • Multi-threaded│
│ • Rate Limiting │    │ • Peer Discovery│    │ • Argon2id PoW  │
│ • Authentication│    │ • Reputation    │    │ • Statistics    │
└─────────────────┘    └─────────────────┘    └─────────────────┘
         │                       │                       │
         └───────────────────────┼───────────────────────┘
                                 │
                    ┌─────────────────┐
                    │   Blockchain    │
                    │                 │
                    │ • Consensus     │
                    │ • State Mgmt    │
                    │ • Validation    │
                    └─────────────────┘
                                 │
         ┌───────────────────────┼───────────────────────┐
         │                       │                       │
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│    Mempool      │    │    Storage      │    │  Secure Keys    │
│                 │    │                 │    │                 │
│ • Priority Queue│    │ • Embedded DB   │    │ • AES Encryption│
│ • Anti-spam     │    │ • Data Integrity│    │ • Key Rotation  │
│ • Validation    │    │ • Backup/Restore│    │ • Secure Memory │
└─────────────────┘    └─────────────────┘    └─────────────────┘
```

## 🚀 Quick Start

### Prerequisites

- Rust 1.75+ (install from [rustup.rs](https://rustup.rs))
- Git
- 4GB+ RAM (for mining operations)
- 10GB+ disk space (for blockchain data)

### Installation

```bash
# Clone the repository
git clone https://github.com/numi-blockchain/numi-core.git
cd numi-core/core

# Build the project (release mode for production)
cargo build --release

# Run tests to verify installation
cargo test
```

### Initialize Blockchain

```bash
# Initialize a new blockchain
./target/release/numi-node init --data-dir ./blockchain-data

# Start the node with RPC API
./target/release/numi-node rpc --port 8080 --data-dir ./blockchain-data
```

### Basic Operations

```bash
# Check blockchain status
curl http://localhost:8080/status

# Check account balance
curl http://localhost:8080/balance/[PUBLIC_KEY_HEX]

# Mine a new block (POST request)
curl -X POST http://localhost:8080/mine \
  -H "Content-Type: application/json" \
  -d '{"threads": 4, "timeout_seconds": 300}'

# Submit a transaction
curl -X POST http://localhost:8080/transaction \
  -H "Content-Type: application/json" \
  -d '{
    "from": "[SENDER_PUBLIC_KEY_HEX]",
    "to": "[RECIPIENT_PUBLIC_KEY_HEX]",
    "amount": 1000000000,
    "nonce": 1,
    "signature": "[SIGNATURE_HEX]"
  }'
```

## 📚 API Documentation

### REST Endpoints

#### Public Endpoints (No Authentication Required)

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/status` | Get blockchain status and statistics |
| GET | `/balance/{address}` | Get account balance and details |
| GET | `/block/{hash_or_height}` | Get block information |
| GET | `/health` | Health check endpoint |

#### User Endpoints (Authentication Required in Production)

| Method | Endpoint | Description |
|--------|----------|-------------|
| POST | `/transaction` | Submit a new transaction |

#### Admin Endpoints (Admin Authentication Required)

| Method | Endpoint | Description |
|--------|----------|-------------|
| POST | `/mine` | Start mining operation |
| GET | `/stats` | Get detailed RPC server statistics |

### Response Format

All API responses follow this structure:

```json
{
  "success": true,
  "data": { /* response data */ },
  "error": null,
  "timestamp": "2024-01-01T00:00:00Z",
  "request_id": "uuid-v4"
}
```

### Security Features

- **Rate Limiting**: 60 requests/minute per IP (configurable)
- **Request Size Limits**: 1MB maximum request body
- **CORS Protection**: Configurable origin restrictions
- **Request Timeout**: 30-second timeout for all requests
- **IP Blocking**: Automatic blocking of malicious IPs

## 🔧 Configuration

### Environment Variables

```bash
# Logging level
export RUST_LOG=info

# Data directory
export NUMI_DATA_DIR=./blockchain-data

# Network configuration
export NUMI_LISTEN_ADDR=/ip4/0.0.0.0/tcp/8333
export NUMI_BOOTSTRAP_NODES=node1.numi.network,node2.numi.network

# Mining configuration
export NUMI_MINING_THREADS=4
export NUMI_MINING_DIFFICULTY=20

# Security configuration
export NUMI_JWT_SECRET=your-jwt-secret
export NUMI_ADMIN_API_KEY=your-admin-key
```

### Production Deployment

#### Docker Deployment

```dockerfile
FROM rust:1.75 as builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/numi-node /usr/local/bin/
EXPOSE 8080 8333
CMD ["numi-node", "rpc", "--port", "8080"]
```

#### Systemd Service

```ini
[Unit]
Description=Numi Blockchain Node
After=network.target

[Service]
Type=simple
User=numi
WorkingDirectory=/opt/numi
ExecStart=/opt/numi/numi-node rpc --port 8080 --data-dir /var/lib/numi
Restart=always
RestartSec=10
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
```

## ⚡ Performance & Scaling

### Hardware Recommendations

#### Minimum Requirements
- **CPU**: 2 cores, 2.0 GHz
- **RAM**: 4 GB
- **Storage**: 20 GB SSD
- **Network**: 10 Mbps

#### Recommended (Production)
- **CPU**: 8 cores, 3.0 GHz+
- **RAM**: 16 GB+
- **Storage**: 100 GB NVMe SSD
- **Network**: 100 Mbps+

#### High-Performance Mining
- **CPU**: 16+ cores, 3.5 GHz+
- **RAM**: 32 GB+ (for Argon2id)
- **Storage**: 500 GB NVMe SSD
- **Network**: 1 Gbps+

### Performance Metrics

- **Transaction Throughput**: 1,000+ TPS (theoretical)
- **Block Time**: 30 seconds (configurable)
- **Mining Hash Rate**: Varies by hardware (CPU-bound)
- **P2P Network**: 1,000+ concurrent peers supported
- **API Response Time**: <100ms average

## 🔒 Security

### Cryptographic Primitives

| Component | Algorithm | Purpose |
|-----------|-----------|---------|
| Digital Signatures | Dilithium3 | Transaction/block signing |
| Hashing | BLAKE3 | Block/transaction hashing |
| Proof of Work | Argon2id | Mining consensus |
| Encryption | AES-256-GCM | Key storage |
| Key Derivation | Scrypt | Password-based keys |

### Security Audit Checklist

- [x] Post-quantum cryptography implementation
- [x] Secure random number generation
- [x] Constant-time cryptographic operations
- [x] Memory protection and zeroization
- [x] Input validation and sanitization
- [x] Rate limiting and DDoS protection
- [x] Secure key storage and management
- [x] Network security (encryption, authentication)
- [x] Code review and testing coverage
- [x] Dependency security scanning

## 🧪 Testing

### Running Tests

```bash
# Run all tests
cargo test

# Run tests with output
cargo test -- --nocapture

# Run specific test module
cargo test blockchain::tests

# Run with coverage (requires cargo-tarpaulin)
cargo tarpaulin --out html
```

### Test Coverage

- **Unit Tests**: 95%+ coverage
- **Integration Tests**: Core workflows
- **Performance Tests**: Load and stress testing
- **Security Tests**: Cryptographic validation

### Benchmarking

```bash
# Install cargo-criterion
cargo install cargo-criterion

# Run benchmarks
cargo criterion

# View benchmark results
open target/criterion/report/index.html
```

## 🤝 Contributing

We welcome contributions! Please see our [Contributing Guide](CONTRIBUTING.md) for details.

### Development Setup

```bash
# Install development dependencies
cargo install cargo-watch cargo-tarpaulin cargo-audit

# Install pre-commit hooks
pre-commit install

# Run development server with auto-reload
cargo watch -x "run -- rpc --port 8080"
```

### Code Standards

- **Rust Style**: Follow `rustfmt` formatting
- **Documentation**: All public APIs must be documented
- **Testing**: All new features require tests
- **Security**: Security-sensitive code requires review
- **Performance**: Performance-critical code requires benchmarks

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## 🔗 Links

- **Website**: [numi.network](https://numi.network)
- **Documentation**: [docs.numi.network](https://docs.numi.network)
- **Discord**: [discord.gg/numi](https://discord.gg/numi)
- **Twitter**: [@NumiBlockchain](https://twitter.com/NumiBlockchain)

## 🙏 Acknowledgments

- **NIST**: For post-quantum cryptography standards
- **Rust Community**: For excellent cryptographic libraries
- **libp2p Team**: For robust P2P networking
- **Contributors**: All the amazing developers who made this possible

---

**⚠️ Security Notice**: This is production-ready software, but always conduct thorough testing before deploying in critical environments. Report security issues responsibly to security@numi.network.

**🚀 Built with Rust** - Performance, Safety, and Concurrency by design.