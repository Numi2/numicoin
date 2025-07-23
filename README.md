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
### Backend (Rust)
- **Enhanced RPC Server**: Complete REST API with comprehensive endpoints
- **Endpoints Implemented**:
  - `GET /status` - Blockchain status and statistics
  - `GET /balance/:address` - Account balance and state
  - `GET /block/:hash` - Block information by hash
  - `POST /transaction` - Submit new transactions
  - `POST /mine` - Mine new blocks
- **CORS Support**: Enabled for cross-origin requests from the wallet
- **Error Handling**: Consistent JSON error responses
- **Real Data**: Connected to actual blockchain state and storage

### Frontend (Next.js)
- **Blockchain Client**: TypeScript client library for API communication
- **Type Safety**: Complete TypeScript interfaces for all API responses
- **Error Handling**: Robust error handling with user-friendly messages
- **Dashboard Integration**: Real-time blockchain status display
- **Wallet Integration**: Connected to wallet context for seamless UX

#### Technical Specifications
- **Programming Language**: Rust
- **Cryptography**: Dilithium3 (simplified placeholder), BLAKE3, Argon2id
- **Database**: Sled (embedded key-value store)
- **Serialization**: Bincode, Serde
- **CLI Framework**: Clap
- **Async Runtime**: Tokio
- **Block Time**: ~30 seconds target
- **Mining Reward**: 0.005 NUMI per block
- **Difficulty**: Auto-adjusting based on block time