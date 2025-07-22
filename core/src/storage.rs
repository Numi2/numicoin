use std::path::Path;

use crate::block::Block;
use crate::transaction::Transaction;
use crate::blockchain::{ChainState, AccountState};
use crate::{Result, BlockchainError};

pub struct BlockchainStorage {
    db: sled::Db,
    blocks: sled::Tree,
    transactions: sled::Tree,
    accounts: sled::Tree,
    state: sled::Tree,
}

impl BlockchainStorage {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let db = sled::open(path)
            .map_err(|e| BlockchainError::StorageError(format!("Failed to open database: {}", e)))?;
        
        let blocks = db.open_tree("blocks")
            .map_err(|e| BlockchainError::StorageError(format!("Failed to open blocks tree: {}", e)))?;
        
        let transactions = db.open_tree("transactions")
            .map_err(|e| BlockchainError::StorageError(format!("Failed to open transactions tree: {}", e)))?;
        
        let accounts = db.open_tree("accounts")
            .map_err(|e| BlockchainError::StorageError(format!("Failed to open accounts tree: {}", e)))?;
        
        let state = db.open_tree("state")
            .map_err(|e| BlockchainError::StorageError(format!("Failed to open state tree: {}", e)))?;
        
        Ok(Self {
            db,
            blocks,
            transactions,
            accounts,
            state,
        })
    }
    
    pub fn save_block(&self, block: &Block) -> Result<()> {
        let height_bytes = block.header.height.to_le_bytes();
        let block_data = bincode::serialize(block)
            .map_err(|e| BlockchainError::SerializationError(format!("Failed to serialize block: {}", e)))?;
        
        self.blocks.insert(height_bytes, block_data)
            .map_err(|e| BlockchainError::StorageError(format!("Failed to save block: {}", e)))?;
        
        Ok(())
    }
    
    pub fn load_block(&self, height: u64) -> Result<Option<Block>> {
        let height_bytes = height.to_le_bytes();
        
        if let Some(block_data) = self.blocks.get(height_bytes)
            .map_err(|e| BlockchainError::StorageError(format!("Failed to load block: {}", e)))? {
            
            let block = bincode::deserialize(&block_data)
                .map_err(|e| BlockchainError::SerializationError(format!("Failed to deserialize block: {}", e)))?;
            
            Ok(Some(block))
        } else {
            Ok(None)
        }
    }
    
    pub fn save_transaction(&self, tx_id: &[u8; 32], transaction: &Transaction) -> Result<()> {
        let tx_data = bincode::serialize(transaction)
            .map_err(|e| BlockchainError::SerializationError(format!("Failed to serialize transaction: {}", e)))?;
        
        self.transactions.insert(tx_id, tx_data)
            .map_err(|e| BlockchainError::StorageError(format!("Failed to save transaction: {}", e)))?;
        
        Ok(())
    }
    
    pub fn load_transaction(&self, tx_id: &[u8; 32]) -> Result<Option<Transaction>> {
        if let Some(tx_data) = self.transactions.get(tx_id)
            .map_err(|e| BlockchainError::StorageError(format!("Failed to load transaction: {}", e)))? {
            
            let transaction = bincode::deserialize(&tx_data)
                .map_err(|e| BlockchainError::SerializationError(format!("Failed to deserialize transaction: {}", e)))?;
            
            Ok(Some(transaction))
        } else {
            Ok(None)
        }
    }
    
    pub fn save_account(&self, public_key: &[u8], account: &AccountState) -> Result<()> {
        let account_data = bincode::serialize(account)
            .map_err(|e| BlockchainError::SerializationError(format!("Failed to serialize account: {}", e)))?;
        
        self.accounts.insert(public_key, account_data)
            .map_err(|e| BlockchainError::StorageError(format!("Failed to save account: {}", e)))?;
        
        Ok(())
    }
    
    pub fn load_account(&self, public_key: &[u8]) -> Result<Option<AccountState>> {
        if let Some(account_data) = self.accounts.get(public_key)
            .map_err(|e| BlockchainError::StorageError(format!("Failed to load account: {}", e)))? {
            
            let account = bincode::deserialize(&account_data)
                .map_err(|e| BlockchainError::SerializationError(format!("Failed to deserialize account: {}", e)))?;
            
            Ok(Some(account))
        } else {
            Ok(None)
        }
    }
    
    pub fn save_chain_state(&self, state: &ChainState) -> Result<()> {
        let state_data = bincode::serialize(state)
            .map_err(|e| BlockchainError::SerializationError(format!("Failed to serialize chain state: {}", e)))?;
        
        self.state.insert(b"chain_state", state_data)
            .map_err(|e| BlockchainError::StorageError(format!("Failed to save chain state: {}", e)))?;
        
        Ok(())
    }
    
    pub fn load_chain_state(&self) -> Result<Option<ChainState>> {
        if let Some(state_data) = self.state.get(b"chain_state")
            .map_err(|e| BlockchainError::StorageError(format!("Failed to load chain state: {}", e)))? {
            
            let state = bincode::deserialize(&state_data)
                .map_err(|e| BlockchainError::SerializationError(format!("Failed to deserialize chain state: {}", e)))?;
            
            Ok(Some(state))
        } else {
            Ok(None)
        }
    }
    
    pub fn get_all_blocks(&self) -> Result<Vec<Block>> {
        let mut blocks = Vec::new();
        
        for result in self.blocks.iter() {
            let (_, block_data) = result
                .map_err(|e| BlockchainError::StorageError(format!("Failed to iterate blocks: {}", e)))?;
            
            let block: Block = bincode::deserialize(&block_data)
                .map_err(|e| BlockchainError::SerializationError(format!("Failed to deserialize block: {}", e)))?;
            
            blocks.push(block);
        }
        
        // Sort by height
        blocks.sort_by_key(|block| block.header.height);
        
        Ok(blocks)
    }
    
    pub fn get_all_accounts(&self) -> Result<Vec<(Vec<u8>, AccountState)>> {
        let mut accounts = Vec::new();
        
        for result in self.accounts.iter() {
            let (public_key, account_data) = result
                .map_err(|e| BlockchainError::StorageError(format!("Failed to iterate accounts: {}", e)))?;
            
            let account = bincode::deserialize(&account_data)
                .map_err(|e| BlockchainError::SerializationError(format!("Failed to deserialize account: {}", e)))?;
            
            accounts.push((public_key.to_vec(), account));
        }
        
        Ok(accounts)
    }
    
    pub fn delete_block(&self, height: u64) -> Result<()> {
        let height_bytes = height.to_le_bytes();
        self.blocks.remove(height_bytes)
            .map_err(|e| BlockchainError::StorageError(format!("Failed to delete block: {}", e)))?;
        
        Ok(())
    }
    
    pub fn delete_transaction(&self, tx_id: &[u8; 32]) -> Result<()> {
        self.transactions.remove(tx_id)
            .map_err(|e| BlockchainError::StorageError(format!("Failed to delete transaction: {}", e)))?;
        
        Ok(())
    }
    
    pub fn delete_account(&self, public_key: &[u8]) -> Result<()> {
        self.accounts.remove(public_key)
            .map_err(|e| BlockchainError::StorageError(format!("Failed to delete account: {}", e)))?;
        
        Ok(())
    }
    
    pub fn compact(&self) -> Result<()> {
        // Sled doesn't have a compact method, so we'll just flush
        self.flush()
    }
    
    pub fn flush(&self) -> Result<()> {
        self.db.flush()
            .map_err(|e| BlockchainError::StorageError(format!("Failed to flush database: {}", e)))?;
        
        Ok(())
    }
    
    pub fn get_database_size(&self) -> Result<u64> {
        let size = self.db.size_on_disk()
            .map_err(|e| BlockchainError::StorageError(format!("Failed to get database size: {}", e)))?;
        
        Ok(size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use crate::crypto::Dilithium3Keypair;
    use crate::transaction::{Transaction, TransactionType};
    use crate::block::Block;
    use chrono::Utc;
    
    #[test]
    fn test_storage_creation() {
        let temp_dir = tempdir().unwrap();
        let storage = BlockchainStorage::new(temp_dir.path()).unwrap();
        // Database size might be 0 initially, so we'll just check that it doesn't panic
        let _size = storage.get_database_size().unwrap();
    }
    
    #[test]
    fn test_block_storage() {
        let temp_dir = tempdir().unwrap();
        let storage = BlockchainStorage::new(temp_dir.path()).unwrap();
        
        let keypair = Dilithium3Keypair::new().unwrap();
        let block = Block::new(
            1,
            [0u8; 32],
            vec![],
            1,
            keypair.public_key.clone(),
        );
        
        storage.save_block(&block).unwrap();
        let loaded_block = storage.load_block(1).unwrap().unwrap();
        
        assert_eq!(block.header.height, loaded_block.header.height);
    }
    
    #[test]
    fn test_transaction_storage() {
        let temp_dir = tempdir().unwrap();
        let storage = BlockchainStorage::new(temp_dir.path()).unwrap();
        
        let keypair = Dilithium3Keypair::new().unwrap();
        let tx = Transaction::new(
            keypair.public_key.clone(),
            TransactionType::Transfer {
                to: vec![1, 2, 3, 4],
                amount: 100,
            },
            1,
        );
        
        storage.save_transaction(&tx.id, &tx).unwrap();
        let loaded_tx = storage.load_transaction(&tx.id).unwrap().unwrap();
        
        assert_eq!(tx.id, loaded_tx.id);
    }
    
    #[test]
    fn test_account_storage() {
        let temp_dir = tempdir().unwrap();
        let storage = BlockchainStorage::new(temp_dir.path()).unwrap();
        
        let account = AccountState {
            balance: 1000,
            nonce: 1,
            staked_amount: 500,
            last_stake_time: Utc::now(),
        };
        
        let public_key = vec![1, 2, 3, 4];
        storage.save_account(&public_key, &account).unwrap();
        let loaded_account = storage.load_account(&public_key).unwrap().unwrap();
        
        assert_eq!(account.balance, loaded_account.balance);
    }
} 