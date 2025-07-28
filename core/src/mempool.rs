use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use dashmap::DashMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::transaction::{Transaction, TransactionId, TransactionType, TransactionFee, MIN_TRANSACTION_FEE};
use crate::{Result, BlockchainError};
use crate::blockchain::NumiBlockchain;
use crate::config::ConsensusConfig;


/// Transaction priority score based on fee rate and age
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct TransactionPriority {
    /// Fee per byte (higher = higher priority)
    pub fee_rate: u64,
    /// Transaction age penalty (older = lower priority to prevent spam)
    pub age_penalty: u64,
    /// Transaction ID for deterministic ordering
    pub tx_id: TransactionId,
}



/// Transaction entry in the mempool with metadata
#[derive(Debug, Clone)]
pub struct MempoolEntry {
    pub transaction: Transaction,
    pub added_at: Instant,
    pub size_bytes: usize,
    pub fee_rate: u64,
    pub priority: TransactionPriority,
    pub validation_attempts: u8,
}

/// Transaction validation result
#[derive(Debug, Clone, PartialEq)]
pub enum ValidationResult {
    Valid,
    InvalidSignature,
    InvalidNonce { expected: u64, got: u64 },
    InsufficientBalance { required: u64, available: u64 },
    DuplicateTransaction,
    TransactionTooLarge,
    FeeTooLow { minimum: u64, got: u64 },
    AccountSpamming { rate_limit: u64 },
    TransactionExpired,
}

/// Statistics about the mempool state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MempoolStats {
    pub total_transactions: usize,
    pub total_size_bytes: usize,
    pub pending_by_fee_range: HashMap<String, usize>,
    pub oldest_transaction_age: Duration,
    pub accounts_with_pending: usize,
    pub rejected_transactions_1h: usize,
}

/// Production-ready transaction mempool with advanced features
pub struct TransactionMempool {
    // Core data structures
    /// Priority queue ordered by fee rate (BTreeMap for efficient range queries)
    priority_queue: Arc<RwLock<BTreeMap<TransactionPriority, TransactionId>>>,
    
    /// All transactions indexed by ID for O(1) lookup
    transactions: Arc<DashMap<TransactionId, MempoolEntry>>,
    
    /// Account nonces to prevent replay attacks
    account_nonces: Arc<DashMap<Vec<u8>, u64>>,
    
    /// Transactions by sender account for efficient account queries
    transactions_by_account: Arc<DashMap<Vec<u8>, HashSet<TransactionId>>>,
    
    /// Weak reference to the blockchain for state-aware validation.  A weak
    /// reference breaks the strong reference cycle between `NumiBlockchain`
    /// and `TransactionMempool` while still allowing on-demand access.
    blockchain: Option<Weak<RwLock<NumiBlockchain>>>,

    /// Configuration parameters (from ConsensusConfig)
    consensus_config: ConsensusConfig,
    max_mempool_size: usize,         // Maximum memory usage in bytes
    max_transactions: usize,         // Maximum number of transactions
    max_tx_age: Duration,           // Maximum transaction age before expiry
    _max_account_txs: usize,         // Maximum pending transactions per account
    
    /// Anti-spam protection
    account_submission_rates: Arc<DashMap<Vec<u8>, Vec<Instant>>>,
    max_submissions_per_hour: usize,
    
    /// Statistics
    current_size_bytes: Arc<RwLock<usize>>,
    rejected_count_1h: Arc<RwLock<usize>>,
    last_cleanup: Arc<RwLock<Instant>>,
}

impl Default for TransactionMempool {
    fn default() -> Self {
        Self::new()
    }
}

impl TransactionMempool {
    /// Create new mempool with default configuration
    pub fn new() -> Self {
        Self::new_with_config(ConsensusConfig::default())
    }
    
    /// Create new mempool with specific consensus configuration
    pub fn new_with_config(consensus_config: ConsensusConfig) -> Self {
        Self {
            priority_queue: Arc::new(RwLock::new(BTreeMap::new())),
            transactions: Arc::new(DashMap::new()),
            account_nonces: Arc::new(DashMap::new()),
            transactions_by_account: Arc::new(DashMap::new()),
            blockchain: None,
            
            // Use configuration values
            max_mempool_size: consensus_config.max_block_size * 256, // 256x block size for mempool
            max_transactions: consensus_config.max_transactions_per_block * 1000, // 1000x block tx limit
            max_tx_age: Duration::from_secs(3600), // 1 hour
            _max_account_txs: 1000,                 // 1000 pending txs per account
            consensus_config,
            
            // Anti-spam: 100 submissions per hour per account
            account_submission_rates: Arc::new(DashMap::new()),
            max_submissions_per_hour: 100,
            
            current_size_bytes: Arc::new(RwLock::new(0)),
            rejected_count_1h: Arc::new(RwLock::new(0)),
            last_cleanup: Arc::new(RwLock::new(Instant::now())),
        }
    }

    /// Set a blockchain handle for state-aware validation
    pub fn set_blockchain_handle(&mut self, blockchain: &Arc<RwLock<NumiBlockchain>>) {
        self.blockchain = Some(Arc::downgrade(blockchain));
    }

    /// Add transaction to mempool with full validation
    pub async fn add_transaction(&self, transaction: Transaction) -> Result<ValidationResult> {
        let tx_id = transaction.id;
        let sender = &transaction.from;
        
        // Check if transaction already exists
        if self.transactions.contains_key(&tx_id) {
            return Ok(ValidationResult::DuplicateTransaction);
        }

        // Validate transaction before admission
        let validation_result = self.validate_transaction(&transaction).await?;
        if validation_result != ValidationResult::Valid {
            *self.rejected_count_1h.write() += 1;
            return Ok(validation_result);
        }

        // Check spam protection
        if !self.check_submission_rate_limit(sender).await {
            return Ok(ValidationResult::AccountSpamming { 
                rate_limit: self.max_submissions_per_hour as u64 
            });
        }

        // Calculate transaction metrics
        let tx_size = self.calculate_transaction_size(&transaction);
        let fee_rate = self.calculate_fee_rate(&transaction, tx_size);
        
        // Check if mempool has space
        if !self.has_space_for_transaction(tx_size, fee_rate) {
            // Try to evict lower-priority transactions
            if !self.make_space_for_transaction(tx_size, fee_rate).await {
                return Ok(ValidationResult::FeeTooLow { 
                    minimum: self.dynamic_min_fee_rate(), 
                    got: fee_rate 
                });
            }
        }

        // Create mempool entry
        let priority = TransactionPriority {
            fee_rate,
            age_penalty: 0, // Will increase over time
            tx_id,
        };

        let entry = MempoolEntry {
            transaction: transaction.clone(),
            added_at: Instant::now(),
            size_bytes: tx_size,
            fee_rate,
            priority: priority.clone(),
            validation_attempts: 0,
        };

        // Add to all data structures atomically
        self.transactions.insert(tx_id, entry);
        self.priority_queue.write().insert(priority, tx_id);
        
        // Update account tracking
        self.account_nonces.insert(sender.clone(), transaction.nonce);
        self.transactions_by_account
            .entry(sender.clone())
            .or_default()
            .insert(tx_id);

        // Update stats
        *self.current_size_bytes.write() += tx_size;
        
        // Record submission for rate limiting
        self.record_submission(sender).await;

        log::info!("✅ Transaction {} added to mempool (fee_rate: {})", 
                  hex::encode(tx_id), fee_rate);

        Ok(ValidationResult::Valid)
    }

    /// Get highest priority transactions for block creation
    pub fn get_transactions_for_block(&self, max_block_size: usize, max_transactions: usize) -> Vec<Transaction> {
        // Ensure priorities are up-to-date before selection
        self.refresh_priorities();
        let mut selected = Vec::new();
        let mut total_size = 0;
        let priority_queue = self.priority_queue.read();
        
        // Iterate from highest to lowest priority
        for (_, tx_id) in priority_queue.iter().rev() {
            if selected.len() >= max_transactions {
                break;
            }
            
            if let Some(entry) = self.transactions.get(tx_id) {
                if total_size + entry.size_bytes > max_block_size {
                    continue; // Skip if transaction would exceed block size
                }
                
                selected.push(entry.transaction.clone());
                total_size += entry.size_bytes;
            }
        }
        
        selected
    }

    /// Remove transactions (typically after block inclusion)
    pub async fn remove_transactions(&self, tx_ids: &[TransactionId]) {
        for tx_id in tx_ids {
            if let Some((_, entry)) = self.transactions.remove(tx_id) {
                // Remove from priority queue
                self.priority_queue.write().remove(&entry.priority);
                
                // Update account tracking
                let sender = &entry.transaction.from;
                if let Some(mut account_txs) = self.transactions_by_account.get_mut(sender) {
                    account_txs.remove(tx_id);
                    if account_txs.is_empty() {
                        drop(account_txs);
                        self.transactions_by_account.remove(sender);
                    }
                }
                
                // Update stats
                *self.current_size_bytes.write() -= entry.size_bytes;
                
                log::debug!("🗑️ Removed transaction {} from mempool", hex::encode(tx_id));
            }
        }
    }

    /// Get transactions for a specific account
    pub fn get_account_transactions(&self, account: &[u8]) -> Vec<Transaction> {
        if let Some(tx_ids) = self.transactions_by_account.get(account) {
            tx_ids.iter()
                .filter_map(|tx_id| self.transactions.get(tx_id))
                .map(|entry| entry.transaction.clone())
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Get mempool statistics
    pub fn get_stats(&self) -> MempoolStats {
        let transactions = &self.transactions;
        let total_transactions = transactions.len();
        let total_size_bytes = *self.current_size_bytes.read();
        
        // Calculate fee distribution
        let mut pending_by_fee_range = HashMap::new();
        let mut oldest_age = Duration::ZERO;
        let now = Instant::now();
        
        for entry in transactions.iter() {
            let fee_range = match entry.fee_rate {
                0..=1000 => "low".to_string(),
                1001..=5000 => "medium".to_string(),
                5001..=20000 => "high".to_string(),
                _ => "premium".to_string(),
            };
            *pending_by_fee_range.entry(fee_range).or_insert(0) += 1;
            
            let age = now.duration_since(entry.added_at);
            if age > oldest_age {
                oldest_age = age;
            }
        }
        
        MempoolStats {
            total_transactions,
            total_size_bytes,
            pending_by_fee_range,
            oldest_transaction_age: oldest_age,
            accounts_with_pending: self.transactions_by_account.len(),
            rejected_transactions_1h: *self.rejected_count_1h.read(),
        }
    }

    /// Periodic cleanup of expired transactions and maintenance
    pub async fn cleanup_expired_transactions(&self) {
        let now = Instant::now();
        let mut expired_tx_ids = Vec::new();
        
        // Find expired transactions
        for entry in self.transactions.iter() {
            if now.duration_since(entry.added_at) > self.max_tx_age {
                expired_tx_ids.push(*entry.key());
            }
        }
        
        // Remove expired transactions
        if !expired_tx_ids.is_empty() {
            log::info!("🧹 Removing {} expired transactions", expired_tx_ids.len());
            self.remove_transactions(&expired_tx_ids).await;
        }
        
        // Clean up old submission rate records
        self.cleanup_rate_limiting_records().await;
        
        // Reset hourly rejection count
        if now.duration_since(*self.last_cleanup.read()) > Duration::from_secs(3600) {
            *self.rejected_count_1h.write() = 0;
            *self.last_cleanup.write() = now;
        }
        // Re-compute priorities after cleanup so that age penalties are updated
        self.refresh_priorities();
    }

    /// Get all transactions currently in the mempool
    pub fn get_all_transactions(&self) -> Vec<Transaction> {
        self.transactions
            .iter()
            .map(|entry| entry.value().transaction.clone())
            .collect()
    }

    // Private helper methods
    
    /// Calculates a dynamic minimum fee rate based on current mempool utilisation.
    /// This creates economic incentives by raising the bar for inclusion when the
    /// mempool is congested and lowering it when there is plenty of capacity.
    fn dynamic_min_fee_rate(&self) -> u64 {
        let base = self.consensus_config.min_transaction_fee;
        let size_utilisation = *self.current_size_bytes.read() as f64 / self.max_mempool_size as f64;
        let count_utilisation = self.transactions.len() as f64 / self.max_transactions as f64;
        let utilisation = size_utilisation.max(count_utilisation);

        if utilisation > 0.90 {
            base.saturating_mul(5)
        } else if utilisation > 0.75 {
            base.saturating_mul(3)
        } else if utilisation > 0.50 {
            base.saturating_mul(2)
        } else {
            base
        }
    }

    /// Refresh the priority queue by applying an age‐based penalty to every
    /// transaction.  Newer transactions keep their full fee_rate while older
    /// transactions gradually lose priority, ensuring liveness and discouraging
    /// spam with low fees that linger in the mempool.
    pub fn refresh_priorities(&self) {
        let now = Instant::now();
        let mut new_queue: BTreeMap<TransactionPriority, TransactionId> = BTreeMap::new();

        // Recompute priority for every transaction
        for entry in self.transactions.iter() {
            let age_secs = now.duration_since(entry.added_at).as_secs();
            // Older transactions get a *lower* effective priority.  Because we
            // iterate over the queue in reverse order (highest first), we store
            // u64::MAX - age to invert the ordering so that large values mean
            // 2higher priority2.
            let age_penalty = u64::MAX - age_secs;

            let new_priority = TransactionPriority {
                fee_rate: entry.fee_rate,
                age_penalty,
                tx_id: *entry.key(),
            };

            // Update the entry's cached priority so removal logic remains valid
            if let Some(mut e) = self.transactions.get_mut(entry.key()) {
                e.priority = new_priority.clone();
            }

            new_queue.insert(new_priority, *entry.key());
        }

        *self.priority_queue.write() = new_queue;
    }

    async fn validate_transaction(&self, transaction: &Transaction) -> Result<ValidationResult> {
        // Use transaction's built-in validation which includes proper fee checks
        if let Err(e) = transaction.validate_structure() {
            match e {
                BlockchainError::InvalidTransaction(msg) if msg.contains("too large") => {
                    return Ok(ValidationResult::TransactionTooLarge);
                }
                BlockchainError::InvalidTransaction(msg) if msg.contains("fee") => {
                    // Extract fee information for validation result
                    let tx_size = self.calculate_transaction_size(transaction);
                    if let Ok(min_fee_info) = TransactionFee::minimum_for_size(tx_size) {
                        return Ok(ValidationResult::FeeTooLow {
                            minimum: min_fee_info.total,
                            got: transaction.fee,
                        });
                    }
                    return Ok(ValidationResult::FeeTooLow {
                        minimum: MIN_TRANSACTION_FEE,
                        got: transaction.fee,
                    });
                }
                _ => return Err(e),
            }
        }
        
        // Validate signature
        if !transaction.verify_signature()? {
            return Ok(ValidationResult::InvalidSignature);
        }
        
        // Check nonce (prevent replay attacks)
        if let Some(last_nonce) = self.account_nonces.get(&transaction.from) {
            if transaction.nonce <= *last_nonce {
                return Ok(ValidationResult::InvalidNonce {
                    expected: *last_nonce + 1,
                    got: transaction.nonce,
                });
            }
        }
        
        // Validate transaction type-specific rules
        match &transaction.transaction_type {
            TransactionType::Transfer { amount, .. } => {
                if *amount == 0 {
                    return Err(BlockchainError::InvalidTransaction("Zero amount transfer".to_string()));
                }
                
                // Check balance if blockchain reference is available
                if let Some(weak_chain) = &self.blockchain {
                    if let Some(blockchain_arc) = weak_chain.upgrade() {
                        let blockchain = blockchain_arc.read();
                        let account_state = blockchain.get_account_state_or_default(&transaction.from);
                        if account_state.balance < transaction.get_required_balance() {
                            return Ok(ValidationResult::InsufficientBalance {
                                required: transaction.get_required_balance(),
                                available: account_state.balance,
                            });
                        }
                    } else {
                        // Blockchain reference is stale, skip balance validation
                        log::warn!("⚠️ Blockchain reference is stale, skipping balance validation for transaction {}", 
                                  hex::encode(transaction.id));
                    }
                } else {
                    // No blockchain reference available, skip balance validation
                    log::warn!("⚠️ No blockchain reference available, skipping balance validation for transaction {}", 
                              hex::encode(transaction.id));
                }
            }
            TransactionType::MiningReward { .. } => {
                // Mining rewards are system-generated and pre-validated
            }
            TransactionType::ContractDeploy { .. } | TransactionType::ContractCall { .. } => {
                // Contract operations are not yet implemented
                return Err(BlockchainError::InvalidTransaction("Contract operations not supported".to_string()));
            }
        }
        
        Ok(ValidationResult::Valid)
    }

    fn calculate_transaction_size(&self, transaction: &Transaction) -> usize {
        // Use the same size calculation as Transaction::calculate_size to ensure
        // consistency between core validation and mempool admission.  This
        // excludes the signature bytes, matching the fee rules enforced by
        // `Transaction::validate_structure`.
        transaction
            .calculate_size()
            .unwrap_or_else(|_| {
                // Fallback to full serialization size only if the lighter
                // calculation unexpectedly fails.
                bincode::serialize(transaction)
                    .map(|bytes| bytes.len())
                    .unwrap_or(512)
            })
    }

    fn calculate_fee_rate(&self, transaction: &Transaction, size_bytes: usize) -> u64 {
        // Use the transaction's actual fee to calculate rate (rounded up per byte)
        if size_bytes == 0 {
            return 0;
        }
        let fee = transaction.fee;
        let size = size_bytes as u64;
        // Ceiling division ensures positive fee yields at least rate 1 when fee >= size
        fee.div_ceil(size)
    }

    fn has_space_for_transaction(&self, tx_size: usize, fee_rate: u64) -> bool {
        let current_size = *self.current_size_bytes.read();
        let current_count = self.transactions.len();
        
        // Check hard limits
        if current_size + tx_size > self.max_mempool_size {
            return false;
        }
        if current_count >= self.max_transactions {
            return false;
        }
        
        // Check if fee meets dynamic minimum
        let min_fee = self.dynamic_min_fee_rate();
        fee_rate >= min_fee
    }

    async fn make_space_for_transaction(&self, tx_size: usize, fee_rate: u64) -> bool {
        // Only evict if the new transaction has higher priority
        let mut evicted_size = 0;
        let mut to_evict = Vec::new();
        
        {
            let priority_queue = self.priority_queue.read();
            
            // Find lowest priority transactions to evict
            for (priority, tx_id) in priority_queue.iter() {
                if priority.fee_rate >= fee_rate {
                    break; // Don't evict higher priority transactions
                }
                
                if let Some(entry) = self.transactions.get(tx_id) {
                    to_evict.push(*tx_id);
                    evicted_size += entry.size_bytes;
                    
                    if evicted_size >= tx_size {
                        break;
                    }
                }
            }
        } // Lock is dropped here
        
        if evicted_size >= tx_size {
            log::info!("🔄 Evicting {} transactions to make space", to_evict.len());
            self.remove_transactions(&to_evict).await;
            true
        } else {
            false
        }
    }

    async fn check_submission_rate_limit(&self, sender: &[u8]) -> bool {
        let now = Instant::now();
        let hour_ago = now - Duration::from_secs(3600);
        
        let mut rates = self.account_submission_rates
            .entry(sender.to_vec())
            .or_default();
        
        // Remove old entries
        rates.retain(|&timestamp| timestamp > hour_ago);
        
        // Check if under rate limit
        rates.len() < self.max_submissions_per_hour
    }

    async fn record_submission(&self, sender: &[u8]) {
        let now = Instant::now();
        self.account_submission_rates
            .entry(sender.to_vec())
            .or_default()
            .push(now);
    }

    async fn cleanup_rate_limiting_records(&self) {
        let now = Instant::now();
        let hour_ago = now - Duration::from_secs(3600);
        
        // Clean up old rate limiting records
        self.account_submission_rates.retain(|_, timestamps| {
            timestamps.retain(|&timestamp| timestamp > hour_ago);
            !timestamps.is_empty()
        });
    }
}

// Thread-safe implementation
unsafe impl Send for TransactionMempool {}
unsafe impl Sync for TransactionMempool {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::Dilithium3Keypair;

    #[tokio::test]
    async fn test_mempool_creation() {
        let mempool = TransactionMempool::new();
        let stats = mempool.get_stats();
        assert_eq!(stats.total_transactions, 0);
    }

    #[tokio::test]
    async fn test_transaction_addition() {
        let mempool = TransactionMempool::new();
        let keypair = Dilithium3Keypair::new().unwrap();
        
        // Create transaction with proper fee calculation
        let mut transaction = Transaction::new(
            keypair.public_key.clone(),
            TransactionType::Transfer {
                to: vec![0; 32],
                amount: 1000,
                memo: None,
            },
            1,
        );
        
        transaction.sign(&keypair).unwrap();
        
        let result = mempool.add_transaction(transaction).await.unwrap();
        assert_eq!(result, ValidationResult::Valid);
        
        let stats = mempool.get_stats();
        assert_eq!(stats.total_transactions, 1);
    }

    #[tokio::test]
    async fn test_duplicate_transaction_rejection() {
        let mempool = TransactionMempool::new();
        let keypair = Dilithium3Keypair::new().unwrap();
        
        // Create transaction with proper fee calculation
        let mut transaction = Transaction::new(
            keypair.public_key.clone(),
            TransactionType::Transfer {
                to: vec![0; 32],
                amount: 1000,
                memo: None,
            },
            1,
        );
        
        transaction.sign(&keypair).unwrap();
        
        // Add first time - should succeed
        let result1 = mempool.add_transaction(transaction.clone()).await.unwrap();
        assert_eq!(result1, ValidationResult::Valid);
        
        // Add second time - should reject
        let result2 = mempool.add_transaction(transaction).await.unwrap();
        assert_eq!(result2, ValidationResult::DuplicateTransaction);
    }
} 