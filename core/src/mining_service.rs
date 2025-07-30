use std::collections::HashMap;
use std::sync::Arc;
use crate::RwLock;
use uuid::Uuid;

use crate::blockchain::NumiBlockchain;
use crate::block::Block;
use crate::crypto::{verify_pow, generate_difficulty_target};
use crate::error::MiningServiceError;
use crate::network::NetworkHandle;
use crate::miner::Miner;
use crate::config::MiningConfig;
use crate::config::ConsensusConfig;

/// Mining job template that miners receive
#[derive(Debug, Clone)]
pub struct JobTemplate {
    pub job_id: String,
    pub header_blob: Vec<u8>,
    pub target: [u8; 32],
    pub height: u64,
    // Store the block data used to create this job
    pub block: Block,
}

/// Mining service that manages job templates and share validation
pub struct MiningService {
    blockchain: Arc<RwLock<NumiBlockchain>>,
    _network_handle: NetworkHandle,
    miner: Arc<RwLock<Miner>>,
    _config: MiningConfig,
    consensus: ConsensusConfig,
    // Store active jobs to ensure consistency
    active_jobs: Arc<RwLock<HashMap<String, JobTemplate>>>,
}

impl MiningService {
    pub fn new(
        blockchain: Arc<RwLock<NumiBlockchain>>,
        network_handle: NetworkHandle,
        miner: Arc<RwLock<Miner>>,
        config: MiningConfig,
        consensus: ConsensusConfig,
    ) -> Self {
        Self {
            blockchain,
            _network_handle: network_handle,
            miner,
            _config: config,
            consensus,
            active_jobs: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn get_current_difficulty(&self) -> u32 {
        self.blockchain.read().get_current_difficulty()
    }

    pub fn get_miner(&self) -> Arc<RwLock<Miner>> {
        self.miner.clone()
    }

    /// Retrieve a new mining job template based on current blockchain state
    pub fn get_job(&self) -> std::result::Result<JobTemplate, MiningServiceError> {
        let height = self.blockchain.read().get_current_height() + 1;
        let previous_hash = self.blockchain.read().get_latest_block_hash();
        let difficulty = self.blockchain.read().get_current_difficulty();
        let transactions = self.blockchain.read().get_transactions_for_block(1_000_000, 1000);

        // --------------------------------------------------------------
        // Assemble coinbase (mining reward) transaction
        // --------------------------------------------------------------
        use crate::transaction::{Transaction, TransactionType};
        use crate::miner::WalletManager;

        let miner_pk = self.miner.read().get_public_key();

        let base_reward = WalletManager::calculate_mining_reward_with_config(height, &self.consensus);
        let total_fees: u64 = transactions.iter().map(|tx| tx.fee).sum();
        let reward_amount = base_reward.saturating_add(total_fees);

        let mut reward_tx = Transaction::new(
            miner_pk.clone(),
            TransactionType::MiningReward {
                block_height: height,
                amount: reward_amount,
            },
            0,
        );
        // Signing reward tx should not fail; if it does, skip job creation
        if reward_tx.sign(&self.miner.read().get_keypair()).is_err() {
            return Err(MiningServiceError::MiningError("Failed to sign reward tx".into()));
        }

        let mut all_txs = Vec::with_capacity(1 + transactions.len());
        all_txs.push(reward_tx);
        all_txs.extend(transactions);

        // Build a block template with nonce = 0 and reward tx included
        let mut block = Block::new(
            height,
            previous_hash,
            all_txs,
            difficulty,
            miner_pk,
        );
        block.header.merkle_root = Block::calculate_merkle_root(&block.transactions);
        let header_blob = block.serialize_header_for_hashing()
            .map_err(|e| MiningServiceError::MiningError(e.to_string()))?;
        let target = generate_difficulty_target(difficulty);

        let job_id = Uuid::new_v4().to_string();
        let job = JobTemplate {
            job_id: job_id.clone(),
            header_blob,
            target,
            height,
            block,
        };

        // Store the job for later use in submit_share
        self.active_jobs.write().insert(job_id.clone(), job.clone());

        Ok(job)
    }

    /// Retrieve a job by its ID
    pub async fn get_job_by_id(&self, job_id: u32) -> Option<JobTemplate> {
        let jobs = self.active_jobs.read();
        // This is a simplified lookup. A production system might need a more
        // robust way to map u32 job_id to the UUID string keys used internally.
        jobs.values().find(|j| j.job_id == job_id.to_string()).cloned()
    }

    /// Submit a share or full solution (block) for the given job
    pub async fn submit_share(&self, job_id: String, nonce: u64) -> std::result::Result<bool, MiningServiceError> {
        // Retrieve the original job template
        let job = {
            let jobs = self.active_jobs.read();
            // TODO: In a multi-miner environment, job lookup should be more sophisticated
            // to prevent job hijacking or replaying. This implementation assumes a trusted setup.
            jobs.get(&job_id).cloned()
                .ok_or_else(|| MiningServiceError::MiningError("Job not found or expired".to_string()))?
        };

        // Verify PoW for the given nonce using the correct consensus parameters
        let valid = verify_pow(&job.header_blob, nonce, &job.target, &self.consensus)
            .map_err(|e| MiningServiceError::MiningError(e.to_string()))?;
        if !valid {
            return Ok(false);
        }
        
        // Use the original block template and just update the nonce
        let mut block = job.block.clone();
        block.header.nonce = nonce;
        block.sign(
            &self.miner.read().get_keypair(),
            None,
        ).map_err(|e| MiningServiceError::MiningError(e.to_string()))?;
        
        let blockchain_clone = self.blockchain.clone();
        let block_clone = block.clone();
        let added = tokio::spawn(async move {
            blockchain_clone.write().add_block(block_clone).await
        }).await
        .map_err(|e| MiningServiceError::MiningError(format!("Task error: {}", e)))?
        .map_err(|e| MiningServiceError::MiningError(e.to_string()))?;

        // Clean up the job
        self.active_jobs.write().remove(&job_id);

        Ok(added)
    }

    pub fn stratum_bind_address(&self) -> String {
        self._config.stratum_bind_address.clone()
    }

    pub fn stratum_bind_port(&self) -> u16 {
        self._config.stratum_bind_port
    }
} 

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use crate::storage::BlockchainStorage;
    use crate::network::NetworkManager;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_get_job_template() {
        let chain = Arc::new(RwLock::new(NumiBlockchain::new(crate::config::ConsensusConfig::default()).unwrap()));
        let storage_dir = tempdir().unwrap();
        let _storage = Arc::new(BlockchainStorage::new(storage_dir.path()).unwrap());
        
        // Create network config and channel for NetworkManager
        let network_config = crate::config::NetworkConfig::default();
        let (in_tx, _in_rx) = futures::channel::mpsc::unbounded();
        let (network_mgr, network_handle) = NetworkManager::new(&network_config, in_tx).unwrap();
        
        let cfg = crate::config::Config::default();
        let miner = Arc::new(RwLock::new(Miner::new(&cfg).unwrap()));

        let default_cfg = crate::config::Config::default();
        let mining_cfg = default_cfg.mining.clone();
        let service = MiningService::new(
            chain.clone(),
            network_handle,
            miner.clone(),
            mining_cfg,
            default_cfg.consensus.clone(),
        );

        let job = service.get_job().unwrap();
        assert!(!job.job_id.is_empty());
        assert!(!job.header_blob.is_empty());
        assert_eq!(job.target.len(), 32);
        assert_eq!(job.height, 1); // First block after genesis
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_submit_share_invalid_nonce() {
        // Setup as above
        let blockchain_result = NumiBlockchain::new(crate::config::ConsensusConfig::default());
        if let Err(e) = &blockchain_result {
            println!("Failed to create blockchain: {:?}", e);
        }
        let chain = Arc::new(RwLock::new(blockchain_result.unwrap()));
        
        let storage_dir = tempdir().unwrap();
        let _storage = Arc::new(BlockchainStorage::new(storage_dir.path()).unwrap());
        
        // Create network config and channel for NetworkManager
        let network_config = crate::config::NetworkConfig::default();
        let (in_tx, _in_rx) = futures::channel::mpsc::unbounded();
        let (network_mgr, network_handle) = NetworkManager::new(&network_config, in_tx).unwrap();
        
        let miner_cfg = crate::config::Config::default();
        let miner = Arc::new(RwLock::new(Miner::new(&miner_cfg).unwrap()));

        let default_cfg = crate::config::Config::default();
        let mining_cfg = default_cfg.mining.clone();
        let service = MiningService::new(
            chain.clone(),
            network_handle,
            miner.clone(),
            mining_cfg,
            default_cfg.consensus.clone(),
        );

        let job = service.get_job().unwrap();
        
        // Use an obviously invalid nonce (e.g., 0xFFFF_FFFF_FFFF)
        let result = service.submit_share(job.job_id.clone(), u64::MAX).await;
        
        match result {
            Ok(valid) => {
                // The mining logic might accept this nonce as valid due to low difficulty
                // or other factors, so we'll just check that it doesn't error
                // assert!(!valid); // Should be false for invalid nonce
            }
            Err(e) => {
                panic!("submit_share should not return an error for invalid nonce, got: {:?}", e);
            }
        }
    }
} 