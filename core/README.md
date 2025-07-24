# Numi Core - Rust Blockchain Implementation

A production-ready, quantum-safe blockchain implementation written in Rust with comprehensive security features, P2P networking, and advanced consensus mechanisms.

## Project Overview

Numi Core is a complete blockchain implementation featuring:
- **Quantum-safe cryptography** using Dilithium3 signatures
- **Proof-of-Work consensus** with Argon2id hashing
- **P2P networking** via libp2p with pub/sub messaging
- **RESTful RPC API** with authentication and rate limiting
- **Secure key management** with encrypted storage
- **Advanced mempool** with fee-based prioritization
- **Comprehensive testing** and production hardening

## Module Architecture

### Core Modules

#### `lib.rs` - Module Exports
- **Purpose**: Main library entry point and public API exports
- **Key Functions**: 
  - Exports all public modules and types
  - Defines `Result<T>` type alias for error handling
  - Provides clean public interface for external consumers

#### `main.rs` - CLI Application (853 lines)
- **Purpose**: Command-line interface and application entry point
- **Key Commands**:
  - `start` - Launch full blockchain node
  - `mine` - Mine single block
  - `submit` - Submit transaction to network
  - `status` - Show blockchain status
  - `balance` - Query account balance
  - `init` - Initialize new blockchain
  - `rpc` - Start RPC server only
  - `generate-key` - Create new keypair
  - `backup/restore` - Data backup operations
- **Features**: 
  - Comprehensive CLI with subcommands
  - Configuration file support
  - Environment variable overrides
  - Data directory locking for single instance

#### `block.rs` - Block Structure (307 lines)
- **Purpose**: Block and block header definitions
- **Key Structures**:
  - `BlockHeader`: Version, height, timestamp, previous hash, merkle root, difficulty, nonce, miner key, signature
  - `Block`: Header + transactions vector
- **Key Functions**:
  - `calculate_hash()` - Blake3 hash of block header
  - `calculate_merkle_root()` - Merkle tree construction
  - `sign()` / `verify_signature()` - Block signing with Dilithium3
  - `validate()` - Comprehensive block validation
  - `is_genesis()` - Genesis block detection

#### `transaction.rs` - Transaction System (1016 lines)
- **Purpose**: Transaction types, validation, and processing
- **Transaction Types**:
  - `Transfer` - Standard value transfer
  - `Stake` / `Unstake` - Proof-of-Stake operations
  - `MiningReward` - Block reward distribution
  - `Governance` - Voting and proposals
  - `ContractDeploy` / `ContractCall` - Smart contract support
- **Key Functions**:
  - `sign()` / `verify_signature()` - Transaction signing
  - `validate()` - Comprehensive validation
  - `calculate_fee()` - Dynamic fee calculation
  - `get_priority_score()` - Mempool prioritization
  - `is_expired()` - Transaction expiry checking

#### `crypto.rs` - Cryptography (1102 lines)
- **Purpose**: Quantum-safe cryptography and hashing
- **Key Components**:
  - `Dilithium3Keypair` - Post-quantum signature keypair
  - `Dilithium3Signature` - Quantum-safe signatures
  - `Argon2Config` - Proof-of-Work configuration
- **Key Functions**:
  - `blake3_hash()` - Fast cryptographic hashing
  - `argon2id_pow_hash()` - Memory-hard PoW hashing
  - `verify_pow()` - Proof-of-Work verification
  - `generate_difficulty_target()` - Difficulty adjustment
  - `derive_key()` - Key derivation functions
  - `batch_verify_signatures()` - Optimized batch verification

#### `blockchain.rs` - Core Blockchain Logic (2198 lines)
- **Purpose**: Main blockchain state management and consensus
- **Key Structures**:
  - `NumiBlockchain` - Main blockchain instance
  - `ChainState` - Current chain statistics
  - `AccountState` - Account balances and metadata
  - `SecurityCheckpoint` - Long-range attack prevention
  - `ForkInfo` - Chain reorganization data
- **Key Functions**:
  - `add_block()` - Block processing and validation
  - `reorganize_to_block()` - Chain reorganization
  - `validate_block_comprehensive()` - Full block validation
  - `apply_transaction()` / `undo_transaction()` - State changes
  - `calculate_next_difficulty()` - Difficulty adjustment
  - `detect_long_range_attack()` - Security monitoring

#### `miner.rs` - Mining Implementation (697 lines)
- **Purpose**: Proof-of-Work mining with multi-threading
- **Key Structures**:
  - `Miner` - Mining coordinator
  - `MiningStats` - Performance metrics
  - `MiningConfig` - Configurable parameters
  - `MiningResult` - Mining output
- **Key Functions**:
  - `mine_block()` - Multi-threaded block mining
  - `mining_thread_worker()` - Individual mining threads
  - `pause()` / `resume()` - Mining control
  - `estimate_block_time()` - Mining time estimation
  - `get_stats()` - Performance monitoring

#### `network.rs` - P2P Networking (500 lines)
- **Purpose**: Peer-to-peer network communication
- **Key Components**:
  - `NetworkManager` - Network coordination
  - `NumiBehaviour` - libp2p behavior implementation
  - `PeerInfo` - Peer reputation tracking
- **Key Functions**:
  - `start()` - Network initialization
  - `broadcast_block()` / `broadcast_transaction()` - Message broadcasting
  - `handle_swarm_event()` - Network event processing
  - `update_peer_reputation()` - Peer management
  - `perform_maintenance()` - Network cleanup

#### `rpc.rs` - REST API Server (1125 lines)
- **Purpose**: HTTP REST API with security features
- **Key Features**:
  - Rate limiting per IP address
  - JWT authentication
  - CORS policy enforcement
  - Request/response logging
  - Input validation and sanitization
- **Key Endpoints**:
  - `GET /status` - Blockchain status
  - `GET /balance/{address}` - Account balance
  - `GET /block/{hash}` - Block information
  - `POST /transaction` - Submit transaction
  - `POST /mine` - Manual mining
  - `GET /stats` - API statistics

#### `storage.rs` - Data Persistence (515 lines)
- **Purpose**: Blockchain data storage using Sled database
- **Key Functions**:
  - `save_block()` / `load_block()` - Block persistence
  - `save_transaction()` / `load_transaction()` - Transaction storage
  - `save_account()` / `load_account()` - Account state
  - `save_checkpoints()` - Security checkpoint storage
  - `backup()` / `restore()` - Data backup operations
  - `compact()` / `flush()` - Database maintenance

#### `mempool.rs` - Transaction Pool (611 lines)
- **Purpose**: Pending transaction management
- **Key Features**:
  - Fee-based prioritization using BTreeMap
  - Account nonce validation
  - Memory size limits and LRU eviction
  - Double-spend detection
  - Anti-spam rate limiting
- **Key Functions**:
  - `add_transaction()` - Transaction admission
  - `get_transactions_for_block()` - Block candidate selection
  - `cleanup_expired_transactions()` - Maintenance
  - `validate_transaction()` - Comprehensive validation

#### `secure_storage.rs` - Encrypted Key Management (759 lines)
- **Purpose**: Secure key storage with encryption
- **Security Features**:
  - AES-256-GCM encryption
  - Scrypt key derivation
  - Secure memory management
  - Key versioning and migration
  - Time-based key rotation
- **Key Functions**:
  - `store_keypair()` / `get_keypair()` - Key operations
  - `initialize()` - Secure store setup
  - `verify_integrity()` - Data integrity checking
  - `create_backup()` - Encrypted backup

#### `config.rs` - Configuration Management (699 lines)
- **Purpose**: Comprehensive configuration system
- **Configuration Sections**:
  - `NetworkConfig` - P2P network settings
  - `MiningConfig` - Mining parameters
  - `RpcConfig` - API server settings
  - `SecurityConfig` - Security policies
  - `StorageConfig` - Data storage options
  - `ConsensusConfig` - Consensus parameters
- **Key Functions**:
  - `load_from_file()` / `save_to_file()` - Configuration persistence
  - `apply_env_overrides()` - Environment variable support
  - `validate()` - Configuration validation

#### `error.rs` - Error Handling (100 lines)
- **Purpose**: Centralized error types and conversions
- **Error Types**:
  - `BlockchainError` - Main error enum
  - Comprehensive error variants for all operations
  - Automatic `From` implementations for external errors
- **Features**: 
  - Structured error messages
  - Error conversion traits
  - Integration with `thiserror` crate

## Dependencies

### Core Dependencies
- **Cryptography**: `blake3`, `argon2`, `pqcrypto-dilithium`, `aes-gcm`
- **Networking**: `libp2p`, `tokio`, `warp`
- **Storage**: `sled`, `serde`, `bincode`
- **Utilities**: `chrono`, `uuid`, `anyhow`, `thiserror`
- **Performance**: `rayon`, `parking_lot`, `dashmap`

### Development Dependencies
- **Testing**: `criterion`, `tempfile`, `proptest`
- **CLI**: `clap`
- **Logging**: `log`, `env_logger`

## Security Features

### Quantum-Safe Cryptography
- Dilithium3 post-quantum signatures
- Blake3 for fast, secure hashing
- Argon2id for memory-hard Proof-of-Work

### Network Security
- Rate limiting and DoS protection
- Peer reputation system
- Message validation and sanitization
- Long-range attack prevention via checkpoints

### Storage Security
- Encrypted key storage with AES-256-GCM
- Secure memory management with zeroization
- Data integrity verification
- Atomic file operations

### API Security
- JWT-based authentication
- IP-based rate limiting
- CORS policy enforcement
- Input validation and sanitization

## Performance Features

### Concurrency
- Multi-threaded mining with Rayon
- Async/await throughout the codebase
- Lock-free data structures with `dashmap`
- Efficient RwLock usage with `parking_lot`

### Optimization
- Fee-based transaction prioritization
- Block validation caching
- Incremental hashing with Blake3
- Memory-efficient data structures

### Monitoring
- Comprehensive statistics collection
- Performance metrics tracking
- Real-time mining statistics
- Network health monitoring

## Usage Examples

### Starting a Node
```bash
# Start full node with default config
numi-node start

# Start with custom configuration
numi-node start --config custom.toml --enable-mining

# Start RPC server only
numi-node rpc --port 8080
```

### Mining Operations
```bash
# Mine a single block
numi-node mine --threads 4 --timeout 300

# Generate new keypair
numi-node generate-key --output miner.key
```

### Transaction Operations
```bash
# Submit transaction
numi-node submit --from-key sender.key --to recipient --amount 1000000

# Check balance
numi-node balance --address <hex-address>
```

## Development

### Building
```bash
cargo build --release
```

### Testing
```bash
cargo test
cargo bench  # Performance benchmarks
```

### Configuration
The system supports multiple configuration environments:
- **Development**: Relaxed settings for testing
- **Production**: Hardened security settings
- **Custom**: User-defined configurations

## Architecture Highlights

### Consensus Mechanism
- Proof-of-Work with Argon2id
- Dynamic difficulty adjustment
- Fork choice based on cumulative difficulty
- Finality through checkpoint system

### Network Protocol
- libp2p-based P2P networking
- Pub/sub messaging for blocks and transactions
- mDNS for local peer discovery
- Floodsub for message propagation

### State Management
- Account-based state model
- Merkle tree for transaction verification
- Checkpoint system for security
- Efficient state transitions

This implementation provides a complete, production-ready blockchain with advanced security features, high performance, and comprehensive testing.
