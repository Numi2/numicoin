use std::time::Duration;
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use crate::mempool::ValidationResult;

/// Rate limiting configuration
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    pub requests_per_minute: u32,
    pub burst_size: u32,
    pub cleanup_interval: Duration,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            requests_per_minute: 60,    // 60 requests per minute
            burst_size: 10,             // Allow bursts of 10 requests
            cleanup_interval: Duration::from_secs(300), // Cleanup every 5 minutes
        }
    }
}

impl RateLimitConfig {
    pub fn production() -> Self {
        Self {
            requests_per_minute: 100,
            burst_size: 20,
            cleanup_interval: Duration::from_secs(300),
        }
    }
    
    pub fn development() -> Self {
        Self {
            requests_per_minute: 1000,  // More lenient for development
            burst_size: 100,
            cleanup_interval: Duration::from_secs(60),
        }
    }
}

/// Authentication configuration
#[derive(Debug, Clone)]
pub struct AuthConfig {
    pub jwt_secret: String,
    pub token_expiry: Duration,
    pub require_auth: bool,
    pub admin_api_key: String,
}

impl Default for AuthConfig {
    fn default() -> Self {
        let generate_secret = || {
            use rand::RngCore;
            let mut rng = rand::rngs::OsRng;
            let mut bytes = [0u8; 32];
            rng.fill_bytes(&mut bytes);
            hex::encode(bytes)
        };

        Self {
            jwt_secret: std::env::var("NUMI_JWT_SECRET").unwrap_or_else(|_| generate_secret()),
            token_expiry: Duration::from_secs(3600),
            require_auth: true,
            admin_api_key: std::env::var("NUMI_ADMIN_KEY").unwrap_or_else(|_| generate_secret()),
        }
    }
}

/// API endpoint access levels
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessLevel {
    Public,     // No authentication required
    User,       // Basic user authentication required
    Admin,      // Admin authentication required
}

/// RPC server statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcStats {
    pub total_requests: u64,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub rate_limited_requests: u64,
    pub active_connections: u32,
    pub blocked_ips: u32,
    pub uptime_seconds: u64,
    pub average_response_time_ms: f64,
}

/// API response wrapper with security headers
#[derive(Debug, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub request_id: String,
}

impl<T> ApiResponse<T> {
    pub fn success(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
            timestamp: chrono::Utc::now(),
            request_id: uuid::Uuid::new_v4().to_string(),
        }
    }
    
    pub fn error(message: String) -> ApiResponse<()> {
        ApiResponse {
            success: false,
            data: None,
            error: Some(message),
            timestamp: chrono::Utc::now(),
            request_id: uuid::Uuid::new_v4().to_string(),
        }
    }
}

/// Blockchain status response
#[derive(Debug, Serialize, Deserialize)]
pub struct StatusResponse {
    pub total_blocks: u64,
    pub total_supply: f64,
    pub current_difficulty: u32,
    pub best_block_hash: String,
    pub mempool_transactions: usize,
    pub mempool_size_bytes: usize,
    pub network_peers: usize,
    pub is_syncing: bool,
    pub chain_work: String,
}

/// Account balance response with enhanced security
#[derive(Debug, Serialize, Deserialize)]
pub struct BalanceResponse {
    pub address: String,
    pub balance: f64,
    pub nonce: u64,
    pub staked_amount: f64,
    pub transaction_count: u64,
}

/// Block information response
#[derive(Debug, Serialize, Deserialize)]
pub struct BlockResponse {
    pub height: u64,
    pub hash: String,
    pub previous_hash: String,
    pub timestamp: DateTime<Utc>,
    pub transactions: Vec<TransactionSummary>,
    pub transaction_count: usize,
    pub difficulty: u32,
    pub nonce: u64,
    pub size_bytes: usize,
}

/// Transaction summary for block responses
#[derive(Debug, Serialize, Deserialize)]
pub struct TransactionSummary {
    pub id: String,
    pub from: String,
    pub tx_type: String,
    pub amount: f64,
    pub fee: f64,
}

/// Transaction submission request with validation
#[derive(Debug, Serialize, Deserialize)]
pub struct TransactionRequest {
    pub from: String,       // Hex-encoded public key
    pub to: String,         // Hex-encoded recipient address
    pub amount: u64,        // Amount in smallest units (NANO units, 1 NUMI = 100 NANO)
    pub nonce: u64,         // Account nonce (prevents replay attacks)
    pub fee: Option<u64>,   // Optional custom fee in NANO units (uses calculated minimum if not provided)
    pub signature: String,  // Hex-encoded detached Dilithium3 signature bytes
}

/// Transaction response
#[derive(Debug, Serialize, Deserialize)]
pub struct TransactionResponse {
    pub id: String,
    pub status: String,
    pub validation_result: String,
}



/// Login request
#[derive(Debug, Serialize, Deserialize)]
pub struct LoginRequest {
    pub api_key: String,
}

/// Login response
#[derive(Debug, Serialize, Deserialize)]
pub struct LoginResponse {
    pub token: String,
}

/// Utility function to display transaction fee
pub fn get_transaction_fee_display(transaction: &crate::transaction::Transaction) -> f64 {
    transaction.fee as f64 / 100.0 // Convert to NUMI
}

/// Utility function to decode hex with error handling
pub async fn decode_hex_field(
    hex_str: &str,
    field_name: &str,
) -> Result<Vec<u8>, String> {
    // Accept common user inputs such as a "0x" prefix or surrounding
    // whitespace.  We *only* perform trimming here so that downstream
    // validation (e.g. signature verification) still fails if the caller
    // supplies the wrong key bytes.
    let cleaned = hex_str.trim().strip_prefix("0x").unwrap_or(hex_str.trim());

    hex::decode(cleaned).map_err(|e| {
        // Provide a more descriptive error to help users troubleshoot.
        format!(
            "Invalid {field_name} hex format ({e}). Expected even-length hex string, optionally prefixed with 0x"
        )
    })
}

/// Convert ValidationResult to user-friendly status message
pub fn validation_result_to_status(result: &ValidationResult) -> String {
    match result {
        ValidationResult::Valid => "accepted".to_string(),
        ValidationResult::InvalidSignature => "rejected: invalid signature".to_string(),
        ValidationResult::InvalidNonce { expected, got } => {
            format!("rejected: invalid nonce (expected {expected}, got {got})")
        }
        ValidationResult::InsufficientBalance { required, available } => {
            format!("rejected: insufficient balance (required {required}, available {available})")
        }
        ValidationResult::DuplicateTransaction => "rejected: duplicate transaction".to_string(),
        ValidationResult::TransactionTooLarge => "rejected: transaction too large".to_string(),
        ValidationResult::FeeTooLow { minimum, got } => {
            format!("rejected: fee too low (minimum {minimum}, got {got})")
        }
        ValidationResult::AccountSpamming { rate_limit } => {
            format!("rejected: account spamming (rate limit: {rate_limit})")
        }
        ValidationResult::TransactionExpired => "rejected: transaction expired".to_string(),
    }
} 