use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

use crossbeam::channel::{tick, unbounded, Receiver, Sender};
use rand::{rngs::SmallRng, Rng, SeedableRng};
use crate::RwLock;

use crate::{
    blockchain::NumiBlockchain,
    block::Block,
    crypto::blake3_hash,
    miner::Miner,
    config::ConsensusConfig,
};

pub struct LocalMiner {
    stop: Arc<AtomicBool>,
    block_tx: Sender<Block>,
    stats: Arc<MiningStats>,
    consensus: ConsensusConfig,
}

#[derive(Debug)]
struct MiningStats {
    hashes_attempted: AtomicU64,
    blocks_found: AtomicU64,
    last_status_report: parking_lot::Mutex<Instant>,
}

impl LocalMiner {
    pub fn spawn(
        chain: Arc<RwLock<NumiBlockchain>>,
        miner: Arc<RwLock<Miner>>,
        threads: usize,
        consensus: ConsensusConfig,
        stratum_connected_rx: Receiver<bool>, // true = at least one miner
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let (block_tx, block_rx) = unbounded::<Block>();
        let stats = Arc::new(MiningStats {
            hashes_attempted: AtomicU64::new(0),
            blocks_found: AtomicU64::new(0),
            last_status_report: parking_lot::Mutex::new(Instant::now()),
        });
        
        // Spawn block processor task
        let chain_clone = chain.clone();
        let stats_clone = stats.clone();
        tokio::spawn(async move {
            Self::block_processor_task(chain_clone, block_rx, stats_clone).await;
        });

        // Spawn mining threads
        for thread_id in 0..threads {
            let chain = chain.clone();
            let miner_ref = miner.clone();
            let stop_flag = stop.clone();
            let stratum_rx = stratum_connected_rx.clone();
            let block_sender = block_tx.clone();
            let stats_ref = stats.clone();
            let consensus_clone = consensus.clone();
            thread::spawn(move || {
                Self::mining_loop(thread_id, chain, miner_ref, stop_flag, stratum_rx, block_sender, stats_ref, consensus_clone)
            });
        }
        
        // Spawn status reporter
        let stats_clone = stats.clone();
        let stop_clone = stop.clone();
        tokio::spawn(async move {
            Self::status_reporter_task(stats_clone, stop_clone).await;
        });

        Self { stop, block_tx, stats, consensus }
    }

    pub fn shutdown(&self) {
        self.stop.store(true, Ordering::SeqCst);
        log::info!("🛑 Local CPU miner stopped");
    }
    
    async fn block_processor_task(
        chain: Arc<RwLock<NumiBlockchain>>,
        block_rx: Receiver<Block>,
        stats: Arc<MiningStats>,
    ) {
        while let Ok(block) = block_rx.recv() {
            let height = block.header.height;
            
            // Use spawn_blocking to handle the parking_lot RwLock which is not Send
            let chain_clone = chain.clone();
            let block_clone = block.clone();
            let result = tokio::task::spawn_blocking(move || {
                futures::executor::block_on(async {
                    chain_clone.write().add_block(block_clone).await
                })
            }).await;
            
            match result {
                Ok(Ok(true)) => {
                    stats.blocks_found.fetch_add(1, Ordering::Relaxed);
                    log::info!("🎉 CPU-miner found valid block #{height}!");
                }
                Ok(Ok(false)) => {
                    log::warn!("⚠️  CPU-miner block #{height} was already known");
                }
                Ok(Err(e)) => {
                    log::warn!("❌ CPU-miner block #{height} rejected: {e}");
                }
                Err(e) => {
                    log::error!("❌ CPU-miner task error for block #{height}: {e}");
                }
            }
        }
    }
    
    async fn status_reporter_task(stats: Arc<MiningStats>, stop: Arc<AtomicBool>) {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        let mut last_hashes = 0u64;
        
        while !stop.load(Ordering::Relaxed) {
            interval.tick().await;
            
            let current_hashes = stats.hashes_attempted.load(Ordering::Relaxed);
            let blocks_found = stats.blocks_found.load(Ordering::Relaxed);
            let hash_rate = (current_hashes - last_hashes) / 30; // hashes per second over 30s
            
            if current_hashes > 0 {
                log::info!("⛏️  CPU Mining Status: ~{} H/s, {} total hashes, {} blocks found", 
                    format_hash_rate(hash_rate), 
                    format_number(current_hashes),
                    blocks_found
                );
            }
            
            last_hashes = current_hashes;
        }
    }

    fn mining_loop(
        _thread_id: usize,
        chain: Arc<RwLock<NumiBlockchain>>,
        miner: Arc<RwLock<Miner>>,
        stop: Arc<AtomicBool>,
        stratum_rx: Receiver<bool>,
        block_tx: Sender<Block>,
        stats: Arc<MiningStats>,
        consensus: ConsensusConfig,
    ) {
        let mut rng = SmallRng::from_entropy();
        let tick = tick(Duration::from_millis(500));
        let mut local_hash_count = 0u64;
        let mut status_counter = 0u32;

        // Track when we last constructed a new block template so we don't
        // stamp successive blocks with sub-second timestamps (which wreaks
        // havoc on the LWMA difficulty algorithm).
        let mut last_template_time = Instant::now() - Duration::from_secs(1);

        while !stop.load(Ordering::Relaxed) {
            // Pause if stratum miners present
            if stratum_rx.try_iter().last().unwrap_or(false) {
                if local_hash_count > 0 {
                    log::info!("⏸️  CPU miner paused - external Stratum miners connected");
                    local_hash_count = 0; // Reset so we don't log this repeatedly
                }
                thread::sleep(Duration::from_secs(1));
                continue;
            }
            
            // Resume message (only once)
            if local_hash_count == 0 {
                log::info!("▶️  CPU miner active - searching for blocks...");
            }

            // Throttle template creation to at most once per second to keep
            // header.timestamp monotonic and ≥ 1 s apart.
            let now = Instant::now();
            if now.duration_since(last_template_time) < Duration::from_secs(1) {
                thread::sleep(Duration::from_millis(200));
                continue;
            }

            last_template_time = now;

            // Build candidate block with proper miner public key
            let (prev_hash, height, difficulty, txs, miner_public_key) = {
                let bc = chain.read();
                let miner_pk = miner.read().get_public_key();
                (
                    bc.get_latest_block_hash(),
                    bc.get_current_height() + 1,
                    bc.get_current_difficulty(),
                    bc.get_transactions_for_block(256 * 1024, 10_000),
                    miner_pk,
                )
            };

            // ------------------------------------------------------------------
            // Create mining-reward (coinbase) transaction and prepend it
            // ------------------------------------------------------------------
            use crate::transaction::{Transaction, TransactionType};
            use crate::miner::WalletManager;

            // Block subsidy according to halving schedule
            let base_reward = WalletManager::calculate_mining_reward_with_config(height, &consensus);
            // Sum of fees from included mempool transactions
            let total_fees: u64 = txs.iter().map(|tx| tx.fee).sum();
            let reward_amount = base_reward.saturating_add(total_fees);

            let mut reward_tx = Transaction::new(
                miner_public_key.clone(),
                TransactionType::MiningReward {
                    block_height: height,
                    amount: reward_amount,
                },
                0,
            );

            if let Err(e) = reward_tx.sign(&miner.read().get_keypair()) {
                log::error!("❌ Failed to sign reward transaction: {e}");
                continue; // Skip this iteration and try again
            }

            // Combine reward + normal transactions (reward must be first)
            let mut full_txs = Vec::with_capacity(1 + txs.len());
            full_txs.push(reward_tx);
            full_txs.extend(txs);

            let mut block = Block::new(height, prev_hash, full_txs, difficulty, miner_public_key);
            // Ensure Merkle root includes reward tx
            block.header.merkle_root = Block::calculate_merkle_root(&block.transactions);
            let target = crate::crypto::generate_difficulty_target(difficulty);

            loop {
                if stop.load(Ordering::Relaxed) { return; }
                if let Ok(signal) = stratum_rx.try_recv() {
                    if signal { break; } // external miner appeared
                }

                block.header.nonce = rng.gen();
                let hash = blake3_hash(&block.serialize_header_for_hashing().unwrap());
                local_hash_count += 1;
                status_counter += 1;
                
                // Update global stats every 1000 hashes to reduce contention
                if status_counter >= 1000 {
                    stats.hashes_attempted.fetch_add(status_counter as u64, Ordering::Relaxed);
                    status_counter = 0;
                }
                
                if hash < target {
                    // Found a block! Sign it properly before sending for processing
                    let mut signed_block = block.clone();
                    if let Err(e) = signed_block.sign(&miner.read().get_keypair(), None) {
                        log::error!("❌ Failed to sign found block: {}", e);
                        break;
                    }
                    
                    if let Err(_) = block_tx.send(signed_block) {
                        log::error!("Failed to send found block to processor");
                    }
                    break;
                }

                // Yield periodically
                if tick.recv().is_ok() { }
            }
        }
        
        // Update final stats before thread exits
        if status_counter > 0 {
            stats.hashes_attempted.fetch_add(status_counter as u64, Ordering::Relaxed);
        }
    }
}

fn format_hash_rate(hashes_per_sec: u64) -> String {
    if hashes_per_sec >= 1_000_000_000 {
        format!("{:.1} GH", hashes_per_sec as f64 / 1_000_000_000.0)
    } else if hashes_per_sec >= 1_000_000 {
        format!("{:.1} MH", hashes_per_sec as f64 / 1_000_000.0)
    } else if hashes_per_sec >= 1_000 {
        format!("{:.1} KH", hashes_per_sec as f64 / 1_000.0)
    } else {
        format!("{} H", hashes_per_sec)
    }
}

fn format_number(num: u64) -> String {
    if num >= 1_000_000_000 {
        format!("{:.1}B", num as f64 / 1_000_000_000.0)
    } else if num >= 1_000_000 {
        format!("{:.1}M", num as f64 / 1_000_000.0)
    } else if num >= 1_000 {
        format!("{:.1}K", num as f64 / 1_000.0)
    } else {
        format!("{}", num)
    }
}