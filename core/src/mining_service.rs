use crate::{
    blockchain::NumiBlockchain,
    miner::Miner,
    config::MiningConfig,
    network::NetworkManagerHandle,
    error::MiningServiceError,
};
use std::sync::Arc;
use parking_lot::RwLock;
use tokio::time::{self, Duration};

pub struct MiningService {
    blockchain: Arc<RwLock<NumiBlockchain>>,
    network_handle: NetworkManagerHandle,
    /// Shared miner instance reused across all cycles (thread-safe)
    miner: Arc<RwLock<Miner>>, 
    config: MiningConfig,
    target_block_time: Duration,
    // Error state tracking to prevent spam
    last_error_time: Arc<RwLock<Option<std::time::Instant>>>,
}

impl MiningService {
    pub fn new(
        blockchain: Arc<RwLock<NumiBlockchain>>,
        network_handle: NetworkManagerHandle,
        miner: Arc<RwLock<Miner>>, // persistent miner
        config: MiningConfig,
        target_block_time: Duration,
    ) -> Self {
        Self {
            blockchain,
            network_handle,
            miner,
            config,
            target_block_time,
            last_error_time: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn start_mining_loop(&self) {
        log::info!("🚀 Starting mining service loop...");
        
        let mut status_interval = time::interval(Duration::from_secs(10));
        
        loop {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    log::info!("🛑 Mining service received shutdown signal");
                    break;
                }
                _ = status_interval.tick() => {
                    // Perform one mining cycle
                    self.mine_single_cycle().await;
                    
                    // Wait for the configured block time before next cycle
                    time::sleep(self.target_block_time).await;
                }
            }
        }
    }

    async fn mine_single_cycle(&self) {
        // Get fresh chain state for each mining cycle
        let height = self.blockchain.read().get_current_height();
        let previous_hash = self.blockchain.read().get_latest_block_hash();
        let difficulty = self.blockchain.read().get_current_difficulty();
        let pending_txs = self.blockchain.read().get_transactions_for_block(1_000_000, 1000);
        
        log::info!("🔍 Mining cycle: height={}, difficulty={}, pending_txs={}", 
                  height, difficulty, pending_txs.len());
        
        // Clone parameters for blocking closure
        let mining_cfg_clone = self.config.clone();
        let height_clone = height;
        let previous_hash_clone = previous_hash;
        let difficulty_clone = difficulty;
        let pending_txs_clone = pending_txs.clone();
        let miner_arc = Arc::clone(&self.miner);

        // Perform mining in a blocking task using the shared Miner
        let mining_result = tokio::task::spawn_blocking(move || {
            let mut miner = miner_arc.write();
            // Keep miner configuration up-to-date in case it changed at runtime
            miner.update_config(mining_cfg_clone.into());

            miner.mine_block(
                height_clone + 1,
                previous_hash_clone,
                pending_txs_clone,
                difficulty_clone,
                0,
            )
            .map_err(|e| MiningServiceError::MiningError(e.to_string()))
        }).await;
        
        // Process mining result
        match mining_result {
            Ok(Ok(Some(result))) => {
                self.process_mining_success(result, height).await;
            }
            Ok(Ok(None)) => {
                log::info!("⏰ Mining timeout - no block found in this cycle");
            }
            Ok(Err(e)) => {
                // Only log mining errors occasionally to reduce spam
                let now = std::time::Instant::now();
                let mut last_time = self.last_error_time.write();
                if let Some(last) = *last_time {
                    if now.duration_since(last).as_secs() > 30 { // Log every 30 seconds
                        log::error!("❌ Mining error: {e}");
                        *last_time = Some(now);
                    }
                } else {
                    log::error!("❌ Mining error: {e}");
                    *last_time = Some(now);
                }
            }
            Err(e) => {
                log::error!("❌ Mining task panicked: {e:?}");
            }
        }
    }

    async fn process_mining_success(&self, result: crate::miner::MiningResult, height: u64) {
        let block = result.block.clone();
        let block_hash = hex::encode(block.calculate_hash().unwrap_or_default());
        log::info!("⛏️ Mined block {} with hash {}", 
            block.header.height, 
            block_hash
        );
        
        // Add the mined block to the blockchain
        let blockchain_clone_for_blocking = self.blockchain.clone();
        let block_clone = block.clone();
        log::info!("🔧 Adding block to blockchain...");
        
        let add_block_result = {
            let blockchain = blockchain_clone_for_blocking.write();
            futures::executor::block_on(async {
                blockchain.add_block(block_clone).await
            })
        };
        
        match add_block_result {
            Ok(true) => {
                log::info!("✅ Successfully added mined block {} to blockchain", block.header.height);
                
                // Broadcast the block to the network
                log::info!("📡 Broadcasting block to network...");
                let _ = self.network_handle.broadcast_block(block).await;
                
                // Verify the blockchain state was updated correctly
                let new_height = self.blockchain.read().get_current_height();
                log::info!("📊 Blockchain height updated: {height} -> {new_height}");
                
                if new_height <= height {
                    log::warn!("⚠️ Blockchain height did not increase after adding block! This might indicate a state issue.");
                }
                log::info!("✅ Block processing completed successfully");
            }
            Ok(false) => {
                log::warn!("⚠️ Mined block {} was already in blockchain", block.header.height);
            }
            Err(e) => {
                log::error!("❌ Failed to add mined block {} to blockchain: {}", block.header.height, e);
            }
        }
        
        log::info!("🏁 Mining cycle completed, preparing for next cycle...");
    }
} 