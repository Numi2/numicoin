use std::fmt;

#[derive(Debug, Clone)]
pub enum BlockchainError {
    InvalidBlock(String),
    InvalidTransaction(String),
    StorageError(String),
    NetworkError(String),
    ConsensusError(String),
    CryptographyError(String),
    // AI Agent Note: Added missing error variants for production readiness
    SerializationError(String),
    InvalidSignature(String),
    InvalidNonce { expected: u64, found: u64 },
    InsufficientBalance(String),
    BlockNotFound(String),
    MiningError(String),
}

impl fmt::Display for BlockchainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BlockchainError::InvalidBlock(msg) => write!(f, "Invalid block: {}", msg),
            BlockchainError::InvalidTransaction(msg) => write!(f, "Invalid transaction: {}", msg),
            BlockchainError::StorageError(msg) => write!(f, "Storage error: {}", msg),
            BlockchainError::NetworkError(msg) => write!(f, "Network error: {}", msg),
            BlockchainError::ConsensusError(msg) => write!(f, "Consensus error: {}", msg),
            BlockchainError::CryptographyError(msg) => write!(f, "Cryptography error: {}", msg),
            BlockchainError::SerializationError(msg) => write!(f, "Serialization error: {}", msg),
            BlockchainError::InvalidSignature(msg) => write!(f, "Invalid signature: {}", msg),
            BlockchainError::InvalidNonce { expected, found } => write!(f, "Invalid nonce: expected {}, found {}", expected, found),
            BlockchainError::InsufficientBalance(msg) => write!(f, "Insufficient balance: {}", msg),
            BlockchainError::BlockNotFound(msg) => write!(f, "Block not found: {}", msg),
            BlockchainError::MiningError(msg) => write!(f, "Mining error: {}", msg),
        }
    }
}

impl std::error::Error for BlockchainError {} 