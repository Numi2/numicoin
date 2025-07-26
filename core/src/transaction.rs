use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use crate::crypto::{Dilithium3Keypair, Dilithium3Signature, blake3_hash, blake3_hash_hex};
use crate::error::BlockchainError;
use crate::Result;

pub type TransactionId = [u8; 32];

/// Maximum transaction size in bytes (prevent DoS)
pub const MAX_TRANSACTION_SIZE: usize = 1024 * 1024; // 1MB

/// Minimum transaction fee in smallest units (prevent spam)
pub const MIN_TRANSACTION_FEE: u64 = 1_000; // 0.000001 NUMI

/// Maximum transaction fee (prevent accidents)
pub const MAX_TRANSACTION_FEE: u64 = 1_000_000_000_000; // 1000 NUMI

/// Fee per byte for standard transactions
pub const STANDARD_FEE_PER_BYTE: u64 = 100; // 0.0000001 NUMI per byte

/// Base fee for all transactions
pub const BASE_TRANSACTION_FEE: u64 = 10_000; // 0.00001 NUMI

/// Maximum transaction validity period in seconds
pub const MAX_TRANSACTION_VALIDITY: u64 = 3600; // 1 hour

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransactionType {
    Transfer {
        to: Vec<u8>, // Public key of recipient
        amount: u64,
        /// Optional memo/message (max 256 bytes)
        memo: Option<String>,
    },
    MiningReward {
        block_height: u64,
        amount: u64,
        /// Mining pool address (if applicable)
        pool_address: Option<Vec<u8>>,
    },
    /// Contract deployment (future extension)
    ContractDeploy {
        code_hash: [u8; 32],
        init_data: Vec<u8>,
    },
    /// Contract execution (future extension)
    ContractCall {
        contract_address: Vec<u8>,
        method: String,
        params: Vec<u8>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionFee {
    /// Base fee amount
    pub base_fee: u64,
    /// Size-based fee
    pub size_fee: u64,
    /// Priority fee (optional)
    pub priority_fee: u64,
    /// Total fee amount
    pub total: u64,
}

impl TransactionFee {
    /// Calculate fee for given transaction size and priority
    pub fn calculate(size_bytes: usize, priority_multiplier: f64) -> Result<Self> {
        if size_bytes > MAX_TRANSACTION_SIZE {
            return Err(BlockchainError::InvalidTransaction("Transaction too large".to_string()));
        }
        
        if priority_multiplier < 0.0 || priority_multiplier > 10.0 {
            return Err(BlockchainError::InvalidTransaction("Invalid priority multiplier".to_string()));
        }
        
        let base_fee = BASE_TRANSACTION_FEE;
        let size_fee = (size_bytes as u64) * STANDARD_FEE_PER_BYTE;
        let priority_fee = ((base_fee + size_fee) as f64 * priority_multiplier) as u64;
        let total = base_fee + size_fee + priority_fee;
        
        if total < MIN_TRANSACTION_FEE {
            return Err(BlockchainError::InvalidTransaction("Fee too low".to_string()));
        }
        
        if total > MAX_TRANSACTION_FEE {
            return Err(BlockchainError::InvalidTransaction("Fee too high".to_string()));
        }
        
        Ok(Self {
            base_fee,
            size_fee,
            priority_fee,
            total,
        })
    }
    
    /// Calculate minimum fee for transaction size
    pub fn minimum_for_size(size_bytes: usize) -> Result<Self> {
        Self::calculate(size_bytes, 0.0)
    }
    
    /// Validate fee amount against calculated minimum
    pub fn validate_amount(&self, paid_fee: u64) -> Result<()> {
        if paid_fee < self.total {
            return Err(BlockchainError::InvalidTransaction(
                format!("Insufficient fee: paid {}, required {}", paid_fee, self.total)));
        }
        
        if paid_fee > MAX_TRANSACTION_FEE {
            return Err(BlockchainError::InvalidTransaction(
                format!("Fee too high: {}", paid_fee)));
        }
        
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub id: TransactionId,
    pub from: Vec<u8>, // Public key of sender
    pub transaction_type: TransactionType,
    pub nonce: u64,
    pub fee: u64, // Transaction fee
    pub gas_limit: u64, // Maximum computation units (for contract calls)
    pub timestamp: DateTime<Utc>,
    /// Transaction validity period (timestamp + max_validity)
    pub valid_until: DateTime<Utc>,
    /// Optional metadata
    pub metadata: Option<String>,
    pub signature: Option<Dilithium3Signature>,
}

impl Transaction {
    pub fn new(
        from: Vec<u8>,
        transaction_type: TransactionType,
        nonce: u64,
    ) -> Self {
        
        let mut tx = Self::new_with_fee(from, transaction_type, nonce, MIN_TRANSACTION_FEE, 0);
        
        // Calculate proper fee based on transaction size
        if let Ok(size) = tx.calculate_size() {
            if let Ok(fee_info) = TransactionFee::minimum_for_size(size) {
                tx.fee = fee_info.total;
            }
        }
        
        tx.id = tx.calculate_hash();
        tx
    }
    
    pub fn new_with_fee(
        from: Vec<u8>,
        transaction_type: TransactionType,
        nonce: u64,
        fee: u64,
        gas_limit: u64,
    ) -> Self {
        let timestamp = Utc::now();
        let valid_until = timestamp + chrono::Duration::seconds(MAX_TRANSACTION_VALIDITY as i64);
        
        let mut tx = Self {
            id: [0u8; 32],
            from,
            transaction_type,
            nonce,
            fee,
            gas_limit,
            timestamp,
            valid_until,
            metadata: None,
            signature: None,
        };
        
        tx.id = tx.calculate_hash();
        tx
    }
    
    pub fn sign(&mut self, keypair: &Dilithium3Keypair) -> Result<()> {
        // Validate before signing
        self.validate_structure()?;
        
        let message = self.serialize_for_signing()?;
        self.signature = Some(keypair.sign(&message)?);
        
        // Recalculate ID after signing
        self.id = self.calculate_hash();
        Ok(())
    }
    
    pub fn verify_signature(&self) -> Result<bool> {
        if let Some(ref signature) = self.signature {
            let message = self.serialize_for_signing()?;
            crate::crypto::Dilithium3Keypair::verify(&message, signature, &self.from)
        } else {
            Ok(false)
        }
    }
    
    pub fn calculate_hash(&self) -> TransactionId {
        let data = self.serialize_for_signing().unwrap_or_default();
        blake3_hash(&data)
    }
    
    pub fn get_hash_hex(&self) -> String {
        blake3_hash_hex(&self.id)
    }
    
    /// Calculate transaction size in bytes
    pub fn calculate_size(&self) -> Result<usize> {
        // Only serialize the transaction for signing (exclude signature and id)
        let serialized = self.serialize_for_signing()?;
        Ok(serialized.len())
    }
    
    /// Validate transaction structure and constraints
    pub fn validate_structure(&self) -> Result<()> {
        // Validate public key format
        if self.from.is_empty() || self.from.len() > 10000 {
            return Err(BlockchainError::InvalidTransaction("Invalid sender public key".to_string()));
        }
        
        // Validate transaction size
        let size = self.calculate_size()?;
        // Enforce maximum transaction size
        if size > MAX_TRANSACTION_SIZE {
            return Err(BlockchainError::InvalidTransaction(
                format!("Transaction too large: {} bytes", size)));
        }
       
        // Validate fee for normal transactions (skip mining rewards and gas-based types)
        if !self.is_reward() && !self.transaction_type.requires_gas() {
            let fee_info = TransactionFee::minimum_for_size(size)?;
            fee_info.validate_amount(self.fee)?;
        }
        
        // Validate timestamp and validity
        let now = Utc::now();
        if self.timestamp > now + chrono::Duration::seconds(300) {
            return Err(BlockchainError::InvalidTransaction("Transaction timestamp too far in future".to_string()));
        }
        
        if self.valid_until <= self.timestamp {
            return Err(BlockchainError::InvalidTransaction("Invalid validity period".to_string()));
        }
        
        if now > self.valid_until {
            return Err(BlockchainError::InvalidTransaction("Transaction expired".to_string()));
        }
        
        // Validate transaction type specific fields
        self.validate_transaction_type()?;
        
        // Validate metadata size
        if let Some(ref metadata) = self.metadata {
            if metadata.len() > 1024 {
                return Err(BlockchainError::InvalidTransaction("Metadata too large".to_string()));
            }
        }
        
        Ok(())
    }
    
    /// Validate transaction type specific constraints
    fn validate_transaction_type(&self) -> Result<()> {
        match &self.transaction_type {
            TransactionType::Transfer { to, amount, memo } => {
                if to.is_empty() || to.len() > 10000 {
                    return Err(BlockchainError::InvalidTransaction("Invalid recipient address".to_string()));
                }
                if *amount == 0 {
                    return Err(BlockchainError::InvalidTransaction("Transfer amount cannot be zero".to_string()));
                }
                if let Some(ref memo) = memo {
                    if memo.len() > 256 {
                        return Err(BlockchainError::InvalidTransaction("Memo too long".to_string()));
                    }
                    // Ensure memo is valid UTF-8
                    if !memo.is_ascii() {
                        return Err(BlockchainError::InvalidTransaction("Memo must be ASCII".to_string()));
                    }
                }
            }
            

            
            TransactionType::MiningReward { block_height: _, amount, pool_address } => {
                if *amount == 0 {
                    return Err(BlockchainError::InvalidTransaction("Mining reward cannot be zero".to_string()));
                }
                if let Some(ref pool) = pool_address {
                    if pool.is_empty() || pool.len() > 10000 {
                        return Err(BlockchainError::InvalidTransaction("Invalid pool address".to_string()));
                    }
                }
                // Mining rewards should have zero fees (system generated)
                if self.fee != 0 {
                    return Err(BlockchainError::InvalidTransaction("Mining rewards cannot have fees".to_string()));
                }
            }
            

            
            TransactionType::ContractDeploy { code_hash: _, init_data } => {
                if init_data.len() > 100_000 { // 100KB limit for init data
                    return Err(BlockchainError::InvalidTransaction("Contract init data too large".to_string()));
                }
                if self.gas_limit == 0 {
                    return Err(BlockchainError::InvalidTransaction("Contract deployment requires gas limit".to_string()));
                }
            }
            
            TransactionType::ContractCall { contract_address, method, params } => {
                if contract_address.is_empty() || contract_address.len() > 10000 {
                    return Err(BlockchainError::InvalidTransaction("Invalid contract address".to_string()));
                }
                if method.is_empty() || method.len() > 64 {
                    return Err(BlockchainError::InvalidTransaction("Invalid method name".to_string()));
                }
                if params.len() > 100_000 { // 100KB limit for parameters
                    return Err(BlockchainError::InvalidTransaction("Contract parameters too large".to_string()));
                }
                if self.gas_limit == 0 {
                    return Err(BlockchainError::InvalidTransaction("Contract call requires gas limit".to_string()));
                }
            }
        }
        
        Ok(())
    }
    
    fn serialize_for_signing(&self) -> Result<Vec<u8>> {
        // Create a copy without signature for signing
        let tx_for_signing = TransactionForSigning {
            from: self.from.clone(),
            transaction_type: self.transaction_type.clone(),
            nonce: self.nonce,
            fee: self.fee,
            gas_limit: self.gas_limit,
            timestamp: self.timestamp,
            valid_until: self.valid_until,
            metadata: self.metadata.clone(),
        };
        
        bincode::serialize(&tx_for_signing)
            .map_err(|e| BlockchainError::SerializationError(format!("Failed to serialize transaction: {e}")))
    }
    
    pub fn validate(&self, current_balance: u64, current_nonce: u64) -> Result<()> {
        // Verify signature
        if !self.verify_signature()? {
            return Err(BlockchainError::InvalidSignature("Transaction signature verification failed".to_string()));
        }
        
        // Verify nonce
        if self.nonce != current_nonce + 1 {
            return Err(BlockchainError::InvalidNonce {
                expected: current_nonce + 1,
                found: self.nonce,
            });
        }
        
        // Verify sufficient balance for transaction amount
        let amount = self.get_amount();
        if amount > current_balance {
            return Err(BlockchainError::InsufficientBalance(
                format!("Required: {}, Available: {}", amount, current_balance)
            ));
        }
        
        // Validate structure
        self.validate_structure()?;
        
        Ok(())
    }
    
    /// Get total amount required from sender (including fees)
    pub fn get_required_balance(&self) -> u64 {
        let amount = self.get_amount();
        amount.saturating_add(self.fee)
    }
    
    pub fn get_amount(&self) -> u64 {
        match &self.transaction_type {
            TransactionType::Transfer { amount, .. } => *amount,
            TransactionType::MiningReward { amount, .. } => *amount,
            TransactionType::ContractDeploy { .. } => 0, // Only fee required
            TransactionType::ContractCall { .. } => 0, // Only fee and gas required
        }
    }
    
    pub fn is_reward(&self) -> bool {
        matches!(self.transaction_type, TransactionType::MiningReward { .. })
    }
    
    /// Check if transaction has expired
    pub fn is_expired(&self) -> bool {
        Utc::now() > self.valid_until
    }
    
    /// Get transaction priority score for mempool ordering
    pub fn get_priority_score(&self) -> u64 {
        if self.is_reward() {
            return u64::MAX; // Highest priority for mining rewards
        }
        // Prioritize by fee for non-reward transactions
        self.fee
    }
    
    /// Set transaction metadata
    pub fn set_metadata(&mut self, metadata: String) -> Result<()> {
        if metadata.len() > 1024 {
            return Err(BlockchainError::InvalidTransaction("Metadata too large".to_string()));
        }
        self.metadata = Some(metadata);
        self.id = self.calculate_hash();
        Ok(())
    }
    
    /// Get transaction type as string
    pub fn get_type_string(&self) -> &'static str {
        match &self.transaction_type {
            TransactionType::Transfer { .. } => "transfer",
            TransactionType::MiningReward { .. } => "mining_reward",
            TransactionType::ContractDeploy { .. } => "contract_deploy",
            TransactionType::ContractCall { .. } => "contract_call",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TransactionForSigning {
    from: Vec<u8>,
    transaction_type: TransactionType,
    nonce: u64,
    fee: u64,
    gas_limit: u64,
    timestamp: DateTime<Utc>,
    valid_until: DateTime<Utc>,
    metadata: Option<String>,
}

impl TransactionType {
    pub fn is_transfer(&self) -> bool {
        matches!(self, TransactionType::Transfer { .. })
    }
    
    pub fn is_reward(&self) -> bool {
        matches!(self, TransactionType::MiningReward { .. })
    }
    
    pub fn is_contract_deploy(&self) -> bool {
        matches!(self, TransactionType::ContractDeploy { .. })
    }
    
    pub fn is_contract_call(&self) -> bool {
        matches!(self, TransactionType::ContractCall { .. })
    }
    
    /// Check if transaction type requires gas
    pub fn requires_gas(&self) -> bool {
        matches!(self, TransactionType::ContractDeploy { .. } | TransactionType::ContractCall { .. })
    }
    
    /// Get estimated gas cost for transaction type
    pub fn estimate_gas(&self) -> u64 {
        match self {
            TransactionType::Transfer { .. } => 21_000,
            TransactionType::MiningReward { .. } => 0,
            TransactionType::ContractDeploy { .. } => 200_000,
            TransactionType::ContractCall { .. } => 100_000,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_transaction_fee_calculation() {
        // Test minimum fee calculation
        let fee_info = TransactionFee::minimum_for_size(500).unwrap();
        assert_eq!(fee_info.base_fee, BASE_TRANSACTION_FEE);
        assert_eq!(fee_info.size_fee, 500 * STANDARD_FEE_PER_BYTE);
        assert_eq!(fee_info.priority_fee, 0);
        assert_eq!(fee_info.total, fee_info.base_fee + fee_info.size_fee);
        
        // Test priority fee calculation
        let priority_fee = TransactionFee::calculate(500, 1.0).unwrap();
        assert!(priority_fee.priority_fee > 0);
        assert!(priority_fee.total > fee_info.total);
        
        // Test validation
        assert!(TransactionFee::calculate(MAX_TRANSACTION_SIZE + 1, 0.0).is_err());
        assert!(TransactionFee::calculate(500, -1.0).is_err());
        assert!(TransactionFee::calculate(500, 11.0).is_err());
    }
    
    #[test]
    fn test_transaction_creation() {
        let keypair = Dilithium3Keypair::new().unwrap();
        let tx = Transaction::new(
            keypair.public_key.clone(),
            TransactionType::Transfer {
                to: vec![1, 2, 3, 4],
                amount: 100,
                memo: Some("Hello".to_string()),
            },
            1,
        );
        
        assert_eq!(tx.nonce, 1);
        assert_eq!(tx.get_amount(), 100);
        assert!(!tx.is_expired());
        assert_eq!(tx.get_type_string(), "transfer");
        assert!(tx.fee >= MIN_TRANSACTION_FEE);
        assert!(tx.gas_limit == 0); // No gas for transfer
        
        // Test with fee
        let tx_with_fee = Transaction::new_with_fee(
            keypair.public_key.clone(),
            TransactionType::Transfer {
                to: vec![1, 2, 3, 4],
                amount: 100,
                memo: None,
            },
            1,
            50_000,
            0,
        );
        
        assert_eq!(tx_with_fee.fee, 50_000);
    }
    
    #[test]
    fn test_transaction_signing() {
        let keypair = Dilithium3Keypair::new().unwrap();
        let mut tx = Transaction::new(
            keypair.public_key.clone(),
            TransactionType::Transfer {
                to: vec![1, 2, 3, 4],
                amount: 100,
                memo: None,
            },
            1,
        );
        
        tx.sign(&keypair).unwrap();
        assert!(tx.verify_signature().unwrap());
        
        // Test signature verification detects wrong sender
        let wrong_keypair = Dilithium3Keypair::new().unwrap();
        let mut wrong_tx = Transaction::new(
            wrong_keypair.public_key.clone(),
            TransactionType::Transfer {
                to: vec![1, 2, 3, 4],
                amount: 100,
                memo: None,
            },
            1,
        );
        
        // Sign with different key than the from field
        wrong_tx.from = vec![0; 32]; // Different from keypair.public_key
        wrong_tx.sign(&wrong_keypair).unwrap();
        assert!(wrong_tx.signature.is_some());
        assert!(!wrong_tx.verify_signature().unwrap()); // Should fail due to sender mismatch
    }
    
    #[test]
    fn test_transaction_validation() {
        let keypair = Dilithium3Keypair::new().unwrap();
        let mut tx = Transaction::new(
            keypair.public_key.clone(),
            TransactionType::Transfer {
                to: vec![1, 2, 3, 4],
                amount: 100,
                memo: None,
            },
            1,
        );
        
        tx.sign(&keypair).unwrap();
        
        // Should validate with sufficient balance
        assert!(tx.validate(1000, 0).is_ok());
        
        // Should fail with insufficient balance
        assert!(tx.validate(50, 0).is_err());
        
        // Should fail with wrong nonce
        assert!(tx.validate(1000, 2).is_err());
    }
    
    #[test]
    fn test_transaction_structure_validation() {
        let keypair = Dilithium3Keypair::new().unwrap();
        
        // Valid transaction
        let valid_tx = Transaction::new(
            keypair.public_key.clone(),
            TransactionType::Transfer {
                to: vec![1, 2, 3, 4, 5, 6, 7, 8],
                amount: 100,
                memo: Some("Valid memo".to_string()),
            },
            1,
        );
        assert!(valid_tx.validate_structure().is_ok());
        
        // Invalid recipient address (empty)
        let invalid_tx = Transaction::new(
            keypair.public_key.clone(),
            TransactionType::Transfer {
                to: Vec::new(),
                amount: 100,
                memo: None,
            },
            1,
        );
        assert!(invalid_tx.validate_structure().is_err());
        
        // Invalid amount (zero)
        let zero_amount_tx = Transaction::new(
            keypair.public_key.clone(),
            TransactionType::Transfer {
                to: vec![1, 2, 3, 4],
                amount: 0,
                memo: None,
            },
            1,
        );
        assert!(zero_amount_tx.validate_structure().is_err());
        
        // Invalid memo (too long)
        let long_memo_tx = Transaction::new(
            keypair.public_key.clone(),
            TransactionType::Transfer {
                to: vec![1, 2, 3, 4],
                amount: 100,
                memo: Some("a".repeat(300)),
            },
            1,
        );
        assert!(long_memo_tx.validate_structure().is_err());
    }
    

    
    #[test]
    fn test_mining_reward_transaction() {
        let keypair = Dilithium3Keypair::new().unwrap();
        
        // Mining rewards should have zero fees
        let mut reward_tx = Transaction::new_with_fee(
            keypair.public_key.clone(),
            TransactionType::MiningReward {
                block_height: 100,
                amount: 5_000_000_000,
                pool_address: None,
            },
            1,
            0, // Zero fee
            0,
        );
        
        assert!(reward_tx.validate_structure().is_ok());
        assert!(reward_tx.is_reward());
        assert_eq!(reward_tx.get_priority_score(), u64::MAX);
        
        // Mining reward with fee should fail
        reward_tx.fee = 1000;
        assert!(reward_tx.validate_structure().is_err());
    }
    

    
    #[test]
    fn test_contract_transactions() {
        let keypair = Dilithium3Keypair::new().unwrap();
        
        // Contract deployment
        let deploy_tx = Transaction::new_with_fee(
            keypair.public_key.clone(),
            TransactionType::ContractDeploy {
                code_hash: [0u8; 32],
                init_data: vec![1, 2, 3, 4],
            },
            1,
            100_000,
            200_000, // Gas limit required
        );
        
        assert!(deploy_tx.validate_structure().is_ok());
        assert_eq!(deploy_tx.get_type_string(), "contract_deploy");
        assert!(deploy_tx.transaction_type.requires_gas());
        
        // Contract call
        let call_tx = Transaction::new_with_fee(
            keypair.public_key.clone(),
            TransactionType::ContractCall {
                contract_address: vec![1, 2, 3, 4],
                method: "transfer".to_string(),
                params: vec![0; 100],
            },
            1,
            50_000,
            100_000, // Gas limit required
        );
        
        assert!(call_tx.validate_structure().is_ok());
        assert_eq!(call_tx.get_type_string(), "contract_call");
        
        // Contract deployment without gas should fail
        let no_gas_deploy = Transaction::new_with_fee(
            keypair.public_key.clone(),
            TransactionType::ContractDeploy {
                code_hash: [0u8; 32],
                init_data: vec![1, 2, 3, 4],
            },
            1,
            100_000,
            0, // No gas limit
        );
        assert!(no_gas_deploy.validate_structure().is_err());
    }
    
    #[test]
    fn test_transaction_expiry() {
        let keypair = Dilithium3Keypair::new().unwrap();
        let tx = Transaction::new(
            keypair.public_key.clone(),
            TransactionType::Transfer {
                to: vec![1, 2, 3, 4],
                amount: 100,
                memo: None,
            },
            1,
        );
        
        assert!(!tx.is_expired());
        assert!(tx.valid_until > tx.timestamp);
        
        // Create an expired transaction
        let mut expired_tx = tx.clone();
        expired_tx.valid_until = Utc::now() - chrono::Duration::seconds(10);
        assert!(expired_tx.is_expired());
        assert!(expired_tx.validate_structure().is_err());
    }
    
    #[test]
    fn test_transaction_metadata() {
        let keypair = Dilithium3Keypair::new().unwrap();
        let mut tx = Transaction::new(
            keypair.public_key.clone(),
            TransactionType::Transfer {
                to: vec![1, 2, 3, 4],
                amount: 100,
                memo: None,
            },
            1,
        );
        
        // Set valid metadata
        assert!(tx.set_metadata("Valid metadata".to_string()).is_ok());
        assert_eq!(tx.metadata, Some("Valid metadata".to_string()));
        
        // Try to set invalid metadata (too long)
        let long_metadata = "a".repeat(2000);
        assert!(tx.set_metadata(long_metadata).is_err());
    }
    
    #[test]
    fn test_required_balance_calculation() {
        let keypair = Dilithium3Keypair::new().unwrap();
        let tx = Transaction::new_with_fee(
            keypair.public_key.clone(),
            TransactionType::Transfer {
                to: vec![1, 2, 3, 4],
                amount: 100,
                memo: None,
            },
            1,
            50,
            0,
        );
        assert_eq!(tx.get_required_balance(), 150); // amount + fee
    }
    
    #[test]
    fn test_priority_score_calculation() {
        let keypair = Dilithium3Keypair::new().unwrap();
        
        // Higher fee should give higher priority
        let high_fee_tx = Transaction::new_with_fee(
            keypair.public_key.clone(),
            TransactionType::Transfer {
                to: vec![1, 2, 3, 4],
                amount: 100,
                memo: None,
            },
            1,
            1000,
            0,
        );
        
        let low_fee_tx = Transaction::new_with_fee(
            keypair.public_key.clone(),
            TransactionType::Transfer {
                to: vec![1, 2, 3, 4],
                amount: 100,
                memo: None,
            },
            1,
            100,
            0,
        );
        
        assert!(high_fee_tx.get_priority_score() > low_fee_tx.get_priority_score());
        
        // Mining rewards should have maximum priority
        let reward_tx = Transaction::new_with_fee(
            keypair.public_key.clone(),
            TransactionType::MiningReward {
                block_height: 1,
                amount: 1000,
                pool_address: None,
            },
            1,
            0,
            0,
        );
        
        assert_eq!(reward_tx.get_priority_score(), u64::MAX);
    }
    
    #[test]
    fn test_transaction_type_helpers() {
        // Test all transaction type helpers
        let transfer_type = TransactionType::Transfer {
            to: vec![1, 2, 3, 4],
            amount: 100,
            memo: None,
        };
        assert!(transfer_type.is_transfer());
        assert!(!transfer_type.requires_gas());
        assert_eq!(transfer_type.estimate_gas(), 21_000);

        let contract_deploy_type = TransactionType::ContractDeploy {
            code_hash: [0u8; 32],
            init_data: vec![],
        };
        assert!(contract_deploy_type.is_contract_deploy());
        assert!(contract_deploy_type.requires_gas());
        assert_eq!(contract_deploy_type.estimate_gas(), 200_000);

        let contract_call_type = TransactionType::ContractCall {
            contract_address: vec![1, 2, 3, 4],
            method: "test".to_string(),
            params: vec![],
        };
        assert!(contract_call_type.is_contract_call());
        assert!(contract_call_type.requires_gas());
    }
} 