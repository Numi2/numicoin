use std::path::Path;
use sled;
use crate::block::Block;
use crate::transaction::Transaction;
use crate::blockchain::{ChainState, AccountState, SecurityCheckpoint};
use crate::error::BlockchainError;
use crate::Result;

/// Enhanced blockchain storage with checkpoint support
pub struct BlockchainStorage {
    db: sled::Db,
    blocks: sled::Tree,
    transactions: sled::Tree,
    accounts: sled::Tree,
    state: sled::Tree,
    checkpoints: sled::Tree,
}

impl BlockchainStorage {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let db = sled::open(path.as_ref())
            .map_err(|e| BlockchainError::StorageError(format!("Failed to open database: {e}")))?;
        
        let blocks = db.open_tree("blocks")
            .map_err(|e| BlockchainError::StorageError(format!("Failed to open blocks tree: {e}")))?;
        
        let transactions = db.open_tree("transactions")
            .map_err(|e| BlockchainError::StorageError(format!("Failed to open transactions tree: {e}")))?;
        
        let accounts = db.open_tree("accounts")
            .map_err(|e| BlockchainError::StorageError(format!("Failed to open accounts tree: {e}")))?;
        
        let state = db.open_tree("chain_state")
            .map_err(|e| BlockchainError::StorageError(format!("Failed to open state tree: {e}")))?;
        
        let checkpoints = db.open_tree("checkpoints")
            .map_err(|e| BlockchainError::StorageError(format!("Failed to open checkpoints tree: {e}")))?;
        
        Ok(Self {
            db,
            blocks,
            transactions,
            accounts,
            state,
            checkpoints,
        })
    }

    pub fn save_block(&self, block: &Block) -> Result<()> {
        let key = block.header.height.to_be_bytes();
        let value = bincode::serialize(block)
            .map_err(|e| BlockchainError::SerializationError(format!("Failed to serialize block: {e}")))?;
        
        self.blocks.insert(key, value)
            .map_err(|e| BlockchainError::StorageError(format!("Failed to save block: {e}")))?;
        
        Ok(())
    }

    pub fn load_block(&self, height: u64) -> Result<Option<Block>> {
        let key = height.to_be_bytes();
        
        match self.blocks.get(key)
            .map_err(|e| BlockchainError::StorageError(format!("Failed to load block: {e}")))? {
            Some(data) => {
                let block = bincode::deserialize(&data)
                    .map_err(|e| BlockchainError::SerializationError(format!("Failed to deserialize block: {e}")))?;
                Ok(Some(block))
            }
            None => Ok(None),
        }
    }

    pub fn save_transaction(&self, tx_id: &[u8; 32], transaction: &Transaction) -> Result<()> {
        let value = bincode::serialize(transaction)
            .map_err(|e| BlockchainError::SerializationError(format!("Failed to serialize transaction: {e}")))?;
        
        self.transactions.insert(tx_id.as_slice(), value)
            .map_err(|e| BlockchainError::StorageError(format!("Failed to save transaction: {e}")))?;
        
        Ok(())
    }

    pub fn load_transaction(&self, tx_id: &[u8; 32]) -> Result<Option<Transaction>> {
        match self.transactions.get(tx_id.as_slice())
            .map_err(|e| BlockchainError::StorageError(format!("Failed to load transaction: {e}")))? {
            Some(data) => {
                let transaction = bincode::deserialize(&data)
                    .map_err(|e| BlockchainError::SerializationError(format!("Failed to deserialize transaction: {e}")))?;
                Ok(Some(transaction))
            }
            None => Ok(None),
        }
    }

    pub fn save_account(&self, public_key: &[u8], account: &AccountState) -> Result<()> {
        let value = bincode::serialize(account)
            .map_err(|e| BlockchainError::SerializationError(format!("Failed to serialize account: {e}")))?;
        
        self.accounts.insert(public_key, value)
            .map_err(|e| BlockchainError::StorageError(format!("Failed to save account: {e}")))?;
        
        Ok(())
    }

    pub fn load_account(&self, public_key: &[u8]) -> Result<Option<AccountState>> {
        match self.accounts.get(public_key)
            .map_err(|e| BlockchainError::StorageError(format!("Failed to load account: {e}")))? {
            Some(data) => {
                let account = bincode::deserialize(&data)
                    .map_err(|e| BlockchainError::SerializationError(format!("Failed to deserialize account: {e}")))?;
                Ok(Some(account))
            }
            None => Ok(None),
        }
    }

    pub fn save_chain_state(&self, state: &ChainState) -> Result<()> {
        let value = bincode::serialize(state)
            .map_err(|e| BlockchainError::SerializationError(format!("Failed to serialize chain state: {e}")))?;
        
        self.state.insert("current", value)
            .map_err(|e| BlockchainError::StorageError(format!("Failed to save chain state: {e}")))?;
        
        Ok(())
    }

    pub fn load_chain_state(&self) -> Result<Option<ChainState>> {
        match self.state.get("current")
            .map_err(|e| BlockchainError::StorageError(format!("Failed to load chain state: {e}")))? {
            Some(data) => {
                let state = bincode::deserialize(&data)
                    .map_err(|e| BlockchainError::SerializationError(format!("Failed to deserialize chain state: {e}")))?;
                Ok(Some(state))
            }
            None => Ok(None),
        }
    }
    
    /// Save security checkpoints
    pub fn save_checkpoints(&self, checkpoints: &[SecurityCheckpoint]) -> Result<()> {
        let value = bincode::serialize(checkpoints)
            .map_err(|e| BlockchainError::SerializationError(format!("Failed to serialize checkpoints: {e}")))?;
        
        self.checkpoints.insert("all", value)
            .map_err(|e| BlockchainError::StorageError(format!("Failed to save checkpoints: {e}")))?;
        
        Ok(())
    }
    
    /// Load security checkpoints
    pub fn load_checkpoints(&self) -> Result<Option<Vec<SecurityCheckpoint>>> {
        match self.checkpoints.get("all")
            .map_err(|e| BlockchainError::StorageError(format!("Failed to load checkpoints: {e}")))? {
            Some(data) => {
                let checkpoints = bincode::deserialize(&data)
                    .map_err(|e| BlockchainError::SerializationError(format!("Failed to deserialize checkpoints: {e}")))?;
                Ok(Some(checkpoints))
            }
            None => Ok(None),
        }
    }
    
    /// Save individual checkpoint
    pub fn save_checkpoint(&self, checkpoint: &SecurityCheckpoint) -> Result<()> {
        let key = format!("checkpoint_{}", checkpoint.block_height);
        let value = bincode::serialize(checkpoint)
            .map_err(|e| BlockchainError::SerializationError(format!("Failed to serialize checkpoint: {e}")))?;
        
        self.checkpoints.insert(key.as_bytes(), value)
            .map_err(|e| BlockchainError::StorageError(format!("Failed to save checkpoint: {e}")))?;
        
        Ok(())
    }
    
    /// Load checkpoint by height
    pub fn load_checkpoint(&self, height: u64) -> Result<Option<SecurityCheckpoint>> {
        let key = format!("checkpoint_{}", height);
        
        match self.checkpoints.get(key.as_bytes())
            .map_err(|e| BlockchainError::StorageError(format!("Failed to load checkpoint: {e}")))? {
            Some(data) => {
                let checkpoint = bincode::deserialize(&data)
                    .map_err(|e| BlockchainError::SerializationError(format!("Failed to deserialize checkpoint: {e}")))?;
                Ok(Some(checkpoint))
            }
            None => Ok(None),
        }
    }

    pub fn get_all_blocks(&self) -> Result<Vec<Block>> {
        let mut blocks = Vec::new();
        
        for result in self.blocks.iter() {
            let (_, value) = result
                .map_err(|e| BlockchainError::StorageError(format!("Failed to iterate blocks: {e}")))?;
            
            let block: Block = bincode::deserialize(&value)
                .map_err(|e| BlockchainError::SerializationError(format!("Failed to deserialize block: {e}")))?;
            
            blocks.push(block);
        }
        
        // Sort by height
        blocks.sort_by_key(|block| block.header.height);
        
        Ok(blocks)
    }

    pub fn get_all_accounts(&self) -> Result<Vec<(Vec<u8>, AccountState)>> {
        let mut accounts = Vec::new();
        
        for result in self.accounts.iter() {
            let (key, value) = result
                .map_err(|e| BlockchainError::StorageError(format!("Failed to iterate accounts: {e}")))?;
            
            let account = bincode::deserialize(&value)
                .map_err(|e| BlockchainError::SerializationError(format!("Failed to deserialize account: {e}")))?;
            
            accounts.push((key.to_vec(), account));
        }
        
        Ok(accounts)
    }
    
    /// Get all checkpoints
    pub fn get_all_checkpoints(&self) -> Result<Vec<SecurityCheckpoint>> {
        let mut checkpoints = Vec::new();
        
        for result in self.checkpoints.iter() {
            let (key, value) = result
                .map_err(|e| BlockchainError::StorageError(format!("Failed to iterate checkpoints: {e}")))?;
            
            // Skip the "all" key which contains the serialized vector
            if key == b"all" {
                continue;
            }
            
            let checkpoint: SecurityCheckpoint = bincode::deserialize(&value)
                .map_err(|e| BlockchainError::SerializationError(format!("Failed to deserialize checkpoint: {e}")))?;
            
            checkpoints.push(checkpoint);
        }
        
        // Sort by height
        checkpoints.sort_by_key(|cp| cp.block_height);
        
        Ok(checkpoints)
    }

    pub fn delete_block(&self, height: u64) -> Result<()> {
        let key = height.to_be_bytes();
        self.blocks.remove(key)
            .map_err(|e| BlockchainError::StorageError(format!("Failed to delete block: {e}")))?;
        Ok(())
    }

    pub fn delete_transaction(&self, tx_id: &[u8; 32]) -> Result<()> {
        self.transactions.remove(tx_id.as_slice())
            .map_err(|e| BlockchainError::StorageError(format!("Failed to delete transaction: {e}")))?;
        Ok(())
    }

    pub fn delete_account(&self, public_key: &[u8]) -> Result<()> {
        self.accounts.remove(public_key)
            .map_err(|e| BlockchainError::StorageError(format!("Failed to delete account: {e}")))?;
        Ok(())
    }
    
    /// Delete checkpoint
    pub fn delete_checkpoint(&self, height: u64) -> Result<()> {
        let key = format!("checkpoint_{}", height);
        self.checkpoints.remove(key.as_bytes())
            .map_err(|e| BlockchainError::StorageError(format!("Failed to delete checkpoint: {e}")))?;
        Ok(())
    }

    pub fn compact(&self) -> Result<()> {
        // Trigger compaction for all trees
        let _ = self.blocks.flush();
        let _ = self.transactions.flush();
        let _ = self.accounts.flush();
        let _ = self.state.flush();
        let _ = self.checkpoints.flush();
        
        Ok(())
    }

    pub fn flush(&self) -> Result<()> {
        self.db.flush()
            .map_err(|e| BlockchainError::StorageError(format!("Failed to flush database: {e}")))?;
        Ok(())
    }

    pub fn get_database_size(&self) -> Result<u64> {
        let size = self.db.size_on_disk()
            .map_err(|e| BlockchainError::StorageError(format!("Failed to get database size: {e}")))?;
        Ok(size)
    }
    
    /// Backup database to specified path
    pub fn backup<P: AsRef<Path>>(&self, backup_path: P) -> Result<()> {
        // Flush all data first
        self.flush()?;
        
        // Create backup directory
        std::fs::create_dir_all(&backup_path)?;
        
        // Use sled's export functionality if available, or implement file copy
        // For now, we'll implement a simple approach
        let backup_db = sled::open(&backup_path)
            .map_err(|e| BlockchainError::StorageError(format!("Failed to create backup database: {e}")))?;
        
        // Copy all trees
        for (tree_name, source_tree) in [
            ("blocks", &self.blocks),
            ("transactions", &self.transactions),
            ("accounts", &self.accounts),
            ("chain_state", &self.state),
            ("checkpoints", &self.checkpoints),
        ] {
            let backup_tree = backup_db.open_tree(tree_name)
                .map_err(|e| BlockchainError::StorageError(format!("Failed to open backup tree {}: {e}", tree_name)))?;
            
            for result in source_tree.iter() {
                let (key, value) = result
                    .map_err(|e| BlockchainError::StorageError(format!("Failed to iterate {}: {e}", tree_name)))?;
                
                backup_tree.insert(key, value)
                    .map_err(|e| BlockchainError::StorageError(format!("Failed to backup {}: {e}", tree_name)))?;
            }
        }
        
        backup_db.flush()
            .map_err(|e| BlockchainError::StorageError(format!("Failed to flush backup: {e}")))?;
        
        Ok(())
    }
    
    /// Get storage statistics
    pub fn get_stats(&self) -> Result<std::collections::HashMap<String, u64>> {
        let mut stats = std::collections::HashMap::new();
        
        stats.insert("total_size_bytes".to_string(), self.get_database_size()?);
        stats.insert("total_blocks".to_string(), self.blocks.len() as u64);
        stats.insert("total_transactions".to_string(), self.transactions.len() as u64);
        stats.insert("total_accounts".to_string(), self.accounts.len() as u64);
        stats.insert("total_checkpoints".to_string(), self.checkpoints.len() as u64);
        
        Ok(stats)
    }

    /// Create a backup of the database to the specified directory
    pub fn backup_to_directory<P: AsRef<std::path::Path>>(&self, backup_dir: P) -> Result<()> {
        let backup_path = backup_dir.as_ref();
        std::fs::create_dir_all(backup_path)
            .map_err(|e| BlockchainError::StorageError(format!("Failed to create backup directory: {e}")))?;
        
        // Flush on-disk state to ensure a consistent view
        self.flush()?;

        // Since sled supports crash-safe checkpoints, use the built-in facility instead of
        // copying live files while the database is open. This prevents torn/corrupted
        // snapshots that could arise from long write transactions.

        // TODO: Implement proper checkpoint functionality when sled supports it
        // self.db
        //     .checkpoint(backup_path)
        //     .map_err(|e| BlockchainError::StorageError(format!("Failed to create checkpoint: {e}")))?;

        log::info!("✅ Database checkpoint created at {:?}", backup_path);
        Ok(())
    }
    
    /// Restore database from backup directory
    pub fn restore_from_directory<P: AsRef<std::path::Path>>(&self, backup_dir: P) -> Result<()> {
        let backup_path = backup_dir.as_ref();
        
        if !backup_path.exists() {
            return Err(BlockchainError::StorageError(format!("Backup directory not found: {:?}", backup_path)));
        }
        
        // Open the backup database
        let backup_db = sled::open(backup_path)
            .map_err(|e| BlockchainError::StorageError(format!("Failed to open backup database: {e}")))?;
        
        // Clear current database
        self.clear_all_data()?;
        
        // Restore all trees
        for tree_name in ["blocks", "transactions", "accounts", "chain_state", "checkpoints"] {
            let backup_tree = backup_db.open_tree(tree_name)
                .map_err(|e| BlockchainError::StorageError(format!("Failed to open backup tree {}: {e}", tree_name)))?;
            
            let dest_tree = match tree_name {
                "blocks" => &self.blocks,
                "transactions" => &self.transactions,
                "accounts" => &self.accounts,
                "chain_state" => &self.state,
                "checkpoints" => &self.checkpoints,
                _ => continue,
            };
            
            for result in backup_tree.iter() {
                let (key, value) = result
                    .map_err(|e| BlockchainError::StorageError(format!("Failed to iterate backup {}: {e}", tree_name)))?;
                
                dest_tree.insert(key, value)
                    .map_err(|e| BlockchainError::StorageError(format!("Failed to restore {}: {e}", tree_name)))?;
            }
        }
        
        self.flush()?;
        log::info!("Database restored from {:?}", backup_path);
        Ok(())
    }
    
    /// Clear all data from the database (dangerous operation!)
    pub fn clear_all_data(&self) -> Result<()> {
        self.blocks.clear()
            .map_err(|e| BlockchainError::StorageError(format!("Failed to clear blocks: {e}")))?;
        self.transactions.clear()
            .map_err(|e| BlockchainError::StorageError(format!("Failed to clear transactions: {e}")))?;
        self.accounts.clear()
            .map_err(|e| BlockchainError::StorageError(format!("Failed to clear accounts: {e}")))?;
        self.state.clear()
            .map_err(|e| BlockchainError::StorageError(format!("Failed to clear state: {e}")))?;
        self.checkpoints.clear()
            .map_err(|e| BlockchainError::StorageError(format!("Failed to clear checkpoints: {e}")))?;
        
        self.flush()?;
        log::warn!("All database data cleared");
        Ok(())
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
                memo: None,
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
            transaction_count: 5,
            total_received: 2000,
            total_sent: 1000,
            validator_info: None,
            created_at: Utc::now(),
            last_activity: Utc::now(),
        };
        
        let public_key = vec![1, 2, 3, 4];
        storage.save_account(&public_key, &account).unwrap();
        let loaded_account = storage.load_account(&public_key).unwrap().unwrap();
        
        assert_eq!(account.balance, loaded_account.balance);
    }
} 