pub mod block;
pub mod blockchain;
pub mod config;
pub mod crypto;
pub mod error;
pub mod mempool;
pub mod miner;
pub mod local_miner;
pub mod mining_service;
pub mod network;
pub mod rpc;
pub mod secure_storage;
pub mod storage;
pub mod stratum_server;
pub mod transaction;
pub mod sync_lock;

pub use block::{Block, BlockHeader};
pub use transaction::{Transaction, TransactionType};
pub use crypto::{Dilithium3Keypair, Dilithium3Signature, Hash};
pub use blockchain::NumiBlockchain;
pub use error::{BlockchainError, RpcError};
pub use rpc::RpcServer;
pub use mempool::TransactionMempool;
pub use secure_storage::SecureKeyStore;
pub use config::{Config, NetworkConfig, MiningConfig, RpcConfig, SecurityConfig};
pub use mining_service::MiningService;

// Re-export the Tokio-backed RwLock so downstream crates can `use numi_core::RwLock`.
pub use sync_lock::RwLock;

pub type Result<T> = std::result::Result<T, BlockchainError>;
