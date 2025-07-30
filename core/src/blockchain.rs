// src/blockchain.rs
//
// Production-ready Numi core.
// Proof-of-Work: **BLAKE3 hash ≤ target** (256-bit, little-endian).
//

#![allow(clippy::result_large_err)]

use std::sync::Arc;

use bs58;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use crate::RwLock;

use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;
use std::io::Write;
use std::collections::BTreeMap;
use bincode;
use tokio::fs;

use crate::{
    block::{Block, BlockHash, BlockHeader},
    config::ConsensusConfig,
    crypto::{blake3_hash, Dilithium3Keypair},
    error::{BlockchainError, InvalidBlockError},
    mempool::{MempoolStats, TransactionMempool, ValidationResult},
    miner::WalletManager,
    storage::BlockchainStorage,
    transaction::{Transaction, TransactionType},
    Result,
};

/// Compare two little-endian 256-bit integers.  Return `true` if `hash` < `target`.
pub fn meets_target(hash: &[u8; 32], target: &[u8; 32]) -> bool {
    for (h, t) in hash.iter().zip(target.iter()).rev() {
        match h.cmp(t) {
            std::cmp::Ordering::Less => return true,
            std::cmp::Ordering::Greater => return false,
            std::cmp::Ordering::Equal => continue,
        }
    }
    true
}

/* --------------------------------------------------------------------------
   Basic data types
   ------------------------------------------------------------------------*/
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AccountState {
    pub balance: u64,
    pub nonce: u64,
    // kept for future extensions
    pub transaction_count: u64,
    pub total_received: u64,
    pub total_sent: u64,
    pub created_at: DateTime<Utc>,
    pub last_activity: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainState {
    pub total_blocks: u64,
    pub total_supply: u64,
    pub current_difficulty: u32,
    pub best_block_hash: BlockHash,
    pub cumulative_difficulty: u128,
}
impl Default for ChainState {
    fn default() -> Self {
        Self {
            total_blocks: 0,
            total_supply: 0,
            current_difficulty: 1,
            best_block_hash: [0; 32],
            cumulative_difficulty: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityCheckpoint {
    pub block_height: u64,
    pub block_hash: BlockHash,
    pub cumulative_difficulty: u128,
    pub timestamp: DateTime<Utc>,
    pub total_supply: u64,
    pub state_root: [u8; 32],
}

/* --------------------------------------------------------------------------
                                 Blockchain
   ------------------------------------------------------------------------*/
pub struct NumiBlockchain {
    blocks: Arc<RwLock<Vec<Block>>>,
    accounts: DashMap<Vec<u8>, AccountState>,
    mempool: Arc<TransactionMempool>,
    state: Arc<RwLock<ChainState>>,
    miner_keypair: Dilithium3Keypair,
    storage: Option<Arc<BlockchainStorage>>, // optional, for persistence
    consensus: ConsensusConfig,
}

impl NumiBlockchain {
    /* ------------------- construction helpers ----------------------- */
    fn build(kp: Dilithium3Keypair, consensus: ConsensusConfig, storage: Option<Arc<BlockchainStorage>>) -> Result<Self> {
        // Placeholder to wire mempool <-> blockchain without cycles
        let placeholder = Self {
            blocks: Arc::new(RwLock::new(Vec::new())),
            accounts: DashMap::new(),
            mempool: Arc::new(TransactionMempool::new()),
            state: Arc::new(RwLock::new(ChainState::default())),
            miner_keypair: kp.clone(),
            storage: storage.clone(),
            consensus: consensus.clone(),
        };
        let chain_arc = Arc::new(RwLock::new(placeholder));

        // link mempool to chain
        {
            let mut mp = TransactionMempool::new();
            mp.attach_chain(&chain_arc);
            chain_arc.write().mempool = Arc::new(mp);
        }

        // create & apply genesis
        {
            let chain_guard = chain_arc.write();
            let genesis = chain_guard.create_genesis_block()?;
            chain_guard.apply_block(&genesis)?;
            chain_guard.blocks.write().push(genesis.clone());
            
            // CRITICAL FIX: Update chain state for genesis block
            {
                let mut st = chain_guard.state.write();
                st.total_blocks = 1; // Genesis is block 1
                st.best_block_hash = genesis.calculate_hash(None)?;
                st.cumulative_difficulty = genesis.header.difficulty as u128;
                
                // Add genesis mining reward to total supply
                if let Some(reward) = genesis.transactions.iter().find_map(|tx| {
                    if let TransactionType::MiningReward { amount, .. } = tx.kind {
                        Some(amount)
                    } else { None }
                }) {
                    st.total_supply = reward;
                }
                
                // Set initial difficulty
                st.current_difficulty = genesis.header.difficulty;
            }
        }

        Arc::try_unwrap(chain_arc)
            .map(|rw| rw.into_inner())
            .map_err(|_| BlockchainError::ConsensusError("Failed to unwrap Arc during blockchain initialization".into()))
    }

    pub fn new(consensus: ConsensusConfig) -> Result<Self> {
        let kp = WalletManager::load_or_create_miner_wallet(&std::path::PathBuf::from("./core-data"))?;
        Self::build(kp, consensus, None)
    }

    pub fn new_with_keypair(kp: Dilithium3Keypair, consensus: ConsensusConfig) -> Result<Self> {
        Self::build(kp, consensus, None)
    }

    pub fn new_with_config(cfg: ConsensusConfig, kp: Option<Dilithium3Keypair>, storage: Option<Arc<BlockchainStorage>>) -> Result<Self> {
        let keypair = kp.unwrap_or(WalletManager::load_or_create_miner_wallet(&std::path::PathBuf::from("./core-data"))?);
        Self::build(keypair, cfg, storage)
    }

    pub async fn load_from_storage(storage: &Arc<BlockchainStorage>, consensus: ConsensusConfig) -> Result<Self> {
        let dir = storage.blocks_dir();

        if !dir.exists() {
            // No prior data – start fresh, ensuring consensus config is passed.
            return Self::new_with_config(consensus, None, Some(storage.clone()));
        }

        // Build a map height → path so we replay in numeric order
        let mut file_map: BTreeMap<u64, std::path::PathBuf> = BTreeMap::new();
        let mut read_dir = fs::read_dir(&dir).await?;
        while let Some(entry) = read_dir.next_entry().await? {
            if !entry.file_type().await?.is_file() { continue; }
            let fname = entry.file_name().into_string().unwrap_or_default();
            if !fname.starts_with("block_") { continue; }
            if let Ok(height) = fname.trim_start_matches("block_").trim_end_matches(".bin").parse::<u64>() {
                file_map.insert(height, entry.path());
            }
        }
        
        if file_map.is_empty() || !file_map.contains_key(&0) {
            // If no blocks or no genesis block, start fresh.
            return Self::new_with_config(consensus, None, Some(storage.clone()));
        }


        // Load genesis block first to correctly initialize the chain state
        let genesis_path = file_map.remove(&0).unwrap(); // Already checked with contains_key
        let genesis_data = fs::read(genesis_path).await?;
        let genesis_block: Block = bincode::deserialize(&genesis_data)
            .map_err(|e| BlockchainError::SerializationError(e.to_string()))?;

        // Manually build the chain with the loaded genesis block
        let keypair = WalletManager::load_or_create_miner_wallet(&std::path::PathBuf::from("./core-data"))?;
        let chain = Self::build(keypair, consensus, Some(storage.clone()))?;
        
        // This is a bit of a hack. `build` creates its own genesis. We need to replace it.
        chain.blocks.write().clear();
        chain.accounts.clear();
        chain.state.write().total_blocks = 0;

        chain.apply_block(&genesis_block)?;
        chain.blocks.write().push(genesis_block.clone());
        
        {
            let mut st = chain.state.write();
            st.total_blocks = 1;
            st.best_block_hash = genesis_block.calculate_hash(None)?;
            st.cumulative_difficulty = genesis_block.header.difficulty as u128;
            if let Some(reward) = genesis_block.transactions.iter().find_map(|tx| {
                if let TransactionType::MiningReward { amount, .. } = tx.kind { Some(amount) } else { None }
            }) {
                st.total_supply = reward;
            }
            st.current_difficulty = genesis_block.header.difficulty;
        }


        for (_height, path) in file_map {
            let data = fs::read(path).await?;
            let block: Block = bincode::deserialize(&data)
                .map_err(|e| BlockchainError::SerializationError(e.to_string()))?;
            
            // Perform a full validation of the block against the current chain state
            let consensus = &chain.consensus;
            block.validate(chain.blocks.read().last(), consensus)?;

            chain.add_block(block).await?;
        }
        Ok(chain)
    }

    /* ------------------- public accessor API ------------------------ */
    pub fn get_current_height(&self) -> u64 {
        self.state.read().total_blocks.saturating_sub(1)
    }
    pub fn get_latest_block_hash(&self) -> BlockHash {
        self.state.read().best_block_hash
    }
    pub fn get_current_difficulty(&self) -> u32 {
        self.state.read().current_difficulty
    }
    pub fn get_chain_state(&self) -> ChainState {
        self.state.read().clone()
    }
    pub fn get_mempool_stats(&self) -> MempoolStats {
        self.mempool.stats()
    }
    pub fn get_transactions_for_block(&self, max_size: usize, max_count: usize) -> Vec<Transaction> {
        self.mempool.select_for_block(max_size, max_count)
    }
    pub fn get_pending_transaction_count(&self) -> usize {
        self.mempool.stats().total_transactions
    }

    /// Attach a BlockchainStorage after construction (used by tests)
    pub fn attach_storage(&mut self, storage: Arc<BlockchainStorage>) {
        self.storage = Some(storage);
    }
    pub fn get_block_by_height(&self, height: u64) -> Option<Block> {
        self.blocks.read().get(height as usize).cloned()
    }
    pub fn get_block_by_hash(&self, hash: &BlockHash) -> Option<Block> {
        self.blocks
            .read()
            .iter()
            .find(|b| b.calculate_hash(None).ok().map_or(false, |h| &h == hash))
            .cloned()
    }
    /// Return up to `count` headers starting after `start_hash` (empty = genesis)
    pub fn get_block_headers(&self, start_hash: Vec<u8>, count: u32) -> Vec<BlockHeader> {
        let blocks = self.blocks.read();
        let mut headers = Vec::new();

        let start_index = if start_hash.is_empty() {
            0
        } else if start_hash.len() == 32 {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&start_hash);
            blocks
                .iter()
                .position(|b| b.calculate_hash(None).ok().filter(|h| h == &arr).is_some())
                .unwrap_or(blocks.len())
        } else {
            blocks.len()
        };

        blocks.iter().skip(start_index).take(count as usize).for_each(|b| headers.push(b.header.clone()));
        headers
    }
    pub fn get_balance_by_pubkey(&self, pk: &[u8]) -> u64 {
        self.accounts.get(pk).map(|a| a.balance).unwrap_or(0)
    }
    pub fn get_account_state_by_address(&self, address: &str) -> Option<AccountState> {
        if !Self::is_valid_address(address) {
            return None;
        }
        for entry in self.accounts.iter() {
            if self.derive_address(entry.key()) == address {
                return Some(entry.value().clone());
            }
        }
        None
    }
    pub fn get_address_from_public_key(&self, pk: &[u8]) -> String {
        self.derive_address(pk)
    }
    pub fn get_account_state_or_default(&self, pk: &[u8]) -> AccountState {
        self.accounts
            .get(pk)
            .map(|r| r.value().clone())
            .unwrap_or_else(|| AccountState {
                balance: 0,
                nonce: 0,
                transaction_count: 0,
                total_received: 0,
                total_sent: 0,
                created_at: Utc::now(),
                last_activity: Utc::now(),
            })
    }
    pub fn mempool_handle(&self) -> Arc<TransactionMempool> {
        Arc::clone(&self.mempool)
    }

    pub fn consensus_params(&self) -> ConsensusConfig {
        self.consensus.clone()
    }

    pub async fn add_transaction(&self, tx: Transaction) -> Result<ValidationResult> {
        self.mempool.add_transaction(tx).await
    }

    /* ----------------------- block handling ------------------------- */
    pub async fn add_block(&self, block: Block) -> Result<bool> {
        if self.get_block_by_hash(&block.calculate_hash(Some(&self.consensus))?).is_some() {
            return Ok(false);
        }

        if block.header.previous_hash != self.get_latest_block_hash() {
            return Err(InvalidBlockError::StaleChain.into());
        }

        self.apply_block(&block)?;
        self.blocks.write().push(block.clone());

        // update chain state
        {
            let mut st = self.state.write();
            st.total_blocks += 1;
            st.best_block_hash = block.calculate_hash(None)?;
            st.cumulative_difficulty += block.header.difficulty as u128;
            // mint
            if let Some(reward) = block.transactions.iter().find_map(|tx| {
                if let TransactionType::MiningReward { amount, .. } = tx.kind {
                    Some(amount)
                } else { None }
            }) {
                st.total_supply += reward;
            }
            // next difficulty based on recent block solvetime statistics
            st.current_difficulty = next_difficulty(&self.blocks.read(), &self.consensus);
        }

        // remove mined txs
        let ids: Vec<_> = block.transactions.iter().map(|t| t.id).collect();
        self.mempool.remove_transactions(&ids).await;

        // ------------------------------------------------------------------
        // Sync sender nonces in mempool with on-chain state so future
        // submissions from those accounts are validated against the correct
        // expected nonce.
        // ------------------------------------------------------------------
        self.mempool.sync_nonces_from_chain(&self.accounts).await;

        // ------------------------------------------------------------------
        // Persistence: write block file & periodic checkpoint (async)
        // ------------------------------------------------------------------
        if let Some(storage) = self.storage.clone() {
            let dir = storage.blocks_dir();
            let block_clone = block.clone();
            tokio::task::spawn_blocking(move || {
                // Write block file crash-safely
                if let Err(e) = std::fs::create_dir_all(&dir) {
                    log::error!("CRITICAL: Failed to create blocks directory '{}': {}", dir.display(), e);
                    return;
                }
                let path = dir.join(format!("block_{:08}.bin", block_clone.header.height));
                if !path.exists() {
                    match bincode::serialize(&block_clone) {
                        Ok(bytes) => {
                            if let Err(e) = (|| -> std::io::Result<()> {
                                let mut tmp = NamedTempFile::new_in(&dir)?;
                                tmp.write_all(&bytes)?;
                                match tmp.persist(&path) {
                                    Ok(_) => Ok(()),
                                    Err(pe) => Err(pe.error),
                                }
                            })() {
                                log::error!("CRITICAL: Failed to persist block file '{}': {}", path.display(), e);
                            }
                        }
                        Err(e) => log::error!("CRITICAL: Failed to serialize block {}: {}", block_clone.header.height, e),
                    }
                }
            });
        }

        // Checkpoint logic
        if block.header.height % self.consensus.checkpoint_interval == 0 {
            if let Some(storage) = self.storage.clone() {
                let chain_state = self.get_chain_state();
                let state_root = self.calculate_state_root();
                let block_clone = block.clone();

                let checkpoint = SecurityCheckpoint {
                    block_height: block_clone.header.height,
                    block_hash: block_clone.calculate_hash(None).unwrap_or_default(),
                    cumulative_difficulty: chain_state.cumulative_difficulty,
                    timestamp: Utc::now(),
                    total_supply: chain_state.total_supply,
                    state_root,
                };

                let result = storage.transaction(|tx| {
                    tx.save_checkpoint(&checkpoint)
                });

                if let Err(e) = result {
                    log::error!("Failed to save checkpoint: {}", e);
                }
            }
        }
        Ok(true)
    }

    /* ------------------- state-recalc & maintenance ----------------- */
    pub async fn recalculate_and_update_total_supply(&self) -> Result<u64> {
        let supply: u64 = self
            .blocks
            .read()
            .iter()
            .flat_map(|b| &b.transactions)
            .filter_map(|tx| match tx.kind {
                TransactionType::MiningReward { amount, .. } => Some(amount),
                _ => None,
            })
            .sum();
        self.state.write().total_supply = supply;
        Ok(supply)
    }
    /// Persist all blocks to plain files so that the chain can be reloaded on
    /// restart even if the embedded sled database is wiped.
    ///
    /// Each file is named `block_{height}.bin` and contains a bincode‐encoded
    /// `Block`.  This is *not* the primary long-term storage format, but is
    /// sufficient for single-node operation and CI tests.
    pub fn save_to_storage(&self, storage: &BlockchainStorage) -> Result<()> {
        let dir = storage.blocks_dir();
        std::fs::create_dir_all(&dir)?;

        for block in self.blocks.read().iter() {
            let path = dir.join(format!("block_{:08}.bin", block.header.height));
            if path.exists() {
                continue; // already persisted
            }
            let data = bincode::serialize(block)
                .map_err(|e| BlockchainError::SerializationError(e.to_string()))?;
            std::fs::write(path, data)?;
        }
        Ok(())
    }
    pub async fn perform_maintenance(&self) -> Result<()> {
        self.mempool.house_keep().await;
        Ok(())
    }

    /* --------------------- internal helpers ------------------------- */
    fn create_genesis_block(&self) -> Result<Block> {
        let mut tx = Transaction::new(
            self.miner_keypair.public_key.clone(),
            TransactionType::MiningReward {
                block_height: 0,
                amount: self.consensus.initial_mining_reward,
            },
            0,
        );
        tx.sign(&self.miner_keypair)?;
        let mut block = Block::new(
            0,
            [0u8; 32],
            vec![tx],
            1,
            self.miner_keypair.public_key.clone(),
        );
        block.sign(&self.miner_keypair, None)?;
        Ok(block)
    }

    fn apply_block(&self, block: &Block) -> Result<()> {
        // structural validation
        block.validate(self.blocks.read().last(), &self.consensus)?;

        // state transition
        for tx in &block.transactions {
            match &tx.kind {
                TransactionType::Transfer { to, amount, .. } => {
                    // Avoid nested mutable locks on the same DashMap shard.
                    // Holding two entry guards for keys that hash to the same
                    // shard can deadlock.  Handle sender and recipient in
                    // separate scopes so the first guard is dropped before the
                    // second is acquired.

                    // Self-transfer: only the fee is deducted while the nonce
                    // is incremented.
                    if tx.from == *to {
                        let mut acc = self.accounts.entry(tx.from.clone()).or_default();
                        if acc.balance < tx.fee {
                            return Err(BlockchainError::InvalidTransaction("Insufficient balance".into()));
                        }
                        acc.balance -= tx.fee;
                        acc.nonce += 1;
                        // No net amount change, nothing else to do.
                    } else {
                        // 1. Debit sender
                        // NOTE: This logic relies on DashMap's shard locking. Operations
                        // on different keys that hash to the same shard will block.
                        // For high-contention accounts, this could become a bottleneck.
                        {
                            let mut sender = self.accounts.entry(tx.from.clone()).or_default();
                            if sender.balance < amount + tx.fee {
                                return Err(BlockchainError::InvalidTransaction("Insufficient balance".into()));
                            }
                            sender.balance -= amount + tx.fee;
                            sender.nonce += 1;
                        }

                        // 2. Credit recipient (sender guard dropped)
                        {
                            let mut recipient = self.accounts.entry(to.clone()).or_default();
                            recipient.balance += amount;
                        }
                    }
                }
                TransactionType::MiningReward { amount, .. } => {
                    let mut miner = self.accounts.entry(tx.from.clone()).or_default();
                    miner.balance += amount;
                }
            }
        }
        Ok(())
    }

    fn derive_address(&self, pk: &[u8]) -> String {
        let h1 = blake3_hash(pk);
        let mut payload = vec![0u8; 21];
        payload[0] = 0x00; // version byte
        payload[1..].copy_from_slice(&h1[..20]); // Using BLAKE3-160 for the address

        let checksum = &blake3_hash(&blake3_hash(&payload))[..4];
        let mut full = vec![0u8; 25];
        full[..21].copy_from_slice(&payload);
        full[21..].copy_from_slice(checksum);
        bs58::encode(full).into_string()
    }

    pub fn is_valid_address(address: &str) -> bool {
        if let Ok(decoded) = bs58::decode(address).into_vec() {
            if decoded.len() != 25 { return false; }
            let checksum = &decoded[21..];
            let data     = &decoded[..21];
            let hash     = &blake3_hash(&blake3_hash(data))[..4];
            checksum == hash
        } else { false }
    }

    fn calculate_state_root(&self) -> [u8; 32] {
        if self.accounts.is_empty() {
            return [0u8; 32];
        }

        let mut hashes: Vec<[u8; 32]> = self.accounts.iter()
            .map(|entry| {
                let mut hasher = blake3::Hasher::new();
                hasher.update(entry.key());
                if let Ok(value_bytes) = bincode::serialize(entry.value()) {
                    hasher.update(&value_bytes);
                }
                *hasher.finalize().as_bytes()
            })
            .collect();

        while hashes.len() > 1 {
            if hashes.len() % 2 != 0 {
                hashes.push(hashes.last().unwrap().clone());
            }

            hashes = hashes.chunks(2)
                .map(|chunk| {
                    let mut hasher = blake3::Hasher::new();
                    hasher.update(&chunk[0]);
                    hasher.update(&chunk[1]);
                    *hasher.finalize().as_bytes()
                })
                .collect();
        }

        hashes.get(0).cloned().unwrap_or([0u8; 32])
    }
}

/* --------------------------------------------------------------------------
   Pragmatic difficulty adjustment
   ------------------------------------------------------------------------*/

/// Calculate the next difficulty using a bounded moving-average algorithm.
///
/// Rationale & rules:
/// 1.  Use the timestamps of the most recent `window` blocks (default taken from
///     `consensus.difficulty_adjustment_interval`).
/// 2.  Clamp each individual solve-time to the range [`T/4`, `T*4`] to reduce
///     the influence of extreme outliers and timestamp manipulation.
/// 3.  Compute the simple average solve-time across the window.
/// 4.  New difficulty  =  last_difficulty × T / avg_solvetime.
/// 5.  Clamp the result to the range [`last / clamp`, `last × clamp`]
///     (`clamp` defaults to 4) so the difficulty can at most quadruple or
///     quarter in one step.
///
/// The algorithm is intentionally simple, transparent and resistant to common
/// timestamp attacks while being easy to tune via the `ConsensusConfig`.
fn next_difficulty(blocks: &[Block], consensus: &ConsensusConfig) -> u32 {
    // Need at least 2 blocks to measure a solve-time.
    if blocks.len() < 2 {
        return 1;
    }

    let target = consensus.target_block_time.as_secs();
    if target == 0 {
        return 1;
    }

    // ---- collect truncated solve-times ----------------------------------
    let window   = consensus
        .difficulty_adjustment_interval
        .max(1)                                   // avoid 0
        .min((blocks.len() - 1) as u64) as usize; // cannot exceed available

    let lower = (target / 4).max(1) as i64;
    let upper = (target * 4) as i64;

    let mut sum: i64 = 0;
    for i in 0..window {
        let b_i    = &blocks[blocks.len() - 1 - i];
        let b_prev = &blocks[blocks.len() - 2 - i];
        let st = (b_i.header.timestamp.timestamp() - b_prev.header.timestamp.timestamp())
            .max(1); // positive, non-zero
        let st_clamped = st.clamp(lower, upper);
        sum += st_clamped;
    }

    let avg = sum as f64 / window as f64;

    // ---- difficulty factor ----------------------------------------------
    let last_diff = blocks.last().unwrap().header.difficulty as f64;
    let mut new_diff = last_diff * target as f64 / avg;

    // ---- anti-oscillation clamp -----------------------------------------
    let clamp_factor = 4.0; // can be made configurable
    let min_diff = (last_diff / clamp_factor).max(1.0);
    let max_diff = (last_diff * clamp_factor).min(std::u32::MAX as f64);
    new_diff = new_diff.clamp(min_diff, max_diff);

    new_diff.round().max(1.0) as u32
}
