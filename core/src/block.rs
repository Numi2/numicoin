use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc, Duration};
use crate::crypto::{Hash, blake3_hash, Dilithium3Signature};
use crate::transaction::Transaction;
use crate::error::BlockchainError;
use crate::Result;

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
    
    pub fn calculate_hash(&self) -> Result<BlockHash> {
        let header_data = self.serialize_header_for_hashing()?;
        Ok(crate::crypto::blake3_hash_block(&header_data))
    }
    
    pub fn get_hash_hex(&self) -> Result<String> {
        Ok(crate::crypto::blake3_hash_hex(&self.calculate_hash()?))
    }
    
    pub fn sign(&mut self, keypair: &crate::crypto::Dilithium3Keypair) -> Result<()> {
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
    
    pub fn validate(&self, previous_block: Option<&Block>) -> Result<()> {
        // Verify block signature
        if !self.verify_signature()? {
            return Err(BlockchainError::InvalidBlock("Block signature verification failed".to_string()));
        }
        
        // Verify previous block hash
        if let Some(prev_block) = previous_block {
            if self.header.previous_hash != prev_block.calculate_hash()? {
                return Err(BlockchainError::InvalidBlock("Previous block hash mismatch".to_string()));
            }
            
            if self.header.height != prev_block.header.height + 1 {
                return Err(BlockchainError::InvalidBlock("Invalid block height".to_string()));
            }
        } else {
            // Genesis block
            if self.header.height != 0 {
                return Err(BlockchainError::InvalidBlock("Genesis block must have height 0".to_string()));
            }
            if self.header.previous_hash != [0u8; 32] {
                return Err(BlockchainError::InvalidBlock("Genesis block previous_hash must be zero".to_string()));
            }
        }
        
        // Verify Merkle root
        if !self.verify_merkle_root() {
            return Err(BlockchainError::InvalidBlock("Invalid Merkle root".to_string()));
        }
        
        // Verify transactions
        for tx in &self.transactions {
            if !tx.verify_signature()? {
                return Err(BlockchainError::InvalidTransaction("Transaction signature verification failed".to_string()));
            }
        }
        
        // Verify timestamp is reasonable
        let now = Utc::now();
        let earliest = now - Duration::hours(1);
        let latest = now + Duration::minutes(10);
        if self.header.timestamp < earliest || self.header.timestamp > latest {
            return Err(BlockchainError::InvalidBlock("Block timestamp outside allowed range".to_string()));
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
            .filter(|tx| !tx.is_reward())
            .map(|tx| tx.fee)
            .sum()
    }
    
    pub fn get_mining_reward(&self) -> u64 {
        // Fixed mining reward: 0.005 NUMI per block
        const MINING_REWARD: u64 = 5_000_000; // 0.005 NUMI in smallest units
        MINING_REWARD
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
        let _ = block.calculate_hash().unwrap();
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
        let _ = block.calculate_hash().unwrap();
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
        
        block.sign(&keypair).unwrap();
        assert!(block.verify_signature().unwrap());
        let _ = block.calculate_hash().unwrap();
    }
    
    #[test]
    fn test_genesis_block() {
        let keypair = Dilithium3Keypair::new().unwrap();
        let mut block = Block::new(
            0,
            [0u8; 32],
            vec![],
            1,
            keypair.public_key.clone(),
        );
        
        // Sign the genesis block before validation
        block.sign(&keypair).unwrap();
        
        assert!(block.is_genesis());
        assert!(block.validate(None).is_ok());
        let _ = block.calculate_hash().unwrap();
    }
} 