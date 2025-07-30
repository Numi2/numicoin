use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc, Duration};
use crate::crypto::{Hash, blake3_hash, blake3_hash_block, Dilithium3Signature, generate_difficulty_target, argon2d_pow};
use crate::config::ConsensusConfig;
use crate::transaction::Transaction;
use crate::error::{BlockchainError, InvalidBlockError};
use crate::Result;
use rayon::prelude::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub type BlockHash = [u8; 32];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockHeader {
    pub version: u32,
    pub height: u64,
    pub timestamp: DateTime<Utc>,
    pub previous_hash: BlockHash,
    pub merkle_root: Hash,
    pub difficulty: u32,
    pub nonce: u64,
    pub miner_public_key: Vec<u8>,
    pub block_signature: Option<Dilithium3Signature>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    pub header: BlockHeader,
    pub transactions: Vec<Transaction>,
}

impl Block {
    pub fn new(
        height: u64,
        previous_hash: BlockHash,
        transactions: Vec<Transaction>,
        difficulty: u32,
        miner_public_key: Vec<u8>,
    ) -> Self {
        let merkle_root = Self::calculate_merkle_root(&transactions);
        
        let header = BlockHeader {
            version: 1,
            height,
            timestamp: Utc::now(),
            previous_hash,
            merkle_root,
            difficulty,
            nonce: 0,
            miner_public_key,
            block_signature: None,
        };
        
        Self {
            header,
            transactions,
        }
    }
    
    pub fn calculate_hash(&self, consensus: Option<&ConsensusConfig>) -> Result<BlockHash> {
        let header_data = self.serialize_header_for_hashing()?;
        if let Some(cfg) = consensus {
            let salt = &blake3_hash(&header_data)[..16];
            let pow_hash = argon2d_pow(&header_data, salt, &cfg.argon2_config)?;
            Ok(blake3_hash_block(&pow_hash))
        } else {
            Ok(blake3_hash_block(&header_data))
        }
    }
    
    pub fn get_hash_hex(&self) -> Result<String> {
        Ok(crate::crypto::blake3_hash_hex(&self.calculate_hash(None)?))
    }
    
    pub fn sign(&mut self, keypair: &crate::crypto::Dilithium3Keypair, coinbase_tx: Option<&mut Transaction>) -> Result<()> {
        if let Some(tx) = coinbase_tx {
            tx.sign(keypair)?;
            self.transactions.insert(0, tx.clone());
            self.header.merkle_root = Self::calculate_merkle_root(&self.transactions);
        }

        let message = self.serialize_header_for_hashing()?;
        self.header.block_signature = Some(keypair.sign(&message)?);
        Ok(())
    }
    
    pub fn verify_signature(&self) -> Result<bool> {
        if let Some(ref signature) = self.header.block_signature {
            let message = self.serialize_header_for_hashing()?;
            crate::crypto::Dilithium3Keypair::verify(&message, signature, &self.header.miner_public_key)
        } else {
            Ok(false)
        }
    }
    
    pub fn calculate_merkle_root(transactions: &[Transaction]) -> Hash {
        if transactions.is_empty() {
            return [0u8; 32];
        }
        
        let mut hashes: Vec<Hash> = transactions.iter()
            .map(|tx| tx.id)
            .collect();
        
        // Build Merkle tree
        while hashes.len() > 1 {
            let mut new_hashes = Vec::new();
            
            for chunk in hashes.chunks(2) {
                let mut combined = Vec::new();
                combined.extend_from_slice(&chunk[0]);
                if chunk.len() > 1 {
                    combined.extend_from_slice(&chunk[1]);
                } else {
                    combined.extend_from_slice(&chunk[0]); // Duplicate for odd number
                }
                new_hashes.push(blake3_hash(&combined));
            }
            
            hashes = new_hashes;
        }
        
        hashes[0]
    }
    
    pub fn verify_merkle_root(&self) -> bool {
        let calculated_root = Self::calculate_merkle_root(&self.transactions);
        calculated_root == self.header.merkle_root
    }
    
    pub fn validate(&self, previous_block: Option<&Block>, consensus: &crate::config::ConsensusConfig) -> Result<()> {
        // Skip PoW check for genesis
        if !self.is_genesis() {
            let target = generate_difficulty_target(self.header.difficulty);
            if !crate::crypto::verify_pow(&self.serialize_header_for_hashing()?, self.header.nonce, &target, consensus)? {
                return Err(InvalidBlockError::InvalidPoW.into());
            }
        }

        // Verify block signature
        if !self.verify_signature()? {
            return Err(InvalidBlockError::SignatureVerificationFailed.into());
        }
        
        // Verify previous block hash
        if let Some(prev_block) = previous_block {
            if self.header.previous_hash != prev_block.calculate_hash(None)? {
                return Err(InvalidBlockError::PreviousBlockHashMismatch.into());
            }
            
            if self.header.height != prev_block.header.height + 1 {
                return Err(InvalidBlockError::InvalidBlockHeight.into());
            }
            // Timestamp validation against the previous block
            let max_future_drift = Duration::minutes(5);
            if self.header.timestamp <= prev_block.header.timestamp {
                return Err(InvalidBlockError::TimestampOutOfRange(
                    "Block timestamp must be greater than the previous block's timestamp".to_string()
                ).into());
            }
            if self.header.timestamp > Utc::now() + max_future_drift {
                return Err(InvalidBlockError::TimestampOutOfRange(
                    "Block timestamp is too far in the future".to_string()
                ).into());
            }
        } else {
            if self.header.height != 0 {
                return Err(InvalidBlockError::GenesisBlockHeightNotZero.into());
            }
            if self.header.previous_hash != [0u8; 32] {
                return Err(InvalidBlockError::GenesisBlockHashNotZero.into());
            }
            // Additional genesis block validation
            if self.transactions.len() != 1 {
                return Err(InvalidBlockError::GenesisBlockInvalidTransactionCount.into());
            }
            if !matches!(self.transactions[0].kind, crate::transaction::TransactionType::MiningReward { .. }) {
                return Err(InvalidBlockError::GenesisBlockTransactionNotReward.into());
            }
        }
        
        // Verify Merkle root
        if !self.verify_merkle_root() {
            return Err(InvalidBlockError::InvalidMerkleRoot.into());
        }
        
        // Verify transactions
        for tx in &self.transactions {
            if !tx.verify_signature()? {
                return Err(InvalidBlockError::InvalidTransaction("Transaction signature verification failed".to_string()).into());
            }
        }

        // ---------------- Mining-reward checks --------------------
        use crate::transaction::TransactionType;

        // Gather reward transactions
        let reward_txs: Vec<&crate::transaction::Transaction> = self
            .transactions
            .iter()
            .filter(|tx| matches!(tx.kind, TransactionType::MiningReward { .. }))
            .collect();

        if self.is_genesis() {
            // Already ensured there is exactly one tx and it is a MiningReward.
            let reward_tx = reward_txs[0];
            if let TransactionType::MiningReward { block_height, amount } = reward_tx.kind {
                if block_height != 0 {
                    return Err(InvalidBlockError::InvalidBlockHeight.into());
                }
                if amount != consensus.initial_mining_reward {
                    return Err(InvalidBlockError::InvalidRewardAmount.into());
                }
            }
        } else {
            // Non-genesis: must contain exactly one MiningReward
            if reward_txs.len() != 1 {
                return Err(InvalidBlockError::InvalidRewardTransactionCount.into());
            }

            let reward_tx = reward_txs[0];
            let subsidy = crate::miner::WalletManager::calculate_mining_reward_with_config(
                self.header.height,
                consensus,
            );
            let expected = subsidy + self.get_total_fees();

            if let TransactionType::MiningReward { block_height, amount } = reward_tx.kind {
                if block_height != self.header.height {
                    return Err(InvalidBlockError::InvalidBlockHeight.into());
                }
                if amount != expected {
                    return Err(InvalidBlockError::InvalidRewardAmount.into());
                }
            }

            // Ensure reward tx is first
            if !matches!(self.transactions.first().map(|tx| &tx.kind), Some(TransactionType::MiningReward { .. })) {
                return Err(InvalidBlockError::RewardTransactionNotFirst.into());
            }
        }
        
        Ok(())
    }
    
    pub fn is_genesis(&self) -> bool {
        self.header.height == 0
    }
    
    pub fn get_transaction_count(&self) -> usize {
        self.transactions.len()
    }
    
    pub fn get_total_fees(&self) -> u64 {
        self.transactions.iter()
            .filter(|tx| !matches!(tx.kind, crate::transaction::TransactionType::MiningReward { .. }))
            .map(|tx| tx.fee)
            .sum()
    }

    /// Calculate the block subsidy based on the provided consensus settings.
    pub fn calculate_block_reward(&self, consensus: &crate::config::ConsensusConfig) -> u64 {
        let halvings = self.header.height / consensus.mining_reward_halving_interval;
        if halvings >= 64 {
            0
        } else {
            consensus.initial_mining_reward >> halvings
        }
    }
    
    pub fn serialize_header_for_hashing(&self) -> Result<Vec<u8>> {
        // Create header data without signature for hashing
        let header_data = HeaderForHashing {
            version: self.header.version,
            height: self.header.height,
            timestamp: self.header.timestamp,
            previous_hash: self.header.previous_hash,
            merkle_root: self.header.merkle_root,
            difficulty: self.header.difficulty,
            nonce: self.header.nonce,
            miner_public_key: self.header.miner_public_key.clone(),
        };
        bincode::serialize(&header_data).map_err(|e| BlockchainError::SerializationError(e.to_string()))
    }

    pub fn mine(&mut self, keypair: &crate::crypto::Dilithium3Keypair, consensus: &ConsensusConfig) -> Result<()> {
        let target = generate_difficulty_target(self.header.difficulty);
        let stop_flag = Arc::new(AtomicBool::new(false));

        let found_nonce = (0..u64::MAX)
            .into_par_iter()
            .find_any(|&nonce| {
                if stop_flag.load(Ordering::Relaxed) {
                    return true;
                }
                let mut block_header = self.header.clone();
                block_header.nonce = nonce;
                
                let header_data = HeaderForHashing {
                    version: block_header.version,
                    height: block_header.height,
                    timestamp: block_header.timestamp,
                    previous_hash: block_header.previous_hash,
                    merkle_root: block_header.merkle_root,
                    difficulty: block_header.difficulty,
                    nonce: block_header.nonce,
                    miner_public_key: block_header.miner_public_key.clone(),
                };
                
                if let Ok(serialized_header) = bincode::serialize(&header_data) {
                    if let Ok(hash) = self.calculate_hash_with_header(&serialized_header, consensus) {
                        if crate::blockchain::meets_target(&hash, &target) {
                            stop_flag.store(true, Ordering::Relaxed);
                            return true;
                        }
                    }
                }
                false
            });

        if let Some(nonce) = found_nonce {
            self.header.nonce = nonce;
            self.sign(keypair, None)?;
            Ok(())
        } else {
            Err(BlockchainError::MiningError("Failed to find a valid nonce".to_string()))
        }
    }

    fn calculate_hash_with_header(&self, header_data: &[u8], consensus: &ConsensusConfig) -> Result<BlockHash> {
        let salt = &blake3_hash(header_data)[..16];
        let pow_hash = argon2d_pow(header_data, salt, &consensus.argon2_config)?;
        Ok(blake3_hash_block(&pow_hash))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HeaderForHashing {
    version: u32,
    height: u64,
    timestamp: DateTime<Utc>,
    previous_hash: BlockHash,
    merkle_root: Hash,
    difficulty: u32,
    nonce: u64,
    miner_public_key: Vec<u8>,
}

impl BlockHeader {
    pub fn get_serialized_size(&self) -> Result<usize> {
        bincode::serialized_size(self)
            .map(|s| s as usize)
            .map_err(|e| BlockchainError::SerializationError(e.to_string()))
    }

    pub fn calculate_hash(&self) -> Result<BlockHash> {
        // Create header data without signature for hashing
        let header_data = HeaderForHashing {
            version: self.version,
            height: self.height,
            timestamp: self.timestamp,
            previous_hash: self.previous_hash,
            merkle_root: self.merkle_root,
            difficulty: self.difficulty,
            nonce: self.nonce,
            miner_public_key: self.miner_public_key.clone(),
        };
        let serialized = bincode::serialize(&header_data)
            .map_err(|e| BlockchainError::SerializationError(e.to_string()))?;
        Ok(crate::crypto::blake3_hash_block(&serialized))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::Dilithium3Keypair;
    use crate::transaction::TransactionType;
    
    #[test]
    fn test_block_creation() {
        let keypair = Dilithium3Keypair::new().unwrap();
        let transactions = vec![
            Transaction::new(
                keypair.public_key.clone(),
                TransactionType::Transfer {
                    to: vec![1, 2, 3, 4],
                    amount: 100,
                    memo: None,
                },
                1,
            )
        ];
        
        let block = Block::new(
            1,
            [0u8; 32],
            transactions,
            2,
            keypair.public_key.clone(),
        );
        
        assert_eq!(block.header.height, 1);
        assert_eq!(block.header.difficulty, 2);
        assert_eq!(block.get_transaction_count(), 1);
        let _ = block.calculate_hash(None).unwrap();
    }
    
    #[test]
    fn test_merkle_root_calculation() {
        let keypair = Dilithium3Keypair::new().unwrap();
        let transactions = vec![
            Transaction::new(
                keypair.public_key.clone(),
                TransactionType::Transfer {
                    to: vec![1, 2, 3, 4],
                    amount: 100,
                    memo: None,
                },
                1,
            ),
            Transaction::new(
                keypair.public_key.clone(),
                TransactionType::Transfer {
                    to: vec![5, 6, 7, 8],
                    amount: 200,
                    memo: None,
                },
                2,
            ),
        ];
        
        let merkle_root = Block::calculate_merkle_root(&transactions);
        assert_ne!(merkle_root, [0u8; 32]);
        let block = Block::new(
            1,
            [0u8; 32],
            transactions,
            2,
            keypair.public_key.clone(),
        );
        let _ = block.calculate_hash(None).unwrap();
    }
    
    #[test]
    fn test_block_signing() {
        let keypair = Dilithium3Keypair::new().unwrap();
        let mut block = Block::new(
            1,
            [0u8; 32],
            vec![],
            2,
            keypair.public_key.clone(),
        );
        
        block.sign(&keypair, None).unwrap();
        assert!(block.verify_signature().unwrap());
        let _ = block.calculate_hash(None).unwrap();
    }
    
    #[test]
    fn test_genesis_block() {
        let keypair = Dilithium3Keypair::new().unwrap();
        use crate::transaction::{Transaction, TransactionType};
        use crate::config::ConsensusConfig;

        let consensus = ConsensusConfig::default();

        let mut reward_tx = Transaction::new(
            keypair.public_key.clone(),
            TransactionType::MiningReward {
                block_height: 0,
                amount: consensus.initial_mining_reward,
            },
            0,
        );
        reward_tx.sign(&keypair).unwrap();

        let mut block = Block::new(
            0,
            [0u8; 32],
            vec![reward_tx],
            1,
            keypair.public_key.clone(),
        );
        
        // Sign the genesis block before validation
        block.sign(&keypair, None).unwrap();
        
        assert!(block.is_genesis());
        let consensus = crate::config::ConsensusConfig::default();
        assert!(block.validate(None, &consensus).is_ok());
        let _ = block.calculate_hash(None).unwrap();
    }
}
