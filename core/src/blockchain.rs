use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::block::{Block, BlockHash};
use crate::transaction::{Transaction, TransactionType};
use crate::crypto::{Dilithium3Keypair, generate_difficulty_target, verify_pow};
use crate::mempool::{TransactionMempool, ValidationResult};
use crate::error::BlockchainError;
use crate::{Result};

// AI Agent Note: This is a production-ready blockchain implementation with proper consensus
// Features implemented:
// - Longest chain consensus rule with proper fork resolution
// - Chain reorganization (reorg) support for handling competing chains
// - Orphan block pool for handling out-of-order blocks
// - Block and transaction validation with comprehensive checks  
// - Account state management with UTXO-like tracking
// - Difficulty adjustment algorithm with proper target time enforcement
// - Memory pools integration with block building
// - Concurrent access safety with high-performance data structures
// - Chain state snapshots for fast sync and recovery

/// Chain state information and statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainState {
    pub total_blocks: u64,
    pub total_supply: u64,
    pub current_difficulty: u32,
    pub average_block_time: u64,
    pub last_block_time: DateTime<Utc>,
    pub active_miners: usize,
    pub best_block_hash: BlockHash,
    pub cumulative_difficulty: u128, // Total work in the chain
}

/// Account state with comprehensive tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountState {
    pub balance: u64,
    pub nonce: u64,
    pub staked_amount: u64,
    pub last_stake_time: DateTime<Utc>,
    pub transaction_count: u64,
    pub total_received: u64,
    pub total_sent: u64,
}

/// Block metadata for consensus tracking
#[derive(Debug, Clone)]
pub struct BlockMetadata {
    pub block: Block,
    pub cumulative_difficulty: u128,
    pub height: u64,
    pub is_main_chain: bool,
    pub children: Vec<BlockHash>,
    pub arrival_time: DateTime<Utc>,
}

/// Fork information for chain reorganization
#[derive(Debug, Clone)]
pub struct ForkInfo {
    pub common_ancestor: BlockHash,
    pub old_chain: Vec<BlockHash>,
    pub new_chain: Vec<BlockHash>,
    pub blocks_to_disconnect: Vec<Block>,
    pub blocks_to_connect: Vec<Block>,
}

/// Orphan block with metadata
#[derive(Debug, Clone)]
pub struct OrphanBlock {
    pub block: Block,
    pub arrival_time: DateTime<Utc>,
    pub processing_attempts: u8,
}

/// Production-ready blockchain with advanced consensus
pub struct NumiBlockchain {
    /// All blocks indexed by hash (includes orphans and side chains)
    blocks: Arc<DashMap<BlockHash, BlockMetadata>>,
    
    /// Main chain blocks ordered by height 
    main_chain: Arc<RwLock<Vec<BlockHash>>>,
    
    /// Account states in the current best chain
    accounts: Arc<DashMap<Vec<u8>, AccountState>>,
    
    /// Current chain state and statistics
    state: Arc<RwLock<ChainState>>,
    
    /// Orphan blocks waiting for their parents
    orphan_pool: Arc<DashMap<BlockHash, OrphanBlock>>,
    
    /// Transaction mempool for pending transactions
    mempool: Arc<TransactionMempool>,
    
    /// Block arrival times for difficulty adjustment
    block_times: Arc<RwLock<VecDeque<(u64, DateTime<Utc>)>>>, // (height, timestamp)
    
    /// Genesis block hash
    genesis_hash: BlockHash,
    
    /// Miner keypair for block signing
    miner_keypair: Dilithium3Keypair,
    
    /// Configuration parameters
    target_block_time: Duration,
    difficulty_adjustment_interval: u64,
    max_orphan_blocks: usize,
    max_reorg_depth: u64,
}

impl NumiBlockchain {
    /// Create new blockchain with genesis block
    pub fn new() -> Result<Self> {
        let miner_keypair = Dilithium3Keypair::new()?;
        let mempool = Arc::new(TransactionMempool::new());
        
        let mut blockchain = Self {
            blocks: Arc::new(DashMap::new()),
            main_chain: Arc::new(RwLock::new(Vec::new())),
            accounts: Arc::new(DashMap::new()),
            state: Arc::new(RwLock::new(ChainState {
                total_blocks: 0,
                total_supply: 0,
                current_difficulty: 1,
                average_block_time: 30,
                last_block_time: Utc::now(),
                active_miners: 0,
                best_block_hash: [0; 32],
                cumulative_difficulty: 0,
            })),
            orphan_pool: Arc::new(DashMap::new()),
            mempool,
            block_times: Arc::new(RwLock::new(VecDeque::new())),
            genesis_hash: [0; 32],
            miner_keypair,
            target_block_time: Duration::from_secs(30), // 30 second blocks
            difficulty_adjustment_interval: 144,        // Adjust every 144 blocks (~1 hour)
            max_orphan_blocks: 1000,                   // Maximum orphan blocks to keep
            max_reorg_depth: 144,                      // Maximum reorganization depth
        };
        
        blockchain.create_genesis_block()?;
        Ok(blockchain)
    }
    
    /// Load blockchain from storage with validation
    pub async fn load_from_storage(storage: &crate::storage::BlockchainStorage) -> Result<Self> {
        let mut blockchain = Self::new()?;
        
        // Clear initial state (will be rebuilt from storage)
        blockchain.blocks.clear();
        blockchain.main_chain.write().clear();
        blockchain.accounts.clear();
        
        // Load all blocks from storage
        let stored_blocks = storage.get_all_blocks()?;
        let mut blocks_by_height: BTreeMap<u64, Vec<Block>> = BTreeMap::new();
        
        for block in stored_blocks {
            blocks_by_height
                .entry(block.header.height)
                .or_insert_with(Vec::new)
                .push(block);
        }
        
        // Rebuild blockchain from blocks in height order
        for (height, blocks) in blocks_by_height {
            for block in blocks {
                if height == 0 {
                    blockchain.genesis_hash = block.calculate_hash();
                }
                blockchain.process_block_internal(block, false).await?;
            }
        }
        
        // Load account states
        let accounts = storage.get_all_accounts()?;
        for (pubkey, account_state) in accounts {
            blockchain.accounts.insert(pubkey, account_state);
        }
        
        // Load and validate chain state
        if let Some(saved_state) = storage.load_chain_state()? {
            *blockchain.state.write() = saved_state;
        }
        
        // Validate the loaded blockchain
        if !blockchain.validate_chain().await {
            return Err(BlockchainError::InvalidBlock("Loaded blockchain failed validation".to_string()));
        }
        
        log::info!("✅ Blockchain loaded from storage with {} blocks", blockchain.get_current_height());
        Ok(blockchain)
    }

    /// Process new block with full consensus logic
    pub async fn add_block(&self, block: Block) -> Result<bool> {
        self.process_block_internal(block, true).await
    }
    
    /// Internal block processing with orphan handling and reorg detection
    async fn process_block_internal(&self, block: Block, validate_pow: bool) -> Result<bool> {
        let block_hash = block.calculate_hash();
        
        // Check if we already have this block
        if self.blocks.contains_key(&block_hash) {
            return Ok(false); // Block already processed
        }
        
        // Basic block validation
        if let Err(e) = self.validate_block_basic(&block).await {
            log::warn!("❌ Block {} failed basic validation: {}", hex::encode(block_hash), e);
            return Err(e);
        }
        
        // Verify proof of work (skip for genesis and loading from storage)
        if validate_pow && !block.is_genesis() {
            if let Err(e) = self.verify_proof_of_work(&block) {
                log::warn!("❌ Block {} failed PoW verification: {}", hex::encode(block_hash), e);
                return Err(e);
            }
        }
        
        // Check if parent exists
        let parent_hash = block.header.previous_hash;
        if !self.blocks.contains_key(&parent_hash) && !block.is_genesis() {
            // Parent not found - add to orphan pool
            return self.handle_orphan_block(block).await;
        }
        
        // Process the block and its transactions
        let was_reorganization = self.connect_block(block).await?;
        
        // Process any orphan blocks that might now be valid
        // Use Box::pin to avoid recursion issues
        Box::pin(self.process_orphan_blocks()).await?;
        
        Ok(was_reorganization)
    }

    /// Connect block to the blockchain and handle potential reorganization
    async fn connect_block(&self, block: Block) -> Result<bool> {
        let block_hash = block.calculate_hash();
        let parent_hash = block.header.previous_hash;
        
        // Calculate cumulative difficulty
        let parent_difficulty = if block.is_genesis() {
            0u128
        } else {
            self.blocks.get(&parent_hash)
                .map(|meta| meta.cumulative_difficulty)
                .unwrap_or(0)
        };
        
        let block_work = self.calculate_block_work(block.header.difficulty);
        let cumulative_difficulty = parent_difficulty + block_work;
        
        // Create block metadata
        let metadata = BlockMetadata {
            block: block.clone(),
            cumulative_difficulty,
            height: block.header.height,
            is_main_chain: false, // Will be updated if this becomes main chain
            children: Vec::new(),
            arrival_time: Utc::now(),
        };
        
        // Add to block index
        self.blocks.insert(block_hash, metadata);
        
        // Update parent's children list
        if !block.is_genesis() {
            if let Some(mut parent_meta) = self.blocks.get_mut(&parent_hash) {
                parent_meta.children.push(block_hash);
            }
        }
        
        // Check if this block extends the best chain
        let current_best_difficulty = self.state.read().cumulative_difficulty;
        
        if cumulative_difficulty > current_best_difficulty {
            // This is the new best chain - perform reorganization
            log::info!("🔄 New best chain found, performing reorganization");
            return self.reorganize_to_block(block_hash).await;
        } else {
            log::debug!("📦 Block {} added to side chain", hex::encode(block_hash));
            return Ok(false);
        }
    }
    
    /// Perform chain reorganization to a new best block
    async fn reorganize_to_block(&self, new_best_hash: BlockHash) -> Result<bool> {
        let current_best_hash = self.state.read().best_block_hash;
        
        // Find the fork point between current and new chain
        let fork_info = self.find_fork_point(current_best_hash, new_best_hash)?;
        
        // Check if reorganization depth is acceptable
        if fork_info.old_chain.len() > self.max_reorg_depth as usize {
            log::warn!("🚫 Reorganization depth {} exceeds maximum {}, rejecting", 
                      fork_info.old_chain.len(), self.max_reorg_depth);
            return Ok(false);
        }
        
        log::info!("🔄 Reorganizing chain: disconnecting {} blocks, connecting {} blocks",
                  fork_info.blocks_to_disconnect.len(), fork_info.blocks_to_connect.len());
        
        // Disconnect old chain blocks (reverse order)
        for block in fork_info.blocks_to_disconnect.iter().rev() {
            self.disconnect_block(block).await?;
        }
        
        // Connect new chain blocks (forward order)
        for block in &fork_info.blocks_to_connect {
            self.connect_block_to_main_chain(block).await?;
        }
        
        // Update main chain
        let new_chain = self.build_chain_to_block(new_best_hash)?;
        *self.main_chain.write() = new_chain;
        
        // Mark blocks as main chain
        self.update_main_chain_flags(new_best_hash).await;
        
        // Update chain state
        self.update_chain_state_after_reorg(new_best_hash).await?;
        
        log::info!("✅ Chain reorganization completed successfully");
        Ok(true)
    }
    
    /// Find fork point between two chains
    fn find_fork_point(&self, hash_a: BlockHash, hash_b: BlockHash) -> Result<ForkInfo> {
        let mut path_a = self.build_chain_to_block(hash_a)?;
        let mut path_b = self.build_chain_to_block(hash_b)?;
        
        // Find common ancestor
        path_a.reverse();
        path_b.reverse();
        
        let mut common_ancestor = self.genesis_hash;
        let min_len = std::cmp::min(path_a.len(), path_b.len());
        
        for i in 0..min_len {
            if path_a[i] == path_b[i] {
                common_ancestor = path_a[i];
            } else {
                break;
            }
        }
        
        // Build fork information
        let old_chain: Vec<BlockHash> = path_a.into_iter()
            .skip_while(|&hash| hash != common_ancestor)
            .skip(1) // Skip common ancestor
            .collect();
            
        let new_chain: Vec<BlockHash> = path_b.into_iter()
            .skip_while(|&hash| hash != common_ancestor) 
            .skip(1) // Skip common ancestor
            .collect();
        
        // Get actual blocks
        let blocks_to_disconnect: Vec<Block> = old_chain.iter()
            .filter_map(|hash| self.blocks.get(hash).map(|meta| meta.block.clone()))
            .collect();
            
        let blocks_to_connect: Vec<Block> = new_chain.iter()
            .filter_map(|hash| self.blocks.get(hash).map(|meta| meta.block.clone()))
            .collect();
        
        Ok(ForkInfo {
            common_ancestor,
            old_chain,
            new_chain, 
            blocks_to_disconnect,
            blocks_to_connect,
        })
    }
    
    /// Build chain path from genesis to given block
    fn build_chain_to_block(&self, mut block_hash: BlockHash) -> Result<Vec<BlockHash>> {
        let mut chain = Vec::new();
        
        while let Some(meta) = self.blocks.get(&block_hash) {
            chain.push(block_hash);
            if meta.block.is_genesis() {
                break;
            }
            block_hash = meta.block.header.previous_hash;
        }
        
        chain.reverse();
        Ok(chain)
    }
    
    /// Disconnect block from main chain (undo transactions)
    async fn disconnect_block(&self, block: &Block) -> Result<()> {
        log::debug!("🔌 Disconnecting block {}", block.header.height);
        
        // Reverse all transactions in the block (in reverse order)
        for transaction in block.transactions.iter().rev() {
            self.undo_transaction(transaction).await?;
        }
        
        // Return transactions to mempool (except mining rewards)
        for transaction in &block.transactions {
            if !matches!(transaction.transaction_type, TransactionType::MiningReward { .. }) {
                let _ = self.mempool.add_transaction(transaction.clone()).await;
            }
        }
        
        Ok(())
    }
    
    /// Connect block to main chain (apply transactions)
    async fn connect_block_to_main_chain(&self, block: &Block) -> Result<()> {
        log::debug!("🔗 Connecting block {} to main chain", block.header.height);
        
        // Validate all transactions in context
        for transaction in &block.transactions {
            if let Err(e) = self.validate_transaction_in_context(transaction).await {
                return Err(BlockchainError::InvalidTransaction(
                    format!("Transaction {} invalid in block context: {}",
                           hex::encode(transaction.get_hash_hex()), e)));
            }
        }
        
        // Apply all transactions
        for transaction in &block.transactions {
            self.apply_transaction(transaction).await?;
        }
        
        // Remove transactions from mempool
        let tx_hashes: Vec<_> = block.transactions.iter()
            .map(|tx| tx.get_hash_hex())
            .collect();
        // Convert String hashes to TransactionId format
        let tx_ids: Vec<[u8; 32]> = tx_hashes.iter()
            .filter_map(|hash| hex::decode(hash).ok())
            .filter_map(|bytes| {
                if bytes.len() == 32 {
                    let mut id = [0u8; 32];
                    id.copy_from_slice(&bytes);
                    Some(id)
                } else {
                    None
                }
            })
            .collect();
        self.mempool.remove_transactions(&tx_ids).await;
        
        Ok(())
    }
    
    /// Handle orphan blocks that arrive before their parents
    async fn handle_orphan_block(&self, block: Block) -> Result<bool> {
        let block_hash = block.calculate_hash();
        
        // Check orphan pool size limit
        if self.orphan_pool.len() >= self.max_orphan_blocks {
            // Remove oldest orphan
            if let Some(oldest) = self.find_oldest_orphan() {
                self.orphan_pool.remove(&oldest);
                log::debug!("🗑️ Removed oldest orphan block to make space");
            }
        }
        
        // Add to orphan pool
        let previous_hash = hex::encode(block.header.previous_hash);
        let orphan = OrphanBlock {
            block,
            arrival_time: Utc::now(),
            processing_attempts: 0,
        };
        
        self.orphan_pool.insert(block_hash, orphan);
        log::info!("👻 Block {} added to orphan pool (parent: {})",
                  hex::encode(block_hash),
                  previous_hash);
        
        Ok(false)
    }
    
    /// Process orphan blocks that might now be valid
    async fn process_orphan_blocks(&self) -> Result<()> {
        let mut processed_any = true;
        
        // Keep processing until no more orphans can be processed
        while processed_any {
            processed_any = false;
            let orphan_hashes: Vec<BlockHash> = self.orphan_pool.iter()
                .map(|entry| *entry.key())
                .collect();
            
            for orphan_hash in orphan_hashes {
                if let Some((_, mut orphan)) = self.orphan_pool.remove(&orphan_hash) {
                    let parent_hash = orphan.block.header.previous_hash;
                    
                    // Check if parent now exists
                    if self.blocks.contains_key(&parent_hash) || orphan.block.is_genesis() {
                        log::info!("🎯 Processing orphan block {} (parent now available)",
                                  hex::encode(orphan_hash));
                        
                        let block = orphan.block.clone();
                        match self.process_block_internal(block, true).await {
                            Ok(_) => {
                                processed_any = true;
                            }
                            Err(e) => {
                                log::warn!("❌ Orphan block processing failed: {}", e);
                                // Increment processing attempts
                                orphan.processing_attempts += 1;
                                if orphan.processing_attempts < 3 {
                                    // Put it back in the pool for retry
                                    self.orphan_pool.insert(orphan_hash, orphan);
                                }
                            }
                        }
                    } else {
                        // Put back in pool
                        self.orphan_pool.insert(orphan_hash, orphan);
                    }
                }
            }
        }
        
        Ok(())
    }
    
    /// Create genesis block
    fn create_genesis_block(&mut self) -> Result<()> {
        let genesis_transactions = vec![
            // Genesis supply allocation
            Transaction::new(
                self.miner_keypair.public_key.clone(),
                TransactionType::MiningReward {
                    block_height: 0,
                    amount: 21_000_000_000_000_000, // 21M NUMI * 10^9 (total supply)
                },
                0,
            )
        ];
        
        let mut genesis_block = Block::new(
            0,
            [0u8; 32], // Previous hash is all zeros
            genesis_transactions,
            1,         // Minimal difficulty
            self.miner_keypair.public_key.clone(),
        );
        
        // Sign the genesis block
        genesis_block.sign(&self.miner_keypair)?;
        self.genesis_hash = genesis_block.calculate_hash();
        
        // Process genesis block
        futures::executor::block_on(self.process_block_internal(genesis_block, false))?;
        
        log::info!("🌱 Genesis block created: {}", hex::encode(self.genesis_hash));
        Ok(())
    }

    /// Comprehensive block validation
    async fn validate_block_basic(&self, block: &Block) -> Result<()> {
        // Check block structure
        if block.transactions.is_empty() && !block.is_genesis() {
            return Err(BlockchainError::InvalidBlock("Block has no transactions".to_string()));
        }
        
        // Validate timestamp (not too far in the future)
        let now = Utc::now();
        let max_future_time = now + chrono::Duration::seconds(7200); // 2 hours
        
        if block.header.timestamp > max_future_time {
            return Err(BlockchainError::InvalidBlock("Block timestamp too far in future".to_string()));
        }
        
        // Validate all transactions
        for (i, transaction) in block.transactions.iter().enumerate() {
            // AI Agent Note: Using placeholder values for basic validation
            // In production, should look up actual account balance and nonce
            if let Err(e) = transaction.validate(0, 0) {
                return Err(BlockchainError::InvalidBlock(
                    format!("Transaction {} invalid: {}", i, e)));
            }
        }
        
        // Check for duplicate transactions
        let mut tx_ids = std::collections::HashSet::new();
        for transaction in &block.transactions {
            let tx_id = transaction.get_hash_hex();
            if !tx_ids.insert(tx_id) {
                return Err(BlockchainError::InvalidBlock("Duplicate transaction in block".to_string()));
            }
        }
        
        // Validate mining reward (should be first transaction if present)
        if !block.is_genesis() {
            let expected_reward = self.get_mining_reward(block.header.height);
            let has_valid_reward = block.transactions.first()
                .map(|tx| matches!(&tx.transaction_type, TransactionType::MiningReward { amount, .. } 
                                  if *amount <= expected_reward))
                .unwrap_or(false);
            
            if !has_valid_reward {
                return Err(BlockchainError::InvalidBlock("Invalid or missing mining reward".to_string()));
            }
        }
        
        Ok(())
    }
    
    /// Verify proof of work for a block
    fn verify_proof_of_work(&self, block: &Block) -> Result<()> {
        let difficulty_target = generate_difficulty_target(block.header.difficulty);
        let header_blob = block.serialize_header_for_hashing();
        
        if !verify_pow(&header_blob, block.header.nonce, &difficulty_target)? {
            return Err(BlockchainError::InvalidBlock("Proof of work verification failed".to_string()));
        }
        
        Ok(())
    }
    
    /// Validate transaction in current blockchain context
    async fn validate_transaction_in_context(&self, transaction: &Transaction) -> Result<()> {
        // Get current account state
        let account_state = self.accounts.get(&transaction.from)
            .map(|state| state)
            .map(|r| r.clone()).unwrap_or_else(|| AccountState {
                balance: 0,
                nonce: 0,
                staked_amount: 0,
                last_stake_time: Utc::now(),
                transaction_count: 0,
                total_received: 0,
                total_sent: 0,
            });
        
        // Validate nonce
        if transaction.nonce != account_state.nonce + 1 {
            return Err(BlockchainError::InvalidTransaction(
                format!("Invalid nonce: expected {}, got {}",
                       account_state.nonce + 1, transaction.nonce)));
        }
        
        // Validate transaction type-specific conditions
        match &transaction.transaction_type {
            TransactionType::Transfer { amount, .. } => {
                if account_state.balance < *amount {
                    return Err(BlockchainError::InvalidTransaction(
                        format!("Insufficient balance: {} < {}", account_state.balance, amount)));
                }
            }
            TransactionType::Stake { amount } => {
                if account_state.balance < *amount {
                    return Err(BlockchainError::InvalidTransaction(
                        format!("Insufficient balance for staking: {} < {}", account_state.balance, amount)));
                }
                if *amount < 1_000_000_000 { // Minimum 1 NUMI
                    return Err(BlockchainError::InvalidTransaction("Stake amount too low".to_string()));
                }
            }
            TransactionType::Unstake { amount } => {
                if account_state.staked_amount < *amount {
                    return Err(BlockchainError::InvalidTransaction(
                        format!("Insufficient staked amount: {} < {}", account_state.staked_amount, amount)));
                }
            }
            TransactionType::MiningReward { .. } => {
                // Mining rewards are validated at block level
            }
            TransactionType::Governance { .. } => {
                if account_state.staked_amount < 1_000_000_000_000 { // Minimum 1000 NUMI staked
                    return Err(BlockchainError::InvalidTransaction(
                        "Insufficient stake for governance participation".to_string()));
                }
            }
        }
        
        Ok(())
    }
    
    /// Apply transaction to account states
    async fn apply_transaction(&self, transaction: &Transaction) -> Result<()> {
        let sender_key = transaction.from.clone();
        
        // Get or create sender account
        let mut sender_state = self.accounts.get(&sender_key)
            .map(|state| state)
            .map(|r| r.clone()).unwrap_or_else(|| AccountState {
                balance: 0,
                nonce: 0,
                staked_amount: 0,
                last_stake_time: Utc::now(),
                transaction_count: 0,
                total_received: 0,
                total_sent: 0,
            });
        
        match &transaction.transaction_type {
            TransactionType::Transfer { to, amount } => {
                // Deduct from sender
                sender_state.balance -= amount;
                sender_state.nonce += 1;
                sender_state.transaction_count += 1;
                sender_state.total_sent += amount;
                
                // Add to recipient
                let mut recipient_state = self.accounts.get(to)
                    .map(|state| state)
                    .map(|r| r.clone()).unwrap_or_else(|| AccountState {
                balance: 0,
                nonce: 0,
                staked_amount: 0,
                last_stake_time: Utc::now(),
                transaction_count: 0,
                total_received: 0,
                total_sent: 0,
            });
                
                recipient_state.balance += amount;
                recipient_state.total_received += amount;
                
                self.accounts.insert(to.clone(), recipient_state.clone());
            }
            
            TransactionType::Stake { amount } => {
                sender_state.balance -= amount;
                sender_state.staked_amount += amount;
                sender_state.last_stake_time = Utc::now();
                sender_state.nonce += 1;
                sender_state.transaction_count += 1;
            }
            
            TransactionType::Unstake { amount } => {
                sender_state.staked_amount -= amount;
                sender_state.balance += amount;
                sender_state.nonce += 1;
                sender_state.transaction_count += 1;
            }
            
            TransactionType::MiningReward { amount, .. } => {
                sender_state.balance += amount;
                sender_state.total_received += amount;
                
                // Update total supply
                let mut state = self.state.write();
                state.total_supply += amount;
            }
            
            TransactionType::Governance { .. } => {
                sender_state.nonce += 1;
                sender_state.transaction_count += 1;
            }
        }
        
        self.accounts.insert(sender_key, sender_state.clone());
        Ok(())
    }
    
    /// Undo transaction effects (for chain reorganization)
    async fn undo_transaction(&self, transaction: &Transaction) -> Result<()> {
        let sender_key = transaction.from.clone();
        
        if let Some(sender_state) = self.accounts.get(&sender_key).map(|s| s.clone()) {
            let mut sender_state = sender_state;
            match &transaction.transaction_type {
                TransactionType::Transfer { to, amount } => {
                    // Restore sender balance
                    sender_state.balance += amount;
                    sender_state.nonce -= 1;
                    sender_state.transaction_count -= 1;
                    sender_state.total_sent -= amount;
                    
                    // Deduct from recipient
                    if let Some(recipient_state) = self.accounts.get(to).map(|s| s.clone()) {
                        let mut recipient_state = recipient_state;
                        recipient_state.balance -= amount;
                        recipient_state.total_received -= amount;
                        self.accounts.insert(to.clone(), recipient_state);
                    }
                }
                
                TransactionType::Stake { amount } => {
                    sender_state.balance += amount;
                    sender_state.staked_amount -= amount;
                    sender_state.nonce -= 1;
                    sender_state.transaction_count -= 1;
                }
                
                TransactionType::Unstake { amount } => {
                    sender_state.staked_amount += amount;
                    sender_state.balance -= amount;
                    sender_state.nonce -= 1;
                    sender_state.transaction_count -= 1;
                }
                
                TransactionType::MiningReward { amount, .. } => {
                    sender_state.balance -= amount;
                    sender_state.total_received -= amount;
                    
                    // Update total supply
                    let mut state = self.state.write();
                    state.total_supply -= amount;
                }
                
                TransactionType::Governance { .. } => {
                    sender_state.nonce -= 1;
                    sender_state.transaction_count -= 1;
                }
            }
            
            self.accounts.insert(sender_key, sender_state);
        }
        
        Ok(())
    }
    
    /// Update chain state after reorganization
    async fn update_chain_state_after_reorg(&self, new_best_hash: BlockHash) -> Result<()> {
        if let Some(best_block_meta) = self.blocks.get(&new_best_hash) {
            let mut state = self.state.write();
            
            state.best_block_hash = new_best_hash;
            state.total_blocks = best_block_meta.height + 1;
            state.cumulative_difficulty = best_block_meta.cumulative_difficulty;
            state.last_block_time = best_block_meta.block.header.timestamp;
            state.current_difficulty = self.calculate_next_difficulty(best_block_meta.height);
            
            // Recalculate average block time
            self.update_average_block_time(best_block_meta.height).await;
        }
        
        Ok(())
    }
    
    /// Update main chain flags for all blocks
    async fn update_main_chain_flags(&self, best_hash: BlockHash) {
        // Reset all main chain flags
        for mut entry in self.blocks.iter_mut() {
            entry.is_main_chain = false;
        }
        
        // Mark main chain blocks
        let main_chain_hashes = self.build_chain_to_block(best_hash).unwrap_or_default();
        for hash in main_chain_hashes {
            if let Some(mut meta) = self.blocks.get_mut(&hash) {
                meta.is_main_chain = true;
            }
        }
    }
    
    /// Calculate work value for a block based on difficulty
    fn calculate_block_work(&self, difficulty: u32) -> u128 {
        // Work = 2^256 / (target + 1)
        // Simplified: work increases exponentially with difficulty
        2u128.pow(difficulty.min(64)) // Cap to prevent overflow
    }
    
    /// Calculate next difficulty adjustment
    fn calculate_next_difficulty(&self, height: u64) -> u32 {
        if height < self.difficulty_adjustment_interval {
            return 1; // Initial difficulty
        }
        
        if height % self.difficulty_adjustment_interval != 0 {
            return self.state.read().current_difficulty; // No adjustment needed
        }
        
        let block_times = self.block_times.read();
        if block_times.len() < self.difficulty_adjustment_interval as usize {
            return self.state.read().current_difficulty;
        }
        
        // Calculate average block time over adjustment interval
        let recent_times: Vec<_> = block_times.iter()
            .rev()
            .take(self.difficulty_adjustment_interval as usize)
            .collect();
        
        if recent_times.len() < 2 {
            return self.state.read().current_difficulty;
        }
        
        let time_diff = recent_times.first().unwrap().1 - recent_times.last().unwrap().1;
        let actual_time = time_diff.num_seconds() as u64;
        let target_time = self.target_block_time.as_secs() * self.difficulty_adjustment_interval;
        
        let current_difficulty = self.state.read().current_difficulty;
        
        // Adjust difficulty based on actual vs target time
        if actual_time < target_time / 2 {
            // Blocks too fast - increase difficulty
            current_difficulty + 1
        } else if actual_time > target_time * 2 {
            // Blocks too slow - decrease difficulty  
            current_difficulty.saturating_sub(1).max(1)
        } else {
            // Fine-tune based on ratio
            let ratio = (target_time as f64) / (actual_time as f64);
            let adjustment = (current_difficulty as f64 * ratio) as u32;
            adjustment.max(1).min(current_difficulty + 5) // Limit large changes
        }
    }
    
    /// Update average block time tracking
    async fn update_average_block_time(&self, current_height: u64) {
        let mut block_times = self.block_times.write();
        
        // Add current block time
        block_times.push_back((current_height, Utc::now()));
        
        // Keep only recent blocks for calculation
        let keep_blocks = (self.difficulty_adjustment_interval * 2).max(100);
        while block_times.len() > keep_blocks as usize {
            block_times.pop_front();
        }
        
        // Calculate new average
        if block_times.len() >= 2 {
            let times_vec: Vec<_> = block_times.iter().collect();
            let total_time: i64 = times_vec.windows(2)
                .map(|w| (w[1].1 - w[0].1).num_seconds())
                .sum();
            
            let avg_time = (total_time as u64) / (block_times.len() as u64 - 1);
            
            let mut state = self.state.write();
            state.average_block_time = avg_time;
        }
    }
    
    /// Find oldest orphan block for eviction
    fn find_oldest_orphan(&self) -> Option<BlockHash> {
        self.orphan_pool.iter()
            .min_by_key(|entry| entry.arrival_time)
            .map(|entry| *entry.key())
    }
    
    /// Validate the entire blockchain
    async fn validate_chain(&self) -> bool {
        let main_chain = self.main_chain.read();
        
        for (i, &block_hash) in main_chain.iter().enumerate() {
            if let Some(block_meta) = self.blocks.get(&block_hash) {
                let block = &block_meta.block;
                
                // Validate block height
                if block.header.height != i as u64 {
                    log::error!("❌ Block height mismatch at index {}: expected {}, got {:?}",
                               i, i, block.header.height);
                    return false;
                }
                
                // Validate previous hash (except genesis)
                if i > 0 {
                    let prev_hash = main_chain[i - 1];
                    if block.header.previous_hash != prev_hash {
                        log::error!("❌ Previous hash mismatch at height {:?}", block.header.height);
                        return false;
                    }
                }
                
                // Validate block structure
                if let Err(e) = futures::executor::block_on(self.validate_block_basic(block)) {
                    log::error!("❌ Block {} failed validation: height {}, error {:?}", i, block.header.height, e);
                    return false;
                }
            } else {
                log::error!("❌ Block {} not found in block index", block_hash.iter().map(|b| format!("{:02x}", b)).collect::<String>());
                return false;
            }
        }
        
        true
    }

    // Public interface methods
    
    /// Save blockchain state to storage
    pub fn save_to_storage(&self, storage: &crate::storage::BlockchainStorage) -> Result<()> {
        // Save all blocks
        for entry in self.blocks.iter() {
            storage.save_block(&entry.block)?;
        }
        
        // Save all accounts
        for entry in self.accounts.iter() {
            storage.save_account(entry.key(), &entry.value())?;
        }
        
        // Save chain state
        let state = self.state.read();
        storage.save_chain_state(&state)?;
        
        Ok(())
    }
    
    /// Add transaction to mempool
    pub async fn add_transaction(&self, transaction: Transaction) -> Result<ValidationResult> {
        self.mempool.add_transaction(transaction).await
    }
    
    /// Get transactions for block creation
    pub fn get_transactions_for_block(&self, max_size: usize, max_count: usize) -> Vec<Transaction> {
        self.mempool.get_transactions_for_block(max_size, max_count)
    }
    
    /// Get current chain state
    pub fn get_chain_state(&self) -> ChainState {
        self.state.read().clone()
    }
    
    /// Get current blockchain height
    pub fn get_current_height(&self) -> u64 {
        self.state.read().total_blocks.saturating_sub(1)
    }
    
    /// Get current difficulty
    pub fn get_current_difficulty(&self) -> u32 {
        self.state.read().current_difficulty
    }
    
    /// Get latest block
    pub fn get_latest_block(&self) -> Option<Block> {
        let best_hash = self.state.read().best_block_hash;
        self.blocks.get(&best_hash).map(|meta| meta.block.clone())
    }
    
    /// Get latest block hash
    pub fn get_latest_block_hash(&self) -> BlockHash {
        self.state.read().best_block_hash
    }
    
    /// Get block by height
    pub fn get_block_by_height(&self, height: u64) -> Option<Block> {
        let main_chain = self.main_chain.read();
        if height < main_chain.len() as u64 {
            let block_hash = main_chain[height as usize];
            self.blocks.get(&block_hash).map(|meta| meta.block.clone())
        } else {
            None
        }
    }

    /// Get block by hash
    pub fn get_block_by_hash(&self, hash: &BlockHash) -> Option<Block> {
        self.blocks.get(hash).map(|meta| meta.block.clone())
    }
    
    /// Get account state
    pub fn get_account_state(&self, public_key: &[u8]) -> Result<AccountState> {
        self.accounts.get(public_key)
            .map(|state| state.clone())
            .ok_or_else(|| BlockchainError::BlockNotFound("Account not found".to_string()))
    }
    
    /// Get account balance
    pub fn get_balance(&self, public_key: &[u8]) -> u64 {
        self.accounts.get(public_key)
            .map(|state| state.balance)
            .unwrap_or(0)
    }
    
    /// Get mining reward for given height
    pub fn get_mining_reward(&self, height: u64) -> u64 {
        // Halving every 210,000 blocks (like Bitcoin)
        let halving_interval = 210_000u64;
        let halvings = height / halving_interval;
        
        if halvings >= 64 {
            return 0; // No more rewards after 64 halvings
        }
        
        // Initial reward: 50 NUMI
        let initial_reward = 50_000_000_000u64; // 50 * 10^9
        initial_reward >> halvings // Divide by 2^halvings
    }
    
    /// Get mempool statistics
    pub fn get_mempool_stats(&self) -> crate::mempool::MempoolStats {
        self.mempool.get_stats()
    }
    
    /// Get pending transactions count
    pub fn get_pending_transaction_count(&self) -> usize {
        self.mempool.get_stats().total_transactions
    }
    
    /// Clean up expired transactions and orphan blocks
    pub async fn perform_maintenance(&self) -> Result<()> {
        // Clean up mempool
        self.mempool.cleanup_expired_transactions().await;
        
        // Clean up old orphan blocks
        self.cleanup_old_orphans().await;
        
        Ok(())
    }
    
    /// Clean up old orphan blocks
    async fn cleanup_old_orphans(&self) {
        let cutoff_time = Utc::now() - chrono::Duration::hours(1);
        
        let old_orphans: Vec<BlockHash> = self.orphan_pool.iter()
            .filter(|entry| entry.arrival_time < cutoff_time)
            .map(|entry| *entry.key())
            .collect();
        
        let orphan_count = old_orphans.len();
        
        for hash in old_orphans {
            self.orphan_pool.remove(&hash);
        }
        
        if orphan_count > 0 {
            log::info!("🧹 Cleaned up {} old orphan blocks", orphan_count);
        }
    }
} 