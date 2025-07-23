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
    InvalidArgument(String),
    InvalidBackup(String),
    IoError(String),
}

impl fmt::Display for BlockchainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BlockchainError::InvalidBlock(msg) => write!(f, "Invalid block: {msg}"),
            BlockchainError::InvalidTransaction(msg) => write!(f, "Invalid transaction: {msg}"),
            BlockchainError::StorageError(msg) => write!(f, "Storage error: {msg}"),
            BlockchainError::NetworkError(msg) => write!(f, "Network error: {msg}"),
            BlockchainError::ConsensusError(msg) => write!(f, "Consensus error: {msg}"),
            BlockchainError::CryptographyError(msg) => write!(f, "Cryptography error: {msg}"),
            BlockchainError::SerializationError(msg) => write!(f, "Serialization error: {msg}"),
            BlockchainError::InvalidSignature(msg) => write!(f, "Invalid signature: {msg}"),
            BlockchainError::InvalidNonce { expected, found } => write!(f, "Invalid nonce: expected {expected}, found {found}"),
            BlockchainError::InsufficientBalance(msg) => write!(f, "Insufficient balance: {msg}"),
            BlockchainError::BlockNotFound(msg) => write!(f, "Block not found: {msg}"),
            BlockchainError::MiningError(msg) => write!(f, "Mining error: {msg}"),
            BlockchainError::InvalidArgument(msg) => write!(f, "Invalid argument: {msg}"),
            BlockchainError::InvalidBackup(msg) => write!(f, "Invalid backup: {msg}"),
            BlockchainError::IoError(msg) => write!(f, "IO error: {msg}"),
        }
    }
}

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

impl std::error::Error for BlockchainError {} 