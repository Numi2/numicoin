use thiserror::Error;
use sled::transaction::UnabortableTransactionError;

#[derive(Debug, Clone, Error)]
pub enum InvalidBlockError {
    #[error("Block signature verification failed")]
    SignatureVerificationFailed,
    #[error("Previous block hash mismatch")]
    PreviousBlockHashMismatch,
    #[error("Invalid block height")]
    InvalidBlockHeight,
    #[error("Genesis block must have height 0")]
    GenesisBlockHeightNotZero,
    #[error("Genesis block previous_hash must be zero")]
    GenesisBlockHashNotZero,
    #[error("Genesis block must have exactly one transaction")]
    GenesisBlockInvalidTransactionCount,
    #[error("Genesis block's only transaction must be a mining reward")]
    GenesisBlockTransactionNotReward,
    #[error("Invalid number of mining reward transactions in block")]
    InvalidRewardTransactionCount,
    #[error("Incorrect mining reward amount")]
    InvalidRewardAmount,
    #[error("Mining reward transaction must be first in the block")]
    RewardTransactionNotFirst,
    #[error("Invalid Merkle root")]
    InvalidMerkleRoot,
    #[error("Block timestamp is outside the allowed range: {0}")]
    TimestampOutOfRange(String),
    #[error("Invalid PoW")]
    InvalidPoW,
    #[error("The block is stale and does not connect to the main chain")]
    StaleChain,
    #[error("Invalid transaction in block: {0}")]
    InvalidTransaction(String),
}

#[derive(Debug, Clone, Error)]
pub enum BlockchainError {
    #[error("Invalid block: {0}")]
    InvalidBlock(#[from] InvalidBlockError),

    #[error("Invalid transaction: {0}")]
    InvalidTransaction(String),

    #[error("Storage error: {0}")]
    StorageError(String),

    #[error("Network error: {0}")]
    NetworkError(String),

    #[error("Consensus error: {0}")]
    ConsensusError(String),

    #[error("Cryptography error: {0}")]
    CryptographyError(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Invalid signature: {0}")]
    InvalidSignature(String),

    #[error("Invalid nonce: expected {expected}, found {found}")]
    InvalidNonce { expected: u64, found: u64 },

    #[error("Insufficient balance: {0}")]
    InsufficientBalance(String),

    #[error("Block not found: {0}")]
    BlockNotFound(String),
    #[error("Peer not found")]
    PeerNotFound,

    #[error("Mining error: {0}")]
    MiningError(String),

    #[error("Invalid argument: {0}")]
    InvalidArgument(String),

    #[error("Invalid backup: {0}")]
    InvalidBackup(String),

    #[error("IO error: {0}")]
    IoError(String),

    #[error("Task join error: {0}")]
    TaskJoinError(String),

    #[error("Missing genesis block")]
    MissingGenesisBlock,
}

#[derive(Debug, Clone, Error)]
pub enum RpcError {
    #[error("API key verification failed")]
    ApiKeyVerificationFailed,
}

#[derive(Debug, Clone, Error)]
pub enum MiningServiceError {
    #[error("Miner wallet not found: {0}")]
    WalletNotFound(String),
    #[error("Miner initialization failed: {0}")]
    MinerInitialization(String),
    #[error("Mining error: {0}")]
    MiningError(String),
}

impl From<MiningServiceError> for BlockchainError {
    fn from(e: MiningServiceError) -> Self {
        BlockchainError::MiningError(e.to_string())
    }
}

impl From<BlockchainError> for MiningServiceError {
    fn from(e: BlockchainError) -> Self {
        MiningServiceError::MiningError(e.to_string())
    }
}

// The `thiserror::Error` derive automatically implements `std::error::Error` and
// `fmt::Display`, so the manual implementations are no longer necessary.

// Existing `From` conversions are kept for convenience and to minimise refactor scope.
// Add From implementations for error conversions
impl From<std::io::Error> for BlockchainError {
    fn from(err: std::io::Error) -> Self {
        BlockchainError::IoError(err.to_string())
    }
}

impl From<serde_json::Error> for BlockchainError {
    fn from(err: serde_json::Error) -> Self {
        BlockchainError::SerializationError(err.to_string())
    }
}

impl From<bincode::Error> for BlockchainError {
    fn from(err: bincode::Error) -> Self {
        BlockchainError::SerializationError(err.to_string())
    }
}

impl From<toml::de::Error> for BlockchainError {
    fn from(err: toml::de::Error) -> Self {
        BlockchainError::SerializationError(err.to_string())
    }
}

impl From<toml::ser::Error> for BlockchainError {
    fn from(err: toml::ser::Error) -> Self {
        BlockchainError::SerializationError(err.to_string())
    }
}

impl From<std::path::StripPrefixError> for BlockchainError {
    fn from(err: std::path::StripPrefixError) -> Self {
        BlockchainError::InvalidArgument(err.to_string())
    }
}

// Convert tokio JoinError into our error type
impl From<tokio::task::JoinError> for BlockchainError {
    fn from(err: tokio::task::JoinError) -> Self {
        BlockchainError::TaskJoinError(err.to_string())
    }
}

impl From<UnabortableTransactionError> for BlockchainError {
    fn from(err: UnabortableTransactionError) -> Self {
        BlockchainError::StorageError(err.to_string())
    }
}