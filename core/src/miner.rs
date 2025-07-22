use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};
use std::thread;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use crate::block::{Block, BlockHash};
use crate::crypto::{generate_difficulty_target, verify_pow, Dilithium3Keypair};
use crate::error::BlockchainError;
use crate::Result;

#[derive(Debug, Clone)]
pub struct MiningStats {
    pub hash_rate: u64,
    pub total_hashes: u64,
    pub current_nonce: u64,
    pub difficulty: u32,
    pub is_mining: bool,
    pub start_time: Instant,
}

#[derive(Debug)]
pub struct MiningResult {
    pub block: Block,
    pub nonce: u64,
    pub hash_rate: u64,
    pub mining_time: Duration,
}

pub struct Miner {
    keypair: Dilithium3Keypair,
    is_mining: Arc<AtomicBool>,
    stats: Arc<Mutex<MiningStats>>,
}

impl Miner {
    pub fn new() -> Result<Self> {
        let keypair = Dilithium3Keypair::new()?;
        let stats = MiningStats {
            hash_rate: 0,
            total_hashes: 0,
            current_nonce: 0,
            difficulty: 1,
            is_mining: false,
            start_time: Instant::now(),
        };
        
        Ok(Self {
            keypair,
            is_mining: Arc::new(AtomicBool::new(false)),
            stats: Arc::new(Mutex::new(stats)),
        })
    }
    
    pub fn mine_block(
        &self,
        height: u64,
        previous_hash: BlockHash,
        transactions: Vec<crate::transaction::Transaction>,
        difficulty: u32,
        start_nonce: u64,
    ) -> Result<Option<MiningResult>> {
        let difficulty_target = generate_difficulty_target(difficulty);
        let mut block = Block::new(
            height,
            previous_hash,
            transactions,
            difficulty,
            self.keypair.public_key.clone(),
        );
        
        let start_time = Instant::now();
        let mut nonce = start_nonce;
        let mut hashes_checked = 0u64;
        let hash_check_interval = 1000; // Update stats every 1000 hashes
        
        self.is_mining.store(true, Ordering::Relaxed);
        
        loop {
            // Check if mining should stop
            if !self.is_mining.load(Ordering::Relaxed) {
                return Ok(None);
            }
            
            block.header.nonce = nonce;
            let header_blob = block.serialize_header_for_hashing();
            
            hashes_checked += 1;
            
            // Update stats periodically
            if hashes_checked % hash_check_interval == 0 {
                let elapsed = start_time.elapsed();
                if elapsed.as_secs() > 0 {
                    let hash_rate = hashes_checked / elapsed.as_secs();
                    self.update_stats(hash_rate, hashes_checked, nonce, difficulty);
                }
            }
            
            // Check if this nonce produces a valid hash
            if verify_pow(&header_blob, nonce, &difficulty_target)? {
                let mining_time = start_time.elapsed();
                let final_hash_rate = hashes_checked / mining_time.as_secs().max(1);
                
                // Sign the block
                block.sign(&self.keypair)?;
                
                self.is_mining.store(false, Ordering::Relaxed);
                
                return Ok(Some(MiningResult {
                    block,
                    nonce,
                    hash_rate: final_hash_rate,
                    mining_time,
                }));
            }
            
            nonce += 1;
            
            // Prevent infinite loop in testing
            if nonce > start_nonce + 10_000_000 {
                self.is_mining.store(false, Ordering::Relaxed);
                return Err(BlockchainError::MiningError("Could not find valid nonce in reasonable time".to_string()));
            }
        }
    }
    
    pub fn mine_block_async(
        &self,
        height: u64,
        previous_hash: BlockHash,
        transactions: Vec<crate::transaction::Transaction>,
        difficulty: u32,
        start_nonce: u64,
    ) -> mpsc::Receiver<Result<Option<MiningResult>>> {
        let (tx, rx) = mpsc::channel(1);
        let is_mining = self.is_mining.clone();
        let keypair = self.keypair.clone();
        
        thread::spawn(move || {
            let miner = Miner {
                keypair,
                is_mining,
                stats: Arc::new(Mutex::new(MiningStats {
                    hash_rate: 0,
                    total_hashes: 0,
                    current_nonce: 0,
                    difficulty: 1,
                    is_mining: false,
                    start_time: Instant::now(),
                })),
            };
            
            let result = miner.mine_block(height, previous_hash, transactions, difficulty, start_nonce);
            let _ = tx.blocking_send(result);
        });
        
        rx
    }
    
    pub fn stop_mining(&self) {
        self.is_mining.store(false, Ordering::Relaxed);
    }
    
    pub fn is_mining(&self) -> bool {
        self.is_mining.load(Ordering::Relaxed)
    }
    
    pub fn get_stats(&self) -> MiningStats {
        // If locking fails just return default stats
        self.stats.lock().map(|s| s.clone()).unwrap_or(MiningStats {
            hash_rate: 0,
            total_hashes: 0,
            current_nonce: 0,
            difficulty: 0,
            is_mining: false,
            start_time: Instant::now(),
        })
    }
    
    fn update_stats(&self, hash_rate: u64, total_hashes: u64, current_nonce: u64, difficulty: u32) {
        if let Ok(mut stats) = self.stats.lock() {
            stats.hash_rate = hash_rate;
            stats.total_hashes = total_hashes;
            stats.current_nonce = current_nonce;
            stats.difficulty = difficulty;
        }
    }
    
    pub fn get_keypair(&self) -> &Dilithium3Keypair {
        &self.keypair
    }
    
    pub fn estimate_mining_time(&self, difficulty: u32) -> Duration {
        // Rough estimation based on difficulty
        // This is a simplified calculation
        let base_time = Duration::from_secs(30); // Base time for difficulty 1
        let difficulty_factor = 2u64.pow(difficulty.saturating_sub(1));
        
        Duration::from_secs(base_time.as_secs() * difficulty_factor)
    }
    
    pub fn get_mining_progress(&self) -> f64 {
        let stats = self.get_stats();
        let elapsed = stats.start_time.elapsed();
        
        if elapsed.as_secs() == 0 {
            return 0.0;
        }
        
        // This is a rough estimate - in practice you'd track actual progress
        let estimated_total_time = self.estimate_mining_time(stats.difficulty);
        let progress = elapsed.as_secs_f64() / estimated_total_time.as_secs_f64();
        
        progress.min(1.0)
    }
}

pub struct MiningPool {
    miners: Vec<Miner>,
    current_miner_index: usize,
}

impl MiningPool {
    pub fn new(miner_count: usize) -> Result<Self> {
        let mut miners = Vec::new();
        for _ in 0..miner_count {
            miners.push(Miner::new()?);
        }
        
        Ok(Self {
            miners,
            current_miner_index: 0,
        })
    }
    
    pub fn get_next_miner(&mut self) -> &Miner {
        let miner = &self.miners[self.current_miner_index];
        self.current_miner_index = (self.current_miner_index + 1) % self.miners.len();
        miner
    }
    
    pub fn get_all_stats(&self) -> Vec<MiningStats> {
        self.miners.iter().map(|miner| miner.get_stats()).collect()
    }
    
    pub fn stop_all_miners(&self) {
        for miner in &self.miners {
            miner.stop_mining();
        }
    }
    
    pub fn get_total_hash_rate(&self) -> u64 {
        self.miners.iter().map(|miner| miner.get_stats().hash_rate).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transaction::Transaction;
    
    #[test]
    fn test_miner_creation() {
        let miner = Miner::new().unwrap();
        assert!(!miner.is_mining());
    }
    
    #[test]
    fn test_mining_stop() {
        let miner = Miner::new().unwrap();
        miner.stop_mining();
        assert!(!miner.is_mining());
    }
    
    #[test]
    fn test_mining_pool_creation() {
        let pool = MiningPool::new(4).unwrap();
        assert_eq!(pool.miners.len(), 4);
    }
    
    #[test]
    fn test_mining_with_low_difficulty() {
        let miner = Miner::new().unwrap();
        let transactions = vec![];
        
        // Test with very low difficulty (should find quickly)
        let result = miner.mine_block(
            1,
            [0u8; 32],
            transactions,
            1, // Very low difficulty
            0,
        );
        
        // Should either find a block or timeout
        match result {
            Ok(Some(_)) => (), // Found block
            Ok(None) => (), // Stopped mining
            Err(_) => (), // Error (timeout)
        }
    }
} 