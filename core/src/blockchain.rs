use std::collections::{BTreeMap, VecDeque, HashMap};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::block::{Block, BlockHash};
use crate::transaction::{Transaction, TransactionType, MAX_TRANSACTION_SIZE};
use crate::crypto::{Dilithium3Keypair, generate_difficulty_target, verify_pow, blake3_hash};
use crate::mempool::{TransactionMempool, ValidationResult};
use crate::error::BlockchainError;
use crate::{Result};

use num_traits::{Zero, ToPrimitive};

/// Maximum blocks that can be processed per second (DoS protection)
const MAX_BLOCKS_PER_SECOND: usize = 10;

/// Maximum orphan blocks per peer (prevent memory exhaustion)
const MAX_ORPHAN_BLOCKS_PER_PEER: usize = 100;

/// Checkpoint interval in blocks
const CHECKPOINT_INTERVAL: u64 = 1000;

/// Maximum number of checkpoints to keep
const MAX_CHECKPOINTS: usize = 100;

/// Maximum processing attempts for orphan blocks
const MAX_PROCESSING_ATTEMPTS: usize = 100; // Prevent infinite loops

/// Finality depth - blocks beyond this are considered final
const FINALITY_DEPTH: u64 = 2016; // ~1 week at 30s blocks

/// Maximum block processing time in milliseconds (DoS protection)
const MAX_BLOCK_PROCESSING_TIME_MS: u64 = 10000; // 10 seconds

/// Security checkpoint for preventing long-range attacks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityCheckpoint {
    pub block_height: u64,
    pub block_hash: BlockHash,
    pub cumulative_difficulty: u128,
    pub timestamp: DateTime<Utc>,
    pub total_supply: u64,

    /// Merkle root of account states at this checkpoint
    pub state_root: [u8; 32],
}

impl SecurityCheckpoint {
    pub fn new(
        block_height: u64,
        block_hash: BlockHash,
        cumulative_difficulty: u128,
        total_supply: u64,
        state_root: [u8; 32],
    ) -> Self {
        Self {
            block_height,
            block_hash,
            cumulative_difficulty,
            timestamp: Utc::now(),
            total_supply,
            state_root,
        }
    }
    
    /// Validate checkpoint against current chain state
    pub fn validate(&self, current_height: u64, current_difficulty: u128) -> Result<()> {
        if self.block_height > current_height {
            return Err(BlockchainError::InvalidBlock("Checkpoint from future".to_string()));
        }
        
        if self.cumulative_difficulty > current_difficulty {
            return Err(BlockchainError::InvalidBlock("Checkpoint difficulty too high".to_string()));
        }
        
        // Validate timestamp is reasonable
        let now = Utc::now();
        if self.timestamp > now + chrono::Duration::minutes(10) {
            return Err(BlockchainError::InvalidBlock("Checkpoint timestamp too far in future".to_string()));
        }
        
        Ok(())
    }
}

/// Enhanced chain state with security metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainState {
    pub total_blocks: u64,
    pub total_supply: u64,
    pub current_difficulty: u32,
    pub average_block_time: u64,
    pub last_block_time: DateTime<Utc>,
    pub active_miners: usize,
    pub best_block_hash: BlockHash,
    pub cumulative_difficulty: u128,
    /// Last finalized block (beyond reorganization)
    pub finalized_block_hash: BlockHash,
    pub finalized_block_height: u64,

    /// Current network hash rate estimate
    pub network_hash_rate: u64,
}

impl Default for ChainState {
    fn default() -> Self {
        Self {
            total_blocks: 0,
            total_supply: 0,
            current_difficulty: 8, // Match the initial difficulty from calculate_next_difficulty
            average_block_time: 30,
            last_block_time: Utc::now(),
            active_miners: 0,
            best_block_hash: [0; 32],
            cumulative_difficulty: 0,
            finalized_block_hash: [0; 32],
            finalized_block_height: 0,

            network_hash_rate: 0,
        }
    }
}

/// Account state with comprehensive tracking and security features
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountState {
    pub balance: u64,
    pub nonce: u64,
    pub transaction_count: u64,
    pub total_received: u64,
    pub total_sent: u64,
    /// Account creation time
    pub created_at: DateTime<Utc>,
    /// Last activity timestamp
    pub last_activity: DateTime<Utc>,
}

impl Default for AccountState {
    fn default() -> Self {
        let now = Utc::now();
        Self {
            balance: 0,
            nonce: 0,
            transaction_count: 0,
            total_received: 0,
            total_sent: 0,
            created_at: now,
            last_activity: now,
        }
    }
}



/// Enhanced block metadata with security and performance features
#[derive(Debug, Clone)]
pub struct BlockMetadata {
    pub block: Block,
    pub cumulative_difficulty: u128,
    pub height: u64,
    pub is_main_chain: bool,
    pub is_finalized: bool,
    pub children: Vec<BlockHash>,
    pub arrival_time: DateTime<Utc>,
    pub processing_time_ms: u64,
    pub peer_id: Option<String>, // Which peer sent this block
    pub validation_cache: Option<ValidationCache>,
}

/// Validation result cache to avoid recomputation
#[derive(Debug, Clone)]
pub struct ValidationCache {
    pub signature_valid: bool,
    pub pow_valid: bool,
    pub structure_valid: bool,
    pub cached_at: DateTime<Utc>,
}

/// Enhanced fork information with security analysis
#[derive(Debug, Clone)]
pub struct ForkInfo {
    pub common_ancestor: BlockHash,
    pub old_chain: Vec<BlockHash>,
    pub new_chain: Vec<BlockHash>,
    pub blocks_to_disconnect: Vec<Block>,
    pub blocks_to_connect: Vec<Block>,
    pub difficulty_change: i128, // Signed difficulty change
    pub is_long_range_attack: bool,
}

/// Enhanced orphan block with DoS protection
#[derive(Debug, Clone)]
pub struct OrphanBlock {
    pub block: Block,
    pub arrival_time: DateTime<Utc>,
    pub processing_attempts: u8,
    pub peer_id: Option<String>,
    pub size_bytes: usize,
}

/// DoS protection metrics per peer
#[derive(Debug, Clone)]
pub struct PeerMetrics {
    pub blocks_received: u64,
    pub invalid_blocks: u64,
    pub last_block_time: DateTime<Utc>,
    pub orphan_blocks: u64,
    pub processing_time_total: u64,
    pub rate_limit_violations: u64,
}

/// Production-ready blockchain with enhanced security and performance
pub struct NumiBlockchain {
    /// All blocks indexed by hash (includes orphans and side chains)
    blocks: Arc<DashMap<BlockHash, BlockMetadata>>,
    
    /// Main chain blocks ordered by height 
    main_chain: Arc<RwLock<Vec<BlockHash>>>,
    
    /// Account states in the current best chain
    accounts: Arc<DashMap<Vec<u8>, AccountState>>,
    
    /// Current chain state and statistics
    state: Arc<RwLock<ChainState>>,
    
    /// Security checkpoints for long-range attack prevention
    checkpoints: Arc<RwLock<Vec<SecurityCheckpoint>>>,
    
    /// Orphan blocks waiting for their parents
    orphan_pool: Arc<DashMap<BlockHash, OrphanBlock>>,
    
    /// Orphan blocks by peer (DoS protection)
    orphan_by_peer: Arc<DashMap<String, Vec<BlockHash>>>,
    
    /// Transaction mempool for pending transactions
    mempool: Arc<TransactionMempool>,
    
    /// Block arrival times for difficulty adjustment
    block_times: Arc<RwLock<VecDeque<(u64, DateTime<Utc>)>>>,
    
    /// DoS protection metrics by peer
    peer_metrics: Arc<DashMap<String, PeerMetrics>>,
    
    /// Genesis block hash
    genesis_hash: BlockHash,
    
    /// Miner keypair for block signing
    miner_keypair: Dilithium3Keypair,
    
    /// Configuration parameters
    target_block_time: Duration,
    difficulty_adjustment_interval: u64,
    max_orphan_blocks: usize,
    max_reorg_depth: u64,
    
    /// Block processing rate limiter
    block_processing_times: Arc<RwLock<VecDeque<DateTime<Utc>>>>,
}

impl NumiBlockchain {
    /// Create new blockchain with genesis block and enhanced security
    pub fn new() -> Result<Self> {
        Self::new_with_keypair(None)
    }

    /// Create new blockchain with optional keypair
    pub fn new_with_keypair(keypair: Option<Dilithium3Keypair>) -> Result<Self> {
        Self::new_with_config(None, keypair)
    }
    
    /// Create new blockchain with consensus configuration and optional keypair
    pub fn new_with_config(consensus_config: Option<crate::config::ConsensusConfig>, keypair: Option<Dilithium3Keypair>) -> Result<Self> {
        let miner_keypair = if let Some(kp) = keypair {
            kp
        } else {
            // Try to load from default wallet file first, fall back to new keypair
            let default_wallet_path = "miner-wallet.json";
            match crate::crypto::Dilithium3Keypair::load_from_file(default_wallet_path) {
                Ok(kp) => {
                    log::info!("🔑 Loaded existing miner wallet from {}", default_wallet_path);
                    kp
                }
                Err(_) => {
                    log::info!("🔑 Creating new miner keypair (no existing wallet found at {})", default_wallet_path);
                    crate::crypto::Dilithium3Keypair::new()?
                }
            }
        };
        
        let blockchain = Self {
            blocks: Arc::new(DashMap::new()),
            main_chain: Arc::new(RwLock::new(Vec::new())),
            accounts: Arc::new(DashMap::new()),
            state: Arc::new(RwLock::new(ChainState::default())),
            checkpoints: Arc::new(RwLock::new(Vec::new())),
            orphan_pool: Arc::new(DashMap::new()),
            orphan_by_peer: Arc::new(DashMap::new()),
            mempool: Arc::new(TransactionMempool::new()), // Temporary mempool
            block_times: Arc::new(RwLock::new(VecDeque::new())),
            peer_metrics: Arc::new(DashMap::new()),
            genesis_hash: [0; 32],
            miner_keypair,
            target_block_time: Duration::from_secs(30), // 30 second blocks
            difficulty_adjustment_interval: 144,        // Adjust every 144 blocks (~1 hour)
            max_orphan_blocks: 1000,                   // Maximum orphan blocks to keep
            max_reorg_depth: 144,                      // Maximum reorganization depth
            block_processing_times: Arc::new(RwLock::new(VecDeque::new())),
        };

        let blockchain_arc = Arc::new(RwLock::new(blockchain));
        let mut mempool = if let Some(ref config) = consensus_config {
            TransactionMempool::new_with_config(config.clone())
        } else {
            TransactionMempool::new()
        };
        mempool.set_blockchain_handle(blockchain_arc.clone());
        
        let mut locked_blockchain = blockchain_arc.write();
        locked_blockchain.mempool = Arc::new(mempool);
        
        locked_blockchain.create_genesis_block()?;

        // TODO: Refactor this to avoid cloning the blockchain.
        // This is a temporary solution to break the Arc cycle.
        let final_blockchain = NumiBlockchain {
            blocks: locked_blockchain.blocks.clone(),
            main_chain: locked_blockchain.main_chain.clone(),
            accounts: locked_blockchain.accounts.clone(),
            state: locked_blockchain.state.clone(),
            checkpoints: locked_blockchain.checkpoints.clone(),
            orphan_pool: locked_blockchain.orphan_pool.clone(),
            orphan_by_peer: locked_blockchain.orphan_by_peer.clone(),
            mempool: locked_blockchain.mempool.clone(),
            block_times: locked_blockchain.block_times.clone(),
            peer_metrics: locked_blockchain.peer_metrics.clone(),
            genesis_hash: locked_blockchain.genesis_hash,
            miner_keypair: locked_blockchain.miner_keypair.clone(),
            target_block_time: locked_blockchain.target_block_time,
            difficulty_adjustment_interval: locked_blockchain.difficulty_adjustment_interval,
            max_orphan_blocks: locked_blockchain.max_orphan_blocks,
            max_reorg_depth: locked_blockchain.max_reorg_depth,
            block_processing_times: locked_blockchain.block_processing_times.clone(),
        };
        Ok(final_blockchain)
    }
    
    /// Load blockchain from storage with validation
    pub async fn load_from_storage(storage: &crate::storage::BlockchainStorage) -> Result<Self> {
        Self::load_from_storage_with_config(storage, None).await
    }
    
    /// Load blockchain from storage with consensus configuration
    pub async fn load_from_storage_with_config(storage: &crate::storage::BlockchainStorage, consensus_config: Option<crate::config::ConsensusConfig>) -> Result<Self> {
        // Try to load from default wallet file first, fall back to new keypair
        let default_wallet_path = "miner-wallet.json";
        let miner_keypair = match crate::crypto::Dilithium3Keypair::load_from_file(default_wallet_path) {
            Ok(kp) => {
                log::info!("🔑 Loaded existing miner wallet from {}", default_wallet_path);
                kp
            }
            Err(_) => {
                log::info!("🔑 Creating new miner keypair (no existing wallet found at {})", default_wallet_path);
                crate::crypto::Dilithium3Keypair::new()?
            }
        };
        
        let mempool = Arc::new(if let Some(ref config) = consensus_config {
            TransactionMempool::new_with_config(config.clone())
        } else {
            TransactionMempool::new()
        });
        
        let mut blockchain = Self {
            blocks: Arc::new(DashMap::new()),
            main_chain: Arc::new(RwLock::new(Vec::new())),
            accounts: Arc::new(DashMap::new()),
            state: Arc::new(RwLock::new(ChainState::default())),
            checkpoints: Arc::new(RwLock::new(Vec::new())),
            orphan_pool: Arc::new(DashMap::new()),
            orphan_by_peer: Arc::new(DashMap::new()),
            mempool,
            block_times: Arc::new(RwLock::new(VecDeque::new())),
            peer_metrics: Arc::new(DashMap::new()),
            genesis_hash: [0; 32],
            miner_keypair,
            target_block_time: Duration::from_secs(30), // 30 second blocks
            difficulty_adjustment_interval: 144,        // Adjust every 144 blocks (~1 hour)
            max_orphan_blocks: 1000,                   // Maximum orphan blocks to keep
            max_reorg_depth: 144,                      // Maximum reorganization depth
            block_processing_times: Arc::new(RwLock::new(VecDeque::new())),
        };
        
        // Load all blocks from storage
        let stored_blocks = storage.iter_blocks(None, None)?.collect::<Result<Vec<_>>>()?;
        let mut blocks_by_height: BTreeMap<u64, Vec<Block>> = BTreeMap::new();
        
        for block in stored_blocks {
            blocks_by_height
                .entry(block.header.height)
                .or_insert_with(Vec::new)
                .push(block);
        }
        
        // Rebuild blockchain from blocks in height order
        if blocks_by_height.is_empty() {
            // No stored blocks, create genesis block
            blockchain.create_genesis_block()?;
        } else {
            // Load existing blocks
            for (height, blocks) in blocks_by_height {
                for block in blocks {
                    if height == 0 {
                        blockchain.genesis_hash = block.calculate_hash()?;
                    }
                    blockchain.process_block_internal(block, false).await?;
                }
            }
        }
        
        // Don't load account states from storage - they should be derived from the blockchain
        // Account states will be rebuilt by replaying all transactions
        
        // Load checkpoints
        if let Some(saved_checkpoints) = storage.load_checkpoints()? {
            for checkpoint in saved_checkpoints {
                blockchain.checkpoints.write().push(checkpoint);
            }
        }
        
        // Rebuild account states from the blockchain by replaying all transactions
        blockchain.rebuild_account_states().await?;
        
        // Recalculate total supply from all blocks to ensure accuracy
        let recalculated_total_supply = blockchain.recalculate_total_supply().await?;
        
        // Load chain state but override total_supply with recalculated value
        if let Some(mut saved_state) = storage.load_chain_state()? {
            saved_state.total_supply = recalculated_total_supply;
            *blockchain.state.write() = saved_state;
        } else {
            // If no saved state, update the default state with recalculated total supply
            let mut state = blockchain.state.write();
            state.total_supply = recalculated_total_supply;
        }
        
        // Validate the loaded blockchain
        if !blockchain.validate_chain().await {
            return Err(BlockchainError::InvalidBlock("Loaded blockchain failed validation".to_string()));
        }
        
        log::info!("✅ Blockchain loaded from storage with {} blocks and {} NUMI total supply", 
                  blockchain.get_current_height(), 
                  recalculated_total_supply as f64 / 100.0);
        Ok(blockchain)
    }

    /// Process new block with enhanced DoS protection and validation
    pub async fn add_block(&self, block: Block) -> Result<bool> {
        self.add_block_from_peer(block, None).await
    }
    
    /// Process new block from specific peer with comprehensive protection
    pub async fn add_block_from_peer(&self, block: Block, peer_id: Option<String>) -> Result<bool> {
        let processing_start = std::time::Instant::now();
        let block_hash = block.calculate_hash()?;
        
        // DoS protection: Rate limiting
        if !self.check_processing_rate_limit()? {
            return Err(BlockchainError::InvalidBlock("Block processing rate limit exceeded".to_string()));
        }
        
        // DoS protection: Block size validation
        let block_size = bincode::serialize(&block)
            .map_err(|e| BlockchainError::SerializationError(e.to_string()))?
            .len();
        
        if block_size > MAX_TRANSACTION_SIZE * 1000 { // Reasonable block size limit
            return Err(BlockchainError::InvalidBlock("Block too large".to_string()));
        }
        
        // Check if we already have this block
        if self.blocks.contains_key(&block_hash) {
            return Ok(false); // Block already processed
        }
        
        // Update peer metrics
        if let Some(ref peer) = peer_id {
            self.update_peer_metrics(peer, |metrics| {
                metrics.blocks_received += 1;
                metrics.last_block_time = Utc::now();
            });
        }
        
        // Enhanced block validation with caching
        if let Err(e) = self.validate_block_comprehensive(&block, peer_id.as_ref()).await {
            if let Some(ref peer) = peer_id {
                self.update_peer_metrics(peer, |metrics| {
                    metrics.invalid_blocks += 1;
                });
            }
            log::warn!("❌ Block {} failed validation: {}", hex::encode(&block_hash), e);
            return Err(e);
        }
        
        // Check if parent exists
        let parent_hash = block.header.previous_hash;
        if !self.blocks.contains_key(&parent_hash) && !block.is_genesis() {
            // Parent not found - add to orphan pool with DoS protection
            return self.handle_orphan_block_protected(block, peer_id).await;
        }
        
        // Wrap block processing (including any I/O) in a timeout to bound total time
        let processing_future = async {
            log::info!("🔄 Starting block processing for block {}", hex::encode(&block_hash));
            
            // Process the block and its transactions
            let was_reorganization = self.connect_block_enhanced(block, peer_id.clone()).await?;
            log::info!("✅ Block processing completed, was_reorganization={}", was_reorganization);

            // Update CPU processing time metric
            let cpu_time = processing_start.elapsed().as_millis() as u64;
            if let Some(ref peer) = peer_id {
                self.update_peer_metrics(peer, |metrics| {
                    metrics.processing_time_total += cpu_time;
                });
            }
            log::info!("⏱️ CPU processing time: {}ms", cpu_time);

            // Process any orphan blocks that might now be valid
            log::info!("🔍 Processing orphan blocks...");
            Box::pin(self.process_orphan_blocks_protected()).await?;
            log::info!("✅ Orphan blocks processing completed");

            // Update checkpoints if needed
            log::info!("🔒 Updating checkpoints...");
            self.update_checkpoints_if_needed().await?;
            log::info!("✅ Checkpoints update completed");

            log::info!("🎉 Block processing fully completed");
            Ok::<bool, BlockchainError>(was_reorganization)
        };
        match tokio::time::timeout(
            std::time::Duration::from_millis(MAX_BLOCK_PROCESSING_TIME_MS),
            processing_future
        ).await {
            Ok(inner) => inner,
            Err(_) => Err(BlockchainError::InvalidBlock("Block processing timed out".to_string())),
        }
    }
    
    /// Internal block processing with orphan handling and reorg detection
    async fn process_block_internal(&self, block: Block, validate_pow: bool) -> Result<bool> {
        let block_hash = block.calculate_hash()?;
        
        // Check if we already have this block
        if self.blocks.contains_key(&block_hash) {
            return Ok(false); // Block already processed
        }
        
        // Basic block validation
        if let Err(e) = self.validate_block_basic(&block).await {
            log::warn!("❌ Block {} failed basic validation: {}", hex::encode(&block_hash), e);
            return Err(e);
        }
        
        // Verify proof of work (skip for genesis and loading from storage)
        if validate_pow && !block.is_genesis() {
            if let Err(e) = self.verify_proof_of_work(&block) {
                log::warn!("❌ Block {} failed PoW verification: {}", hex::encode(&block_hash), e);
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
        let block_hash = block.calculate_hash()?;
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
            is_finalized: false, // Not finalized yet
            children: Vec::new(),
            arrival_time: Utc::now(),
            processing_time_ms: 0,
            peer_id: None,
            validation_cache: None, // Could be populated if we cached earlier
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
            log::debug!("📦 Block {} added to side chain", hex::encode(&block_hash));
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
            difficulty_change: 0, // Placeholder, will be calculated
            is_long_range_attack: false,
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
                           hex::encode(&transaction.get_hash_hex()), e)));
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
        
        // Update main chain vector to include this block
        let block_hash = block.calculate_hash()?;
        let mut main_chain = self.main_chain.write();
        
        // Ensure the main chain has enough capacity
        if block.header.height >= main_chain.len() as u64 {
            main_chain.resize(block.header.height as usize + 1, [0u8; 32]);
        }
        
        // Add the block to the main chain at the correct height
        main_chain[block.header.height as usize] = block_hash;
        
        log::debug!("📝 Updated main chain: block {} at height {}", 
                   hex::encode(&block_hash), block.header.height);
        
        Ok(())
    }
    
    /// Handle orphan blocks that arrive before their parents
    async fn handle_orphan_block(&self, block: Block) -> Result<bool> {
        let block_hash = block.calculate_hash()?;
        
        // Check orphan pool size limit
        if self.orphan_pool.len() >= self.max_orphan_blocks {
            // Remove oldest orphan
            if let Some(oldest) = self.find_oldest_orphan() {
                self.orphan_pool.remove(&oldest);
                log::debug!("🗑️ Removed oldest orphan block to make space");
            }
        }
        
        // Add to orphan pool
        let previous_hash = hex::encode(&block.header.previous_hash);
        let size_bytes = block.serialize_header_for_hashing()?.len();
        let orphan = OrphanBlock {
            block,
            arrival_time: Utc::now(),
            processing_attempts: 0,
            peer_id: None,
            size_bytes,
        };
        
        self.orphan_pool.insert(block_hash, orphan);
        log::info!("👻 Block {} added to orphan pool (parent: {})",
                  hex::encode(&block_hash),
                  previous_hash);
        
        Ok(false)
    }
    
    /// Process orphan blocks that might now be valid
    async fn process_orphan_blocks(&self) -> Result<()> {
        let mut processed_any = true;
        let mut iteration_count = 0;
        const MAX_ITERATIONS: usize = 100; // Prevent infinite loops from orphan storms
        
        // Keep processing until no more orphans can be processed
        while processed_any && iteration_count < MAX_ITERATIONS {
            processed_any = false;
            iteration_count += 1;
            let orphan_hashes: Vec<BlockHash> = self.orphan_pool.iter()
                .map(|entry| *entry.key())
                .collect();
            
            for orphan_hash in orphan_hashes {
                if let Some((_, mut orphan)) = self.orphan_pool.remove(&orphan_hash) {
                    let parent_hash = orphan.block.header.previous_hash;
                    
                    // Check if parent now exists
                    if self.blocks.contains_key(&parent_hash) || orphan.block.is_genesis() {
                        log::info!("🎯 Processing orphan block {} (parent now available)",
                                  hex::encode(&orphan_hash));
                        
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
        
        if iteration_count >= MAX_ITERATIONS {
            log::warn!("⚠️ Orphan processing stopped after {} iterations to prevent DoS", MAX_ITERATIONS);
        }
        
        Ok(())
    }
    
    /// Create genesis block
    fn create_genesis_block(&mut self) -> Result<()> {
        log::info!("🔑 Creating genesis block with miner public key: {} (length: {})", 
                  hex::encode(&self.miner_keypair.public_key), 
                  self.miner_keypair.public_key.len());
        
        let genesis_transactions = vec![
            // Genesis block mining reward (same as other blocks)
            Transaction::new(
                self.miner_keypair.public_key.clone(),
                TransactionType::MiningReward {
                    block_height: 0,
                    amount: 1000, // 10 NUMI = 1000 NANO
                    pool_address: None,
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
        self.genesis_hash = genesis_block.calculate_hash()?;
        
        // Process genesis block (add to index, but transaction not applied automatically)
        futures::executor::block_on(self.process_block_internal(genesis_block.clone(), false))?;
        // Manually connect genesis block to main chain to apply genesis reward
        let genesis_block_for_connect = genesis_block.clone();
        futures::executor::block_on(async {
            self.connect_block_to_main_chain(&genesis_block_for_connect).await?;
            self.update_chain_state_after_reorg(self.genesis_hash).await?;
            Ok::<(), BlockchainError>(())
        })?;

        log::info!("🌱 Genesis block created: {}", hex::encode(&self.genesis_hash));
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
            // Validate in current blockchain context (balance, nonce, etc.)
            if let Err(e) = self.validate_transaction_in_context(transaction).await {
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
        
        if !verify_pow(&header_blob?, block.header.nonce, &difficulty_target)? {
            return Err(BlockchainError::InvalidBlock("Proof of work verification failed".to_string()));
        }
        
        Ok(())
    }
    
    /// Validate transaction in current blockchain context
    async fn validate_transaction_in_context(&self, transaction: &Transaction) -> Result<()> {
        // Get current account state
        let account_state = self.accounts.get(&transaction.from)
            .map(|state| state.clone())
            .unwrap_or_else(|| AccountState {
                balance: 0,
                nonce: 0,
                transaction_count: 0,
                total_received: 0,
                total_sent: 0,
                created_at: Utc::now(),
                last_activity: Utc::now(),
            });
        
        // Validate transaction type-specific conditions
        match &transaction.transaction_type {
            TransactionType::Transfer { amount, .. } => {
                if transaction.nonce != account_state.nonce + 1 {
                    return Err(BlockchainError::InvalidTransaction(
                        format!("Invalid nonce: expected {}, got {}", 
                               account_state.nonce + 1, transaction.nonce)));
                }
                if account_state.balance < (*amount + transaction.fee) {
                    return Err(BlockchainError::InvalidTransaction(
                        format!("Insufficient balance: {} < {} (amount + fee)", account_state.balance, *amount + transaction.fee)));
                }
            }

            TransactionType::MiningReward { .. } => {
                // Mining rewards are validated at block level
            }

            TransactionType::ContractDeploy { .. } | TransactionType::ContractCall { .. } => {
                // Contract transactions are not yet implemented
                return Err(BlockchainError::InvalidTransaction("Contract transactions not yet supported".to_string()));
            }
        }
        
        Ok(())
    }
    
    /// Apply transaction to account states
    async fn apply_transaction(&self, transaction: &Transaction) -> Result<()> {
        let sender_key = transaction.from.clone();
        
        // Derive address from public key (first 64 bytes of hash)
        let address = self.derive_address(&sender_key);
        
        log::debug!("🔍 Applying transaction with sender key: {} (length: {}) -> address: {}", 
                   hex::encode(&sender_key[..sender_key.len().min(32)]), sender_key.len(), 
                   hex::encode(&address));
        
        // Get or create sender account using address
        let mut sender_state = self.accounts.get(&address)
            .map(|state| state.clone())
            .unwrap_or_default();
        
        match &transaction.transaction_type {
            TransactionType::Transfer { to, amount, memo: _ } => {
                // Deduct amount and fee from sender
                sender_state.balance -= amount + transaction.fee;
                sender_state.nonce += 1;
                sender_state.transaction_count += 1;
                sender_state.total_sent += amount;
                
                // Derive recipient address
                let recipient_address = self.derive_address(to);
                
                // Add to recipient
                let mut recipient_state = self.accounts.get(&recipient_address)
                    .map(|state| state.clone())
                    .unwrap_or_else(|| AccountState {
                        balance: 0,
                        nonce: 0,
                        transaction_count: 0,
                        total_received: 0,
                        total_sent: 0,
                        created_at: Utc::now(),
                        last_activity: Utc::now(),
                    });
                
                recipient_state.balance += amount;
                recipient_state.total_received += amount;
                
                self.accounts.insert(recipient_address, recipient_state);
            }
            
            TransactionType::MiningReward { amount, .. } => {
                log::info!("💰 Applying mining reward: {} NUMI to {}", 
                          *amount as f64 / 100.0, 
                          hex::encode(&sender_key[..sender_key.len().min(16)]));
                sender_state.balance += amount;
                sender_state.total_received += amount;
                
                // Update total supply
                let mut state = self.state.write();
                state.total_supply += amount;
                log::info!("💰 Updated total supply to: {} NUMI", state.total_supply as f64 / 100.0);
            }
            

            TransactionType::ContractDeploy { .. } | TransactionType::ContractCall { .. } => {
                // Contract operations are not yet implemented
                return Err(BlockchainError::InvalidTransaction("Contract operations not supported".to_string()));
            }
        }
        
        self.accounts.insert(address, sender_state);
        log::info!("✅ Transaction applied successfully");
        Ok(())
    }
    
    /// Undo transaction effects (for chain reorganization)
    async fn undo_transaction(&self, transaction: &Transaction) -> Result<()> {
        let sender_key = transaction.from.clone();
        
        if let Some(mut sender_state) = self.accounts.get(&sender_key).map(|s| s.clone()) {
            match &transaction.transaction_type {
                TransactionType::Transfer { to, amount, memo: _ } => {
                    // Restore sender balance
                    sender_state.balance += amount + transaction.fee;
                    sender_state.nonce -= 1;
                    sender_state.transaction_count -= 1;
                    sender_state.total_sent -= amount;
                    
                    // Deduct from recipient
                    if let Some(mut recipient_state) = self.accounts.get(to).map(|s| s.clone()) {
                        recipient_state.balance -= amount;
                        recipient_state.total_received -= amount;
                        self.accounts.insert(to.clone(), recipient_state);
                    }
                }
                

                
                TransactionType::MiningReward { amount, .. } => {
                    sender_state.balance -= amount;
                    sender_state.total_received -= amount;
                    
                    // Update total supply
                    let mut state = self.state.write();
                    state.total_supply -= amount;
                }
                

                TransactionType::ContractDeploy { .. } | TransactionType::ContractCall { .. } => {
                    // Contract operations are not yet implemented
                    return Err(BlockchainError::InvalidTransaction("Contract operations not supported".to_string()));
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
            
            let old_height = state.total_blocks.saturating_sub(1);
            let new_height = best_block_meta.height;
            
            log::info!("🔄 Updating chain state: height {} -> {}, best_hash: {}", 
                      old_height, new_height, hex::encode(&new_best_hash));
            
            state.best_block_hash = new_best_hash;
            state.total_blocks = best_block_meta.height + 1;
            state.cumulative_difficulty = best_block_meta.cumulative_difficulty;
            state.last_block_time = best_block_meta.block.header.timestamp;
            
            log::info!("✅ Chain state updated: total_blocks={}, difficulty={}, cumulative_difficulty={}", 
                      state.total_blocks, best_block_meta.block.header.difficulty, state.cumulative_difficulty);
            
            log::info!("🔄 update_chain_state_after_reorg returning Ok(())...");
            Ok(())
        } else {
            log::error!("❌ Failed to find block metadata for best hash: {}", hex::encode(&new_best_hash));
            Err(BlockchainError::BlockNotFound("Block metadata not found".to_string()))
        }
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
        // Work = 2^256 / (target + 1), where target is derived from difficulty
        let target = crate::crypto::generate_difficulty_target(difficulty);
        // Convert target (Vec<u8>) to U256 (big-endian)
        let mut target_bytes = [0u8; 32];
        for (i, b) in target.iter().enumerate().take(32) {
            target_bytes[i] = *b;
        }
        let target_value = num_bigint::BigUint::from_bytes_be(&target_bytes);
        let one = num_bigint::BigUint::from(1u8);
        let max = num_bigint::BigUint::from_bytes_be(&[0xFFu8; 32]); // 2^256 - 1
        if target_value.is_zero() {
            return u128::MAX; // Easiest possible work
        }
        let work = (&max / (&target_value + &one)).to_u128().unwrap_or(u128::MAX);
        work
    }
    
    /// Calculate next difficulty adjustment
    fn calculate_next_difficulty(&self, height: u64) -> u32 {
        if height < self.difficulty_adjustment_interval {
            return 8; // Higher initial difficulty for more challenging mining
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
                    log::error!("❌ Block height mismatch at index {}: expected {}, got {}", 
                               i, i, block.header.height);
                    return false;
                }
                
                // Validate previous hash (except genesis)
                if i > 0 {
                    let prev_hash = main_chain[i - 1];
                    if block.header.previous_hash != prev_hash {
                        log::error!("❌ Previous hash mismatch at height {}", block.header.height);
                        return false;
                    }
                }
                
                // Validate block structure
                if let Err(e) = futures::executor::block_on(self.validate_block_basic(block)) {
                    log::error!("❌ Block {} failed validation: {}", block.header.height, e);
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
        
        // Don't save account states - they should be derived from the blockchain
        // Account states will be rebuilt by replaying all transactions when loading
        
        // Save checkpoints
        storage.save_checkpoints(&self.checkpoints.read().clone())?;
        
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
        let total_blocks = self.state.read().total_blocks;
        let height = total_blocks.saturating_sub(1);
        log::debug!("📏 get_current_height: total_blocks={}, height={}", total_blocks, height);
        height
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
    
    /// Derive address from public key (32 bytes of hash)
    fn derive_address(&self, public_key: &[u8]) -> Vec<u8> {
        use blake3::Hasher;
        let mut hasher = Hasher::new();
        hasher.update(public_key);
        let hash = hasher.finalize();
        hash.as_bytes().to_vec()
    }
    
    /// Get account balance
    pub fn get_balance(&self, public_key: &[u8]) -> u64 {
        // Determine if this is a public key or an address
        let address = if public_key.len() == 32 {
            // This is already an address (32 bytes)
            public_key.to_vec()
        } else {
            // This is a public key, derive the address
            self.derive_address(public_key)
        };
        
        let balance = self.accounts.get(&address)
            .map(|state| state.balance)
            .unwrap_or(0);
        
        log::debug!("🔍 Balance lookup for {} -> address {}: {} NUMI", 
                   hex::encode(&public_key[..public_key.len().min(16)]), 
                   hex::encode(&address),
                   balance as f64 / 100.0);
        
        balance
    }
    
    /// Get mining reward for given height
    pub fn get_mining_reward(&self, height: u64) -> u64 {
        // Halving every 210,000 blocks (like Bitcoin)
        let halving_interval = 210_000u64;
        let halvings = height / halving_interval;
        
        if halvings >= 64 {
            return 0; // No more rewards after 64 halvings
        }
        
        // Initial reward: 10 NUMI = 1000 NANO
        let initial_reward = 1000u64; // 10 NUMI in NANO units
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

    /// Enhanced block validation with comprehensive checks and caching
    async fn validate_block_comprehensive(&self, block: &Block, _peer_id: Option<&String>) -> Result<()> {
        let block_hash = block.calculate_hash()?;
        
        // Check validation cache first
        if let Some(metadata) = self.blocks.get(&block_hash) {
            if let Some(ref cache) = metadata.validation_cache {
                if cache.cached_at > Utc::now() - chrono::Duration::minutes(5) { // Cache valid for 5 minutes
                    if cache.structure_valid && cache.signature_valid && cache.pow_valid {
                        return Ok(());
                    } else {
                        return Err(BlockchainError::InvalidBlock("Cached validation failed".to_string()));
                    }
                }
            }
        }
        
        // Enhanced basic validation
        self.validate_block_structure_enhanced(block)?;
        
        // Verify block signature
        if !block.verify_signature()? {
            return Err(BlockchainError::InvalidBlock("Block signature verification failed".to_string()));
        }
        
        // Verify proof of work for non-genesis blocks
        if !block.is_genesis() {
            if let Err(e) = self.verify_proof_of_work_enhanced(block) {
                log::warn!("❌ Block {} failed PoW verification: {}", hex::encode(&block_hash), e);
                return Err(e);
            }
        }
        
        // Validate against parent if available
        if let Some(parent_meta) = self.blocks.get(&block.header.previous_hash) {
            self.validate_block_against_parent(block, &parent_meta.block)?;
        }
        
        // Cache validation results
        let _cache = ValidationCache {
            signature_valid: true,
            pow_valid: true,
            structure_valid: true,
            cached_at: Utc::now(),
        };
        
        // Store cache would require block to be in the index already
        // This is a design consideration for performance vs complexity
        
        Ok(())
    }
    
    /// Enhanced block structure validation with DoS protection
    fn validate_block_structure_enhanced(&self, block: &Block) -> Result<()> {
        // Basic structure checks
        if block.transactions.is_empty() && !block.is_genesis() {
            return Err(BlockchainError::InvalidBlock("Block has no transactions".to_string()));
        }
        
        // Transaction count limit (DoS protection)
        if block.transactions.len() > 10000 {
            return Err(BlockchainError::InvalidBlock("Too many transactions in block".to_string()));
        }
        
        // Validate timestamp with stricter bounds
        let now = Utc::now();
        let max_future_time = now + chrono::Duration::seconds(900); // 15 minutes tolerance
        let min_past_time = now - chrono::Duration::days(1); // 1 day tolerance
        
        if block.header.timestamp > max_future_time {
            return Err(BlockchainError::InvalidBlock("Block timestamp too far in future".to_string()));
        }
        
        if block.header.timestamp < min_past_time {
            return Err(BlockchainError::InvalidBlock("Block timestamp too far in past".to_string()));
        }
        
        // Enhanced transaction validation
        let mut total_fees = 0u64;
        let mut has_mining_reward = false;
        
        for (i, transaction) in block.transactions.iter().enumerate() {
            // Validate transaction structure
            if let Err(e) = transaction.validate_structure() {
                return Err(BlockchainError::InvalidBlock(
                    format!("Transaction {} invalid: {}", i, e)));
            }
            
            // Check for duplicate transactions in block
            for (j, other_tx) in block.transactions.iter().enumerate() {
                if i != j && transaction.id == other_tx.id {
                    return Err(BlockchainError::InvalidBlock("Duplicate transaction in block".to_string()));
                }
            }
            
            // Track fees and rewards
            if transaction.is_reward() {
                if has_mining_reward {
                    return Err(BlockchainError::InvalidBlock("Multiple mining rewards in block".to_string()));
                }
                has_mining_reward = true;
                
                // Validate mining reward amount
                let expected_reward = self.get_mining_reward(block.header.height);
                let total_fees = block.get_total_fees();
                let max_reward = expected_reward.saturating_add(total_fees);
                if transaction.get_amount() > max_reward {
                    return Err(BlockchainError::InvalidBlock("Invalid mining reward amount".to_string()));
                }
            } else {
                total_fees = total_fees.saturating_add(transaction.fee);
            }
        }
        
        // Genesis block special validation
        if block.is_genesis() {
            if block.header.height != 0 {
                return Err(BlockchainError::InvalidBlock("Genesis block must have height 0".to_string()));
            }
            if block.header.previous_hash != [0u8; 32] {
                return Err(BlockchainError::InvalidBlock("Genesis block must have zero previous hash".to_string()));
            }
        }
        
        Ok(())
    }
    
    /// Enhanced proof of work verification with attack detection
    fn verify_proof_of_work_enhanced(&self, block: &Block) -> Result<()> {
        let difficulty_target = generate_difficulty_target(block.header.difficulty);
        let header_blob = block.serialize_header_for_hashing();
        
        // Verify the PoW meets the difficulty target
        if !verify_pow(&header_blob?, block.header.nonce, &difficulty_target)? {
            return Err(BlockchainError::InvalidBlock("Proof of work verification failed".to_string()));
        }
        
        // Additional checks for difficulty consistency
        let _current_state = self.state.read();
        let expected_difficulty = self.calculate_next_difficulty(block.header.height);
        
        // Allow some tolerance for difficulty transitions
        if block.header.difficulty > expected_difficulty + 5 || 
           (expected_difficulty > 5 && block.header.difficulty < expected_difficulty - 5) {
            return Err(BlockchainError::InvalidBlock(
                format!("Invalid difficulty: expected ~{}, got {}", expected_difficulty, block.header.difficulty)));
        }
        
        Ok(())
    }
    
    /// Validate block against its parent
    fn validate_block_against_parent(&self, block: &Block, parent: &Block) -> Result<()> {
        // Height validation
        if block.header.height != parent.header.height + 1 {
            return Err(BlockchainError::InvalidBlock("Invalid block height sequence".to_string()));
        }
        
        // Timestamp validation (must be after parent)
        if block.header.timestamp <= parent.header.timestamp {
            return Err(BlockchainError::InvalidBlock("Block timestamp must be after parent".to_string()));
        }
        
        // Previous hash validation
        if block.header.previous_hash != parent.calculate_hash()? {
            return Err(BlockchainError::InvalidBlock("Previous block hash mismatch".to_string()));
        }
        
        Ok(())
    }
    
    /// Enhanced orphan block handling with DoS protection
    async fn handle_orphan_block_protected(&self, block: Block, peer_id: Option<String>) -> Result<bool> {
        let block_hash = block.calculate_hash()?;
        
        // Check global orphan pool size limit
        if self.orphan_pool.len() >= self.max_orphan_blocks {
            // Remove oldest orphan
            if let Some(oldest) = self.find_oldest_orphan() {
                if let Some((_, old_orphan)) = self.orphan_pool.remove(&oldest) {
                    // Also remove from peer tracking
                    if let Some(ref old_peer) = old_orphan.peer_id {
                        if let Some(mut peer_orphans) = self.orphan_by_peer.get_mut(old_peer) {
                            peer_orphans.retain(|&h| h != oldest);
                        }
                    }
                }
            }
        }
        
        // Check per-peer orphan limit
        if let Some(ref peer) = peer_id {
            let peer_orphan_count = self.orphan_by_peer.get(peer)
                .map(|orphans| orphans.len())
                .unwrap_or(0);
                
            if peer_orphan_count >= MAX_ORPHAN_BLOCKS_PER_PEER {
                // Remove oldest orphan from this peer
                if let Some(mut peer_orphans) = self.orphan_by_peer.get_mut(peer) {
                    if let Some(oldest_hash) = peer_orphans.first().copied() {
                        peer_orphans.remove(0);
                        self.orphan_pool.remove(&oldest_hash);
                    }
                }
            }
            
            // Update peer metrics
            self.update_peer_metrics(peer, |metrics| {
                metrics.orphan_blocks += 1;
            });
        }
        
        // Calculate block size for DoS protection
        let block_size = bincode::serialize(&block)
            .map_err(|e| BlockchainError::SerializationError(e.to_string()))?
            .len();
        
        // Add to orphan pool
        let previous_hash = hex::encode(&block.header.previous_hash);
        let orphan = OrphanBlock {
            block,
            arrival_time: Utc::now(),
            processing_attempts: 0,
            peer_id: peer_id.clone(),
            size_bytes: block_size,
        };
        
        self.orphan_pool.insert(block_hash, orphan);
        
        // Track by peer
        if let Some(ref peer) = peer_id {
            self.orphan_by_peer
                .entry(peer.clone())
                .or_insert_with(Vec::new)
                .push(block_hash);
        }
        
        log::info!("👻 Block {} added to orphan pool (parent: {}, peer: {:?})",
                  hex::encode(&block_hash),
                  previous_hash,
                  peer_id);
        
        Ok(false)
    }
    
    /// Enhanced block connection with security checks
    async fn connect_block_enhanced(&self, block: Block, peer_id: Option<String>) -> Result<bool> {
        let processing_start = std::time::Instant::now();
        let block_hash = block.calculate_hash()?;
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
        
        // Create enhanced block metadata
        let processing_time = processing_start.elapsed().as_millis() as u64;
        let metadata = BlockMetadata {
            block: block.clone(),
            cumulative_difficulty,
            height: block.header.height,
            is_main_chain: false, // Will be updated if this becomes main chain
            is_finalized: false,  // Not finalized yet
            children: Vec::new(),
            arrival_time: Utc::now(),
            processing_time_ms: processing_time,
            peer_id,
            validation_cache: None, // Could be populated if we cached earlier
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
            // This is the new best chain - perform reorganization with security checks
            log::info!("🔄 New best chain found, performing reorganization");
            let result = self.reorganize_to_block_secure(block_hash).await;
            log::info!("✅ Reorganization completed with result: {:?}", result);
            return result;
        } else {
            log::debug!("📦 Block {} added to side chain", hex::encode(&block_hash));
            return Ok(false);
        }
    }
    
    /// DoS protection: Check block processing rate limit
    fn check_processing_rate_limit(&self) -> Result<bool> {
        let now = Utc::now();
        let one_second_ago = now - chrono::Duration::seconds(1);
        
        let mut processing_times = self.block_processing_times.write();
        
        // Remove old timestamps
        while let Some(&front_time) = processing_times.front() {
            if front_time <= one_second_ago {
                processing_times.pop_front();
            } else {
                break;
            }
        }
        
        // Check if we're under the rate limit
        if processing_times.len() >= MAX_BLOCKS_PER_SECOND {
            return Ok(false);
        }
        
        // Add current timestamp
        processing_times.push_back(now);
        Ok(true)
    }
    
    /// Update peer metrics safely
    fn update_peer_metrics<F>(&self, peer_id: &str, update_fn: F)
    where
        F: FnOnce(&mut PeerMetrics),
    {
        let mut entry = self.peer_metrics.entry(peer_id.to_string()).or_insert_with(|| {
            PeerMetrics {
                blocks_received: 0,
                invalid_blocks: 0,
                last_block_time: Utc::now(),
                orphan_blocks: 0,
                processing_time_total: 0,
                rate_limit_violations: 0,
            }
        });
        
        update_fn(&mut entry);
    }
    
    /// Update security checkpoints when needed
    async fn update_checkpoints_if_needed(&self) -> Result<()> {
        let current_height = self.get_current_height();
        
        if current_height > 0 && current_height % CHECKPOINT_INTERVAL == 0 {
            let state = self.state.read();
            let state_root = self.calculate_state_root();
            
            let checkpoint = SecurityCheckpoint::new(
                current_height,
                state.best_block_hash,
                state.cumulative_difficulty,
                state.total_supply,
                state_root,
            );
            
            let mut checkpoints = self.checkpoints.write();
            checkpoints.push(checkpoint);
            
            // Keep only recent checkpoints
            if checkpoints.len() > MAX_CHECKPOINTS {
                let len = checkpoints.len();
                checkpoints.drain(0..len - MAX_CHECKPOINTS);
            }
            
            log::info!("📍 Security checkpoint created at height {}", current_height);
        }
        
        Ok(())
    }
    
    /// Calculate state root from all account states
    fn calculate_state_root(&self) -> [u8; 32] {
        let mut account_hashes = Vec::new();
        
        for entry in self.accounts.iter() {
            let account_data = bincode::serialize(&(entry.key(), entry.value())).unwrap_or_default();
            account_hashes.push(blake3_hash(&account_data));
        }
        
        // Sort hashes for deterministic root
        account_hashes.sort_unstable();
        
        // Calculate Merkle root of account hashes
        if account_hashes.is_empty() {
            return [0u8; 32];
        }
        
        while account_hashes.len() > 1 {
            let mut new_hashes = Vec::new();
            
            for chunk in account_hashes.chunks(2) {
                let mut combined = Vec::new();
                combined.extend_from_slice(&chunk[0]);
                if chunk.len() > 1 {
                    combined.extend_from_slice(&chunk[1]);
                } else {
                    combined.extend_from_slice(&chunk[0]); // Duplicate for odd number
                }
                new_hashes.push(blake3_hash(&combined));
            }
            
            account_hashes = new_hashes;
        }
        
        account_hashes[0]
    }

    /// Secure chain reorganization with long-range attack detection
    async fn reorganize_to_block_secure(&self, new_best_hash: BlockHash) -> Result<bool> {
        let current_best_hash = self.state.read().best_block_hash;
        
        log::info!("🔄 Starting secure reorganization: current_best={}, new_best={}", 
                  hex::encode(&current_best_hash), hex::encode(&new_best_hash));
        
        // Find the fork point between current and new chain
        let mut fork_info = self.find_fork_point(current_best_hash, new_best_hash)?;
        
        log::info!("📍 Fork info: old_chain={}, new_chain={}, blocks_to_disconnect={}, blocks_to_connect={}", 
                  fork_info.old_chain.len(), fork_info.new_chain.len(), 
                  fork_info.blocks_to_disconnect.len(), fork_info.blocks_to_connect.len());
        
        // Enhanced security: Detect long-range attacks
        fork_info.is_long_range_attack = self.detect_long_range_attack(&fork_info);
        
        if fork_info.is_long_range_attack {
            log::warn!("🚨 Potential long-range attack detected, checking against checkpoints");
            if !self.validate_against_checkpoints(&fork_info)? {
                return Err(BlockchainError::ConsensusError("Long-range attack rejected by checkpoints".to_string()));
            }
        }
        
        // Check if reorganization depth is acceptable
        if fork_info.old_chain.len() > self.max_reorg_depth as usize {
            log::warn!("🚫 Reorganization depth {} exceeds maximum {}, rejecting", 
                      fork_info.old_chain.len(), self.max_reorg_depth);
            return Ok(false);
        }
        
        // Calculate difficulty change
        let old_difficulty = fork_info.blocks_to_disconnect.iter()
            .map(|b| self.calculate_block_work(b.header.difficulty) as i128)
            .sum::<i128>();
        let new_difficulty = fork_info.blocks_to_connect.iter()
            .map(|b| self.calculate_block_work(b.header.difficulty) as i128)
            .sum::<i128>();
        fork_info.difficulty_change = new_difficulty - old_difficulty;
        
        log::info!("🔄 Reorganizing chain: disconnecting {} blocks, connecting {} blocks (difficulty change: {})",
                  fork_info.blocks_to_disconnect.len(), fork_info.blocks_to_connect.len(), fork_info.difficulty_change);
        
        // Disconnect old chain blocks (reverse order)
        log::info!("🔗 Disconnecting old chain blocks...");
        for block in fork_info.blocks_to_disconnect.iter().rev() {
            self.disconnect_block(block).await?;
        }
        
        // Connect new chain blocks (forward order)
        log::info!("🔗 Connecting new chain blocks...");
        for block in &fork_info.blocks_to_connect {
            self.connect_block_to_main_chain(block).await?;
        }
        
        // Update main chain
        log::info!("📝 Updating main chain vector...");
        let new_chain = self.build_chain_to_block(new_best_hash)?;
        *self.main_chain.write() = new_chain;
        
        // Mark blocks as main chain and update finalization
        log::info!("🏷️ Updating main chain flags...");
        self.update_main_chain_flags(new_best_hash).await;
        log::info!("🔒 Updating finalization status...");
        self.update_finalization_status().await;
        
        // Update chain state
        log::info!("📊 Updating chain state...");
        self.update_chain_state_after_reorg(new_best_hash).await?;

        log::info!("✅ Secure chain reorganization completed successfully");
        log::info!("🔄 Reorganization returning Ok(true)...");
        Ok(true)
    }
    
    /// Detect potential long-range attacks
    fn detect_long_range_attack(&self, fork_info: &ForkInfo) -> bool {
        // Check if the fork goes back too far
        let fork_depth = fork_info.old_chain.len();
        
        // If fork is deeper than finality depth, it's a potential long-range attack
        if fork_depth > FINALITY_DEPTH as usize {
            return true;
        }
        
        // Check if fork has suspiciously low difficulty for its length
        let avg_old_difficulty = if !fork_info.blocks_to_disconnect.is_empty() {
            fork_info.blocks_to_disconnect.iter()
                .map(|b| b.header.difficulty as u64)
                .sum::<u64>() / fork_info.blocks_to_disconnect.len() as u64
        } else {
            0
        };
        
        let avg_new_difficulty = if !fork_info.blocks_to_connect.is_empty() {
            fork_info.blocks_to_connect.iter()
                .map(|b| b.header.difficulty as u64)
                .sum::<u64>() / fork_info.blocks_to_connect.len() as u64
        } else {
            0
        };
        
        // If new chain has significantly lower difficulty and is long, suspect attack
        if fork_depth > 100 && avg_new_difficulty < avg_old_difficulty / 2 {
            return true;
        }
        
        false
    }
    
    /// Validate fork against security checkpoints
    fn validate_against_checkpoints(&self, fork_info: &ForkInfo) -> Result<bool> {
        let checkpoints = self.checkpoints.read();
        
        // Find the most recent checkpoint that affects this fork
        for checkpoint in checkpoints.iter().rev() {
            // Check if fork affects this checkpoint
            for &block_hash in &fork_info.old_chain {
                if let Some(block_meta) = self.blocks.get(&block_hash) {
                    if block_meta.height <= checkpoint.block_height {
                        // Fork affects this checkpoint, validate against it
                        if block_hash != checkpoint.block_hash {
                            log::warn!("🚨 Fork conflicts with checkpoint at height {}", checkpoint.block_height);
                            return Ok(false);
                        }
                    }
                }
            }
        }
        
        Ok(true)
    }
    
    /// Update block finalization status
    async fn update_finalization_status(&self) {
        log::info!("🔒 Starting finalization status update...");
        let current_height = self.get_current_height();
        let finality_height = current_height.saturating_sub(FINALITY_DEPTH);
        
        log::info!("📏 Finalization: current_height={}, finality_height={}", current_height, finality_height);
        
        // Mark blocks as finalized if they're deep enough
        let mut finalized_count = 0;
        for mut entry in self.blocks.iter_mut() {
            let meta = entry.value_mut();
            if meta.height <= finality_height && meta.is_main_chain {
                meta.is_finalized = true;
                finalized_count += 1;
            }
        }
        
        log::info!("✅ Finalized {} blocks", finalized_count);
        
        // Update chain state with finalization info
        if let Some(finalized_hash) = self.main_chain.read().get(finality_height as usize).copied() {
            let mut state = self.state.write();
            state.finalized_block_hash = finalized_hash;
            state.finalized_block_height = finality_height;
            log::info!("📊 Updated chain state finalization: finalized_block_hash={}", hex::encode(&finalized_hash));
        }
        
        log::info!("✅ Finalization status update completed");
    }
    
    /// Protected orphan block processing with DoS prevention
    async fn process_orphan_blocks_protected(&self) -> Result<()> {
        let mut processed_any = true;
        let mut processing_attempts = 0;
        
        // Keep processing until no more orphans can be processed
        while processed_any && processing_attempts < MAX_PROCESSING_ATTEMPTS {
            processed_any = false;
            processing_attempts += 1;
            
            let orphan_hashes: Vec<BlockHash> = self.orphan_pool.iter()
                .map(|entry| *entry.key())
                .collect();
            
            for orphan_hash in orphan_hashes {
                if let Some((_, mut orphan)) = self.orphan_pool.remove(&orphan_hash) {
                    let parent_hash = orphan.block.header.previous_hash;
                    
                    // Check if parent now exists
                    if self.blocks.contains_key(&parent_hash) || orphan.block.is_genesis() {
                        log::info!("🎯 Processing orphan block {} (parent now available)",
                                  hex::encode(&orphan_hash));
                        
                        // Remove from peer tracking
                        if let Some(ref peer_id) = orphan.peer_id {
                            if let Some(mut peer_orphans) = self.orphan_by_peer.get_mut(peer_id) {
                                peer_orphans.retain(|&h| h != orphan_hash);
                            }
                        }
                        
                        let block = orphan.block.clone();
                        let peer_id = orphan.peer_id.clone();
                        match self.add_block_from_peer(block, peer_id.clone()).await {
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
                                    // Also update peer tracking
                                    if let Some(ref peer_id) = peer_id {
                                        self.orphan_by_peer
                                            .entry(peer_id.clone())
                                            .or_default()
                                            .push(orphan_hash);
                                    }
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
        
        if processing_attempts >= MAX_PROCESSING_ATTEMPTS {
            log::warn!("⚠️ Orphan processing hit maximum attempts limit");
        }
        
        Ok(())
    }
    
    /// Get peer statistics for monitoring
    pub fn get_peer_metrics(&self, peer_id: &str) -> Option<PeerMetrics> {
        self.peer_metrics.get(peer_id).map(|entry| entry.clone())
    }
    
    /// Get all peer statistics
    pub fn get_all_peer_metrics(&self) -> HashMap<String, PeerMetrics> {
        self.peer_metrics.iter()
            .map(|entry| (entry.key().clone(), entry.value().clone()))
            .collect()
    }
    
    /// Get security checkpoints
    pub fn get_checkpoints(&self) -> Vec<SecurityCheckpoint> {
        self.checkpoints.read().clone()
    }
    
    /// Get latest security checkpoint
    pub fn get_latest_checkpoint(&self) -> Option<SecurityCheckpoint> {
        self.checkpoints.read().last().cloned()
    }
    
    /// Validate checkpoint and potentially add it
    pub fn add_checkpoint(&self, checkpoint: SecurityCheckpoint) -> Result<()> {
        let _current_state = self.state.read();
        checkpoint.validate(_current_state.total_blocks, _current_state.cumulative_difficulty)?;
        
        let mut checkpoints = self.checkpoints.write();
        
        // Check if we already have a checkpoint at this height
        if let Some(existing) = checkpoints.iter().find(|cp| cp.block_height == checkpoint.block_height) {
            if existing.block_hash != checkpoint.block_hash {
                return Err(BlockchainError::ConsensusError("Conflicting checkpoint".to_string()));
            }
            return Ok(()); // Already have this checkpoint
        }
        
        // Insert checkpoint in correct position (sorted by height)
        let insert_pos = checkpoints.iter()
            .position(|cp| cp.block_height > checkpoint.block_height)
            .unwrap_or(checkpoints.len());
        
        checkpoints.insert(insert_pos, checkpoint);
        
        // Keep only recent checkpoints
        if checkpoints.len() > MAX_CHECKPOINTS {
            let len = checkpoints.len();
            checkpoints.drain(0..len - MAX_CHECKPOINTS);
        }
        
        Ok(())
    }
    
    /// Get account state or default
    pub fn get_account_state_or_default(&self, public_key: &[u8]) -> AccountState {
        self.accounts.get(public_key)
            .map(|state| state.clone())
            .unwrap_or_default()
    }
    
    /// Update account activity timestamp
    pub fn update_account_activity(&self, public_key: &[u8]) {
        if let Some(mut account) = self.accounts.get_mut(public_key) {
            account.last_activity = Utc::now();
        }
    }
    

    
    /// Get network statistics
    pub fn get_network_stats(&self) -> HashMap<String, u64> {
        let mut stats = HashMap::new();
        let state = self.state.read();
        
        stats.insert("total_blocks".to_string(), state.total_blocks);
        stats.insert("total_supply".to_string(), state.total_supply);

        stats.insert("network_hash_rate".to_string(), state.network_hash_rate);
        stats.insert("orphan_blocks".to_string(), self.orphan_pool.len() as u64);
        stats.insert("peer_count".to_string(), self.peer_metrics.len() as u64);
        
        stats
    }
    

    

    

    
    /// Clean up old peer metrics
    #[allow(dead_code)]
    async fn cleanup_old_peer_metrics(&self) {
        let cutoff_time = Utc::now() - chrono::Duration::hours(24);
        
        let old_peers: Vec<String> = self.peer_metrics.iter()
            .filter(|entry| entry.last_block_time < cutoff_time)
            .map(|entry| entry.key().clone())
            .collect();
        
        for peer_id in &old_peers {
            self.peer_metrics.remove(peer_id);
            self.orphan_by_peer.remove(peer_id);
        }
        
        if !old_peers.is_empty() {
            log::info!("🧹 Cleaned up {} old peer metrics", old_peers.len());
        }
    }
    
    /// Update network hash rate estimate
    #[allow(dead_code)]
    async fn update_network_hash_rate(&self) {
        let current_difficulty = self.state.read().current_difficulty;
        let target_time = self.target_block_time.as_secs();
        
        // Estimate hash rate based on current difficulty and target time
        // Hash rate = difficulty * 2^difficulty / target_time (simplified)
        let estimated_hash_rate = (current_difficulty as u64).saturating_mul(1 << current_difficulty.min(20)) / target_time;
        
        let mut state = self.state.write();
        state.network_hash_rate = estimated_hash_rate;
    }
    
    /// Check if block is finalized (beyond reorganization)
    pub fn is_block_finalized(&self, block_hash: &BlockHash) -> bool {
        self.blocks.get(block_hash)
            .map(|meta| meta.is_finalized)
            .unwrap_or(false)
    }
    
    /// Get finalized blocks up to a certain height
    pub fn get_finalized_blocks(&self, up_to_height: u64) -> Vec<Block> {
        self.blocks.iter()
            .filter(|entry| {
                let meta = entry.value();
                meta.is_finalized && meta.height <= up_to_height
            })
            .map(|entry| entry.value().block.clone())
            .collect()
    }

    /// Get a clone of the transaction mempool handle for use without holding locks
    pub fn mempool_handle(&self) -> Arc<TransactionMempool> {
        Arc::clone(&self.mempool)
    }

    /// Recalculate total supply from all blocks in the main chain
    async fn recalculate_total_supply(&self) -> Result<u64> {
        let mut total_supply = 0u64;
        let main_chain = self.main_chain.read();
        
        log::info!("🔍 Recalculating total supply from {} blocks in main chain", main_chain.len());
        
        for (i, &block_hash) in main_chain.iter().enumerate() {
            if let Some(block_meta) = self.blocks.get(&block_hash) {
                log::info!("🔍 Block {} (height {}): {} transactions", i, block_meta.height, block_meta.block.transactions.len());
                for (j, transaction) in block_meta.block.transactions.iter().enumerate() {
                    if let TransactionType::MiningReward { amount, .. } = &transaction.transaction_type {
                        log::info!("💰 Found mining reward in block {} transaction {}: {} NUMI", i, j, *amount as f64 / 100.0);
                        total_supply += amount;
                    }
                }
            } else {
                log::warn!("⚠️ Block hash {} not found in blocks map", hex::encode(&block_hash));
            }
        }
        
        log::info!("💰 Recalculated total supply: {} NUMI", total_supply as f64 / 100.0);
        Ok(total_supply)
    }

    /// Public function to manually recalculate and update total supply (for debugging)
    pub async fn recalculate_and_update_total_supply(&self) -> Result<u64> {
        let recalculated_supply = self.recalculate_total_supply().await?;
        let mut state = self.state.write();
        state.total_supply = recalculated_supply;
        log::info!("✅ Updated total supply to: {} NUMI", recalculated_supply as f64 / 100.0);
        Ok(recalculated_supply)
    }
    
    /// Rebuild account states by replaying all transactions from the blockchain
    pub async fn rebuild_account_states(&self) -> Result<()> {
        log::info!("🔄 Rebuilding account states from blockchain...");
        
        // Clear existing account states
        self.accounts.clear();
        
        // Get all blocks in height order
        let main_chain = self.main_chain.read();
        let mut total_supply = 0u64;
        
        for (height, block_hash) in main_chain.iter().enumerate() {
            if let Some(block_meta) = self.blocks.get(block_hash) {
                let block = &block_meta.block;
                log::debug!("🔍 Processing block {} (height {}) with {} transactions", 
                           hex::encode(&block_hash[..8]), height, block.transactions.len());
                
                // Process all transactions in this block
                for (tx_index, transaction) in block.transactions.iter().enumerate() {
                    log::debug!("  Processing transaction {} in block {}", tx_index, height);
                    
                    // Apply transaction to rebuild account state
                    self.apply_transaction(transaction).await?;
                    
                    // Track total supply for mining rewards
                    if let TransactionType::MiningReward { amount, .. } = &transaction.transaction_type {
                        total_supply += amount;
                        log::debug!("💰 Mining reward in block {}: {} NUMI (total: {} NUMI)", 
                                   height, *amount as f64 / 100.0, total_supply as f64 / 100.0);
                    }
                }
            }
        }
        
        log::info!("✅ Account states rebuilt from {} blocks", main_chain.len());
        log::info!("💰 Total supply from blockchain: {} NUMI", total_supply as f64 / 100.0);
        log::info!("📊 Total accounts in memory: {}", self.accounts.len());
        
        // Debug: List all accounts
        for entry in self.accounts.iter() {
            let (pubkey, account) = entry.pair();
            log::info!("  Account {} ({}...): {} NUMI", 
                      hex::encode(&pubkey), 
                      hex::encode(&pubkey[..pubkey.len().min(8)]),
                      account.balance as f64 / 100.0);
        }
        
        Ok(())
    }

    /// Get all accounts with their balances (for CLI display)
    pub fn get_all_accounts(&self) -> Vec<(Vec<u8>, AccountState)> {
        self.accounts.iter()
            .map(|item| (item.key().clone(), item.value().clone()))
            .collect()
    }
} 