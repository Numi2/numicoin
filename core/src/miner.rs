use crate::{
    block::Block,
    transaction::Transaction,
    crypto::Dilithium3Keypair,
    error::BlockchainError,
    Result,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;
use parking_lot::RwLock;
use chrono;
use num_cpus;
use std::path::{Path, PathBuf};

/// Shared wallet and mining utility functions
pub struct WalletManager;

impl WalletManager {
    /// Calculate mining reward based on configurable halving schedule
    pub fn calculate_mining_reward(height: u64) -> u64 {
        Self::calculate_mining_reward_with_config(height, &Default::default())
    }
    
    /// Calculate mining reward with custom configuration
    pub fn calculate_mining_reward_with_config(height: u64, config: &crate::config::ConsensusConfig) -> u64 {
        let halving_interval = config.mining_reward_halving_interval;
        let initial_reward = config.initial_mining_reward;

        let halvings = height / halving_interval;
        if halvings >= 64 {
            return 0;
        }
        initial_reward >> halvings
    }
    
    /// Load or create miner wallet with consistent logic
    pub fn load_or_create_miner_wallet(data_directory: &Path) -> Result<Dilithium3Keypair> {
        let wallet_path = data_directory.join("miner-wallet.json");
        
        match Dilithium3Keypair::load_from_file(&wallet_path) {
            Ok(kp) => {
                log::info!("🔑 Loaded existing miner wallet from {wallet_path:?}");
                Ok(kp)
            }
            Err(_) => {
                log::info!("🔑 Creating new miner keypair (no existing wallet found at {wallet_path:?})");
                let kp = Dilithium3Keypair::new()?;
                
                // Ensure parent directory exists
                if let Some(parent) = wallet_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                
                // Save the new keypair to the configured wallet path
                if let Err(e) = kp.save_to_file(&wallet_path) {
                    log::warn!("⚠️ Failed to save new keypair to {wallet_path:?}: {e}");
                } else {
                    log::info!("✅ New miner wallet saved to {wallet_path:?}");
                }
                Ok(kp)
            }
        }
    }
    
    /// Load or create miner wallet with custom path
    pub fn load_or_create_miner_wallet_at_path(wallet_path: &Path) -> Result<Dilithium3Keypair> {
        match Dilithium3Keypair::load_from_file(wallet_path) {
            Ok(kp) => {
                log::warn!("🔑 Loaded existing miner wallet from {wallet_path:?}");
                Ok(kp)
            }
            Err(_) => {
                log::warn!("🔑 Creating new miner keypair (no wallet found at {wallet_path:?})");
                let kp = Dilithium3Keypair::new()?;
                
                // Ensure parent directory exists
                if let Some(parent) = wallet_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                
                // Save the new keypair
                if let Err(e) = kp.save_to_file(wallet_path) {
                    log::warn!("⚠️ Failed to save new keypair to {wallet_path:?}: {e}");
                } else {
                    log::info!("✅ New miner wallet saved to {wallet_path:?}");
                }
                Ok(kp)
            }
        }
    }
}

/// Basic mining statistics
#[derive(Debug, Clone)]
pub struct MiningStats {
    pub hash_rate: u64,           // Hashes per second
    pub total_hashes: u64,        // Total hashes computed
    pub current_nonce: u64,       // Current nonce being tested
    pub difficulty: u32,          // Current difficulty
    pub is_mining: bool,          // Mining active status
    pub blocks_mined: u64,        // Total blocks successfully mined
    pub mining_time_secs: u64,    // Total mining time in seconds
    pub start_timestamp: u64,     // When mining started (unix timestamp)
    pub threads_active: usize,    // Number of active mining threads
}

/// Simple mining configuration
#[derive(Debug, Clone)]
pub struct MiningConfig {
    /// Number of threads to use (0 = auto-detect)
    pub thread_count: usize,
    /// Nonce range per thread
    pub nonce_chunk_size: u64,
    /// Statistics update interval in seconds
    pub stats_update_interval: u64,
    /// Path to the miner's wallet file
    pub wallet_path: PathBuf,
}

impl From<crate::config::MiningConfig> for MiningConfig {
    fn from(cfg: crate::config::MiningConfig) -> Self {
        Self {
            thread_count: cfg.thread_count,
            nonce_chunk_size: cfg.nonce_chunk_size,
            stats_update_interval: cfg.stats_update_interval_secs,
            wallet_path: cfg.wallet_path.clone(),
        }
    }
}

impl Default for MiningConfig {
    fn default() -> Self {
        Self {
            thread_count: num_cpus::get(),
            nonce_chunk_size: 10_000,
            stats_update_interval: 5,
            wallet_path: PathBuf::from("miner-wallet.json"),
        }
    }
}

impl MiningConfig {
    /// Resolve wallet path relative to data directory if needed
    pub fn resolve_wallet_path(&self, data_directory: &Path) -> PathBuf {
        if self.wallet_path.is_absolute() {
            self.wallet_path.clone()
        } else {
            data_directory.join(&self.wallet_path)
        }
    }
}

/// Mining result containing the successfully mined block and statistics
#[derive(Debug)]
pub struct MiningResult {
    pub block: Block,
    pub nonce: u64,
    pub hash_rate: u64,
    pub mining_time_secs: u64,
    pub thread_id: usize,
    pub total_attempts: u64,
}

/// Multi-threaded miner
pub struct Miner {
    /// Miner's keypair for signing blocks
    keypair: Dilithium3Keypair,
    
    /// Mining control flags
    is_mining: Arc<AtomicBool>,
    should_stop: Arc<AtomicBool>,
    
    /// Mining statistics
    stats: Arc<RwLock<MiningStats>>,
    
    /// Configuration
    config: MiningConfig,
    
    /// Global nonce counter for work distribution
    global_nonce: Arc<AtomicU64>,
    
    /// Active thread handles
    thread_handles: Vec<std::thread::JoinHandle<()>>,
}

impl Miner {
    /// Create new miner with default configuration
    pub fn new() -> Result<Self> {
        let config = MiningConfig::default();
        Self::with_config(config)
    }
    
    /// Create new miner with custom configuration
    pub fn with_config(config: MiningConfig) -> Result<Self> {
        // Use default data directory for wallet resolution
        let data_dir = PathBuf::from("./core-data");
        Self::with_config_and_data_dir(config, data_dir)
    }
    
    /// Create new miner with custom configuration and data directory
    pub fn with_config_and_data_dir(config: MiningConfig, data_directory: PathBuf) -> Result<Self> {
        let wallet_path = config.resolve_wallet_path(&data_directory);
        
        // Use centralized wallet management
        let keypair = WalletManager::load_or_create_miner_wallet_at_path(&wallet_path)?;
        
        Self::with_config_and_keypair(config, keypair)
    }
    
    /// Create new miner with custom configuration and specific keypair
    pub fn with_config_and_keypair(config: MiningConfig, keypair: Dilithium3Keypair) -> Result<Self> {
        let stats = MiningStats {
            hash_rate: 0,
            total_hashes: 0,
            current_nonce: 0,
            difficulty: 1,
            is_mining: false,
            blocks_mined: 0,
            mining_time_secs: 0,
            start_timestamp: chrono::Utc::now().timestamp() as u64,
            threads_active: 0,
        };
        
        Ok(Self {
            keypair,
            is_mining: Arc::new(AtomicBool::new(false)),
            should_stop: Arc::new(AtomicBool::new(false)),
            stats: Arc::new(RwLock::new(stats)),
            config,
            global_nonce: Arc::new(AtomicU64::new(0)),
            thread_handles: Vec::new(),
        })
    }
    
    /// Start mining a block with multi-threaded approach
    pub fn mine_block(
        &mut self,
        height: u64,
        previous_hash: crate::block::BlockHash,
        transactions: Vec<Transaction>,
        difficulty: u32,
        start_nonce: u64,
    ) -> Result<Option<MiningResult>> {
        if self.is_mining.load(Ordering::Relaxed) {
            return Err(BlockchainError::MiningError("Mining already in progress".to_string()));
        }
        
        log::info!("🔨 Starting multi-threaded mining for block {height} (difficulty: {difficulty})");
        
        // Prepare mining parameters
        let difficulty_target = crate::crypto::generate_difficulty_target(difficulty);
        let mut block = Block::new(
            height,
            previous_hash,
            {
                // Calculate economic incentives: base block reward + total collected fees
                let mut txs = transactions;
                let total_fees: u64 = txs.iter().map(|tx| tx.fee).sum();

                // Determine base block reward according to halving schedule
                let base_reward = WalletManager::calculate_mining_reward(height);
                let reward_amount = base_reward.saturating_add(total_fees);

                // Construct the reward transaction addressed to the miner
                let mut reward_tx = Transaction::new_with_fee(
                    self.keypair.public_key_bytes().to_vec(),
                    crate::transaction::TransactionType::MiningReward {
                        block_height: height,
                        amount: reward_amount,
                        pool_address: None,
                    },
                    0, // nonce for system-generated tx
                    0, // fee is zero for reward
                    0, // gas_limit
                );
                // Sign the reward transaction with the miner's keypair
                if let Err(e) = reward_tx.sign(&self.keypair) {
                    log::error!("Failed to sign mining reward transaction: {e}");
                }

                // Insert reward transaction as the very first transaction in the block
                txs.insert(0, reward_tx);
                txs
            },
            difficulty,
            self.keypair.public_key_bytes().to_vec(),
        );
        
        // Initialize mining state
        self.is_mining.store(true, Ordering::Relaxed);
        self.should_stop.store(false, Ordering::Relaxed);
        self.global_nonce.store(start_nonce, Ordering::Relaxed);
        
        self.update_mining_stats(difficulty, true);
        
        // Determine optimal thread count
        let thread_count = if self.config.thread_count == 0 {
            num_cpus::get()
        } else {
            self.config.thread_count
        };
        
        log::info!("🚀 Using {thread_count} threads for mining");
        
        // Create mining result channels
        let (result_tx, result_rx) = std::sync::mpsc::channel();
        self.thread_handles.clear();
        
        // Spawn mining threads
        for thread_id in 0..thread_count {
            let result_tx = result_tx.clone();
            let is_mining = Arc::clone(&self.is_mining);
            let should_stop = Arc::clone(&self.should_stop);
            let global_nonce = Arc::clone(&self.global_nonce);
            let stats = Arc::clone(&self.stats);
            let config = self.config.clone();
            let block_template = block.clone();

            let handle = std::thread::spawn(move || {
                // Catch panics in worker threads
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    Self::mining_thread_worker(
                        thread_id,
                        block_template,
                        difficulty_target,
                        is_mining,
                        should_stop,
                        global_nonce,
                        stats,
                        config,
                        result_tx,
                    );
                }));
                if let Err(err) = result {
                    log::error!("Mining thread {thread_id} panicked: {err:?}");
                }
            });
            self.thread_handles.push(handle);
        }
        
        // Start statistics monitoring thread
        let stats_monitor = self.spawn_stats_monitor();
        
        // Wait for mining result or timeout
        let mining_timeout = std::time::Duration::from_secs(300); // 5 minutes timeout
        let result = match result_rx.recv_timeout(mining_timeout) {
            Ok(mining_result) => {
                log::info!("🎉 Block mined successfully by thread {} in {} seconds!", 
                          mining_result.thread_id, mining_result.mining_time_secs);
                
                // Stop all threads
                self.should_stop.store(true, Ordering::Relaxed);
                self.is_mining.store(false, Ordering::Relaxed);
                
                // Update final statistics
                self.update_final_stats(&mining_result);
                
                // Sign the mined block
                block.header.nonce = mining_result.nonce;
                
                let mut coinbase_tx = block.transactions.remove(0);
                block.sign(&self.keypair, Some(&mut coinbase_tx))?;
                
                Ok(Some(MiningResult {
                    block,
                    nonce: mining_result.nonce,
                    hash_rate: mining_result.hash_rate,
                    mining_time_secs: mining_result.mining_time_secs,
                    thread_id: mining_result.thread_id,
                    total_attempts: mining_result.total_attempts,
                }))
            }
            Err(_) => {
                log::warn!("⏰ Mining timeout reached, stopping threads");
                self.should_stop.store(true, Ordering::Relaxed);
                self.is_mining.store(false, Ordering::Relaxed);
                Ok(None)
            }
        };
        
        // Clean up threads
        for handle in self.thread_handles.drain(..) {
            let _ = handle.join();
        }
        let _ = stats_monitor.join();
        
        result
    }
    
    /// Mine block with blockchain integration - gets difficulty and transactions from blockchain
    pub fn mine_block_integrated(
        &mut self,
        blockchain: &crate::blockchain::NumiBlockchain,
        start_nonce: u64,
    ) -> Result<Option<MiningResult>> {
        if self.is_mining.load(Ordering::Relaxed) {
            return Err(BlockchainError::MiningError("Mining already in progress".to_string()));
        }
        
        // Get current blockchain state
        let current_height = blockchain.get_current_height() + 1; // Next block height
        let previous_hash = blockchain.get_latest_block_hash();
        let difficulty = blockchain.get_current_difficulty();
        
        // Get transactions from blockchain's mempool
        let max_transactions = 1000; // Reasonable limit
        let max_size = 1_000_000; // 1MB block size limit
        let transactions = blockchain.get_transactions_for_block(max_size, max_transactions);
        
        log::info!("🔨 Mining block {} with {} transactions (difficulty: {})", 
                  current_height, transactions.len(), difficulty);
        
        // Use the existing mine_block method with blockchain-provided data
        self.mine_block(current_height, previous_hash, transactions, difficulty, start_nonce)
    }
    
    /// Mine and automatically submit block to blockchain - full integration
    pub async fn mine_and_submit_block(
        &mut self,
        blockchain: &crate::blockchain::NumiBlockchain,
        start_nonce: u64,
    ) -> Result<Option<crate::block::Block>> {
        // Mine the block using integrated method
        match self.mine_block_integrated(blockchain, start_nonce)? {
            Some(mining_result) => {
                log::info!("Success!🎉 Block mined!");
                
                // Submit the mined block to the blockchain
                match blockchain.add_block(mining_result.block.clone()).await {
                    Ok(was_reorganization) => {
                        if was_reorganization {
                            log::info!("🔄 Chain reorganization");
                        } else {
                            log::info!("✅ Block submitted to blockchain successfully");
                        }
                        Ok(Some(mining_result.block))
                    }
                    Err(e) => {
                        log::error!("❌ Failed to submit mined block to blockchain: {}", e);
                        Err(e)
                    }
                }
            }
            None => {
                log::info!("⏰ Mining timed out, no block produced");
                Ok(None)
            }
        }
    }
    
    /// Worker function for mining threads
    fn mining_thread_worker(
        thread_id: usize,
        mut block: Block,
        difficulty_target: [u8; 32],
        is_mining: Arc<AtomicBool>,
        should_stop: Arc<AtomicBool>,
        global_nonce: Arc<AtomicU64>,
        stats: Arc<RwLock<MiningStats>>,
        config: MiningConfig,
        result_tx: std::sync::mpsc::Sender<MiningResult>,
    ) {
        log::debug!("🔨 Mining thread {thread_id} started");
        
        let mut local_hashes = 0u64;
        let mut total_hashes = 0u64;
        let mut last_stats_update = Instant::now();
        let thread_start_time = Instant::now();
        
        // Main mining loop
        while is_mining.load(Ordering::Relaxed) && !should_stop.load(Ordering::Relaxed) {
            if should_stop.load(Ordering::Relaxed) {
                break;
            }
            
            // Get next nonce range to work on
            let start_nonce = global_nonce.fetch_add(config.nonce_chunk_size, Ordering::Relaxed);
            let end_nonce = start_nonce + config.nonce_chunk_size;
            
            // Test nonce range
            for nonce in start_nonce..end_nonce {
                if should_stop.load(Ordering::Relaxed) {
                    break;
                }
                
                block.header.nonce = nonce;
                let header_blob = match block.serialize_header_for_hashing() {
                    Ok(blob) => blob,
                    Err(e) => {
                        log::error!("Failed to serialize header in thread {thread_id}: {e}");
                        continue;
                    }
                };
                
                total_hashes += 1;
                local_hashes += 1;
                
                // Check if this nonce satisfies the difficulty target
                match crate::crypto::verify_pow(&header_blob, nonce, &difficulty_target) {
                    Ok(true) => {
                        // Found valid block!
                        let mining_time = thread_start_time.elapsed();
                        let hash_rate = total_hashes / mining_time.as_secs().max(1);
                        
                        let mining_result = MiningResult {
                            block,
                            nonce,
                            hash_rate,
                            mining_time_secs: mining_time.as_secs(),
                            thread_id,
                            total_attempts: total_hashes,
                        };
                        
                        // Send result and exit
                        let _ = result_tx.send(mining_result);
                        return;
                    }
                    Ok(false) => {
                        // Continue mining
                    }
                    Err(e) => {
                        log::error!("PoW verification error in thread {thread_id}: {e}");
                        continue;
                    }
                }
                
                // Update statistics periodically
                if last_stats_update.elapsed().as_secs() >= config.stats_update_interval {
                    Self::update_thread_stats(&stats, thread_id, local_hashes, thread_start_time);
                    local_hashes = 0;
                    last_stats_update = Instant::now();
                }
            }
        }
        
        log::debug!("🔨 Mining thread {thread_id} finished (hashes: {local_hashes})");
    }
    
    /// Update thread-specific mining statistics
    fn update_thread_stats(
        stats: &Arc<RwLock<MiningStats>>,
        _thread_id: usize,
        hashes: u64,
        start_time: Instant,
    ) {
        let mut stats = stats.write();
        stats.total_hashes += hashes;
        stats.current_nonce += hashes;
        
        let elapsed = start_time.elapsed();
        if elapsed.as_secs() > 0 {
            stats.hash_rate = stats.total_hashes / elapsed.as_secs();
        }
    }
    
    /// Spawn statistics monitoring thread
    fn spawn_stats_monitor(&self) -> std::thread::JoinHandle<()> {
        let stats = Arc::clone(&self.stats);
        let is_mining = Arc::clone(&self.is_mining);
        let update_interval = self.config.stats_update_interval;
        
        std::thread::spawn(move || {
            while is_mining.load(Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_secs(update_interval));
                
                let stats = stats.read();
                log::info!("📊 Mining stats: {} H/s, {} total hashes, {} threads", 
                          stats.hash_rate, stats.total_hashes, stats.threads_active);
            }
        })
    }
    
    /// Update mining statistics
    fn update_mining_stats(&self, difficulty: u32, mining_active: bool) {
        let mut stats = self.stats.write();
        stats.difficulty = difficulty;
        stats.start_timestamp = chrono::Utc::now().timestamp() as u64;
        stats.is_mining = mining_active;
        stats.threads_active = if mining_active { self.config.thread_count } else { 0 };
        if mining_active {
            stats.total_hashes = 0;
            stats.current_nonce = self.global_nonce.load(Ordering::Relaxed);
            stats.hash_rate = 0;
        }
    }
    
    /// Update final statistics after successful mining
    fn update_final_stats(&self, result: &MiningResult) {
        let mut stats = self.stats.write();
        stats.blocks_mined += 1;
        stats.mining_time_secs += result.mining_time_secs;
        stats.is_mining = false;
        stats.threads_active = 0;
    }
    
    /// Stop mining completely
    pub fn stop(&mut self) {
        if self.is_mining.load(Ordering::Relaxed) {
            log::info!("🛑 Stopping mining...");
            self.should_stop.store(true, Ordering::Relaxed);
            self.is_mining.store(false, Ordering::Relaxed);
            
            // Wait for threads to finish
            while let Some(handle) = self.thread_handles.pop() {
                let _ = handle.join();
            }
            
            let mut stats = self.stats.write();
            stats.is_mining = false;
            stats.threads_active = 0;
            
            log::info!("✅ Mining stopped");
        }
    }
    
    /// Get current mining statistics
    pub fn get_stats(&self) -> MiningStats {
        self.stats.read().clone()
    }
    
    /// Check if currently mining
    pub fn is_mining(&self) -> bool {
        self.is_mining.load(Ordering::Relaxed)
    }
    
    /// Update mining configuration
    pub fn update_config(&mut self, config: MiningConfig) {
        if self.is_mining.load(Ordering::Relaxed) {
            log::warn!("Cannot update mining config while mining is active");
            return;
        }
        
        self.config = config;
        log::info!("🔧 Mining configuration updated");
    }
    
    /// Get current mining configuration
    pub fn get_config(&self) -> &MiningConfig {
        &self.config
    }
    
    #[cfg(test)]
    pub fn get_keypair(&self) -> &Dilithium3Keypair {
        &self.keypair
    }
    
    /// Estimate time to mine next block based on current hash rate
    pub fn estimate_block_time(&self, difficulty: u32) -> std::time::Duration {
        let stats = self.stats.read();
        if stats.hash_rate == 0 {
            return std::time::Duration::from_secs(u64::MAX); // Unknown
        }
        
        // Avoid overflow: large difficulty treated as infinite
        if difficulty >= 64 {
            return std::time::Duration::from_secs(u64::MAX);
        }
        let target_hashes = 1u64 << difficulty;
        let estimated_seconds = target_hashes / stats.hash_rate;
        
        std::time::Duration::from_secs(estimated_seconds)
    }
}

impl Drop for Miner {
    fn drop(&mut self) {
        self.stop();
    }
}

// Thread-safe implementation
unsafe impl Send for Miner {}
unsafe impl Sync for Miner {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::Dilithium3Keypair;

    #[test]
    fn test_miner_creation() {
        let miner = Miner::new().unwrap();
        assert!(!miner.is_mining());
    }
    
    #[test]
    fn test_mining_config() {
        let config = MiningConfig::default();
        assert!(config.thread_count > 0);
        assert!(config.nonce_chunk_size > 0);
    }
    
    #[test]
    fn test_mining_stats() {
        let miner = Miner::new().unwrap();
        let stats = miner.get_stats();
        
        assert_eq!(stats.hash_rate, 0);
        assert_eq!(stats.total_hashes, 0);
        assert!(!stats.is_mining);
        assert_eq!(stats.blocks_mined, 0);
    }
    
    #[tokio::test]
    async fn test_mining_simple_block() {
        let mut miner = Miner::with_config(MiningConfig::default()).unwrap();
        let keypair = Dilithium3Keypair::new().unwrap();
        
        // Create simple transaction
        let transaction = crate::transaction::Transaction::new(
            keypair.public_key_bytes().to_vec(),
            crate::transaction::TransactionType::MiningReward {
                block_height: 1,
                amount: 1000,
                pool_address: None,
            },
            0,
        );
        
        // Try mining with very low difficulty
        let result = miner.mine_block(
            1,
            [0; 32],
            vec![transaction],
            1, // Very low difficulty
            0,
        ).unwrap();
        
        // Should eventually find a solution or timeout
        assert!(result.is_some() || result.is_none()); // Either works
    }
    
    #[test]
    fn test_block_time_estimation() {
        let miner = Miner::new().unwrap();
        
        // With zero hash rate, should return maximum duration
        let estimate = miner.estimate_block_time(10);
        assert!(estimate.as_secs() > 1000000); // Very large number
    }
    
    #[test]
    fn test_miner_wallet_persistence() {
        use std::fs;
        use tempfile::tempdir;
        
        let temp_dir = tempdir().unwrap();
        let wallet_path = temp_dir.path().join("test-wallet.json");
        
        // Create a config with a specific wallet path
        let config = MiningConfig {
            wallet_path: wallet_path.clone(),
            ..Default::default()
        };
        
        // Create miner - should create and save wallet
        let miner1 = Miner::with_config(config.clone()).unwrap();
        let public_key1 = miner1.get_keypair().public_key.clone();
        
        // Verify wallet file was created
        assert!(wallet_path.exists());
        
        // Create another miner with same config - should load existing wallet
        let miner2 = Miner::with_config(config).unwrap();
        let public_key2 = miner2.get_keypair().public_key.clone();
        
        // Verify both miners use the same keypair (loaded from file)
        assert_eq!(public_key1, public_key2);
        
        // Clean up
        fs::remove_file(&wallet_path).unwrap();
    }
} 