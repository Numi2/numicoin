use std::fmt;

#[derive(Debug, Clone)]
pub enum BlockchainError {
    InvalidBlock(String),
    InvalidTransaction(String),
    StorageError(String),
    NetworkError(String),
    ConsensusError(String),
    CryptographyError(String), // AI Agent Note: Added for crypto operations and key management
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
        }
    }
}

impl std::error::Error for BlockchainError {} 