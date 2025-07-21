use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use crate::block::{Block, BlockHash};
use crate::transaction::{Transaction, TransactionType, TransactionId};
use crate::crypto::{Dilithium3Keypair, generate_difficulty_target, verify_pow};
use crate::error::BlockchainError;
use crate::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainState {
    pub total_blocks: u64,
    pub total_supply: u64,
    pub current_difficulty: u32,
    pub average_block_time: u64,
    pub last_block_time: DateTime<Utc>,
    pub active_miners: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountState {
    pub balance: u64,
    pub nonce: u64,
    pub staked_amount: u64,
    pub last_stake_time: DateTime<Utc>,
}

pub struct NumiBlockchain {
    chain: Arc<RwLock<Vec<Block>>>,
    pending_transactions: Arc<RwLock<Vec<Transaction>>>,
    accounts: Arc<RwLock<HashMap<Vec<u8>, AccountState>>>,
    state: Arc<RwLock<ChainState>>,
    miner_keypair: Dilithium3Keypair,
}

impl NumiBlockchain {
    pub fn new() -> Result<Self> {
        let miner_keypair = Dilithium3Keypair::new()?;
        let mut blockchain = Self {
            chain: Arc::new(RwLock::new(Vec::new())),
            pending_transactions: Arc::new(RwLock::new(Vec::new())),
            accounts: Arc::new(RwLock::new(HashMap::new())),
            state: Arc::new(RwLock::new(ChainState {
                total_blocks: 0,
                total_supply: 0,
                current_difficulty: 1,
                average_block_time: 30,
                last_block_time: Utc::now(),
                active_miners: 0,
            })),
            miner_keypair,
        };
        
        blockchain.create_genesis_block()?;
        Ok(blockchain)
    }
    
    pub fn load_from_storage(storage: &crate::storage::BlockchainStorage) -> Result<Self> {
        let miner_keypair = Dilithium3Keypair::new()?;
        let mut blockchain = Self {
            chain: Arc::new(RwLock::new(Vec::new())),
            pending_transactions: Arc::new(RwLock::new(Vec::new())),
            accounts: Arc::new(RwLock::new(HashMap::new())),
            state: Arc::new(RwLock::new(ChainState {
                total_blocks: 0,
                total_supply: 0,
                current_difficulty: 1,
                average_block_time: 30,
                last_block_time: Utc::now(),
                active_miners: 0,
            })),
            miner_keypair,
        };
        
        // Try to load existing chain state
        if let Some(saved_state) = storage.load_chain_state()? {
            *blockchain.state.write().unwrap() = saved_state;
        }
        
        // Load all blocks
        let blocks = storage.get_all_blocks()?;
        if blocks.is_empty() {
            // No blocks found, create genesis
            blockchain.create_genesis_block()?;
        } else {
            // Load existing blocks
            for block in blocks {
                blockchain.add_block_without_validation(block)?;
            }
        }
        
        // Load all accounts
        let accounts = storage.get_all_accounts()?;
        for (pubkey, account_state) in accounts {
            blockchain.accounts.write().unwrap().insert(pubkey, account_state);
        }
        
        Ok(blockchain)
    }
    
    fn create_genesis_block(&mut self) -> Result<()> {
        let mut genesis_block = Block::new(
            0,
            [0u8; 32],
            vec![],
            1,
            self.miner_keypair.public_key.clone(),
        );
        
        // Sign the genesis block
        genesis_block.sign(&self.miner_keypair)?;
        
        self.add_block(genesis_block)?;
        Ok(())
    }
    
    pub fn add_block(&mut self, block: Block) -> Result<()> {
        // Validate the block
        let previous_block = if block.header.height > 0 {
            Some(self.get_block_by_height(block.header.height - 1)?)
        } else {
            None
        };
        
        block.validate(previous_block.as_ref())?;
        
        // Verify proof of work (skip for genesis block)
        if !block.is_genesis() {
            self.verify_proof_of_work(&block)?;
        }
        
        // Apply transactions to state
        self.apply_transactions(&block.transactions)?;
        
        // Add block to chain
        {
            let mut chain = self.chain.write().unwrap();
            chain.push(block.clone());
        }
        
        // Update chain state
        self.update_chain_state(&block)?;
        
        // Remove processed transactions from pending
        {
            let mut pending = self.pending_transactions.write().unwrap();
            let processed_ids: Vec<TransactionId> = block.transactions.iter()
                .map(|tx| tx.id)
                .collect();
            pending.retain(|tx| !processed_ids.contains(&tx.id));
        }
        
        Ok(())
    }
    
    fn add_block_without_validation(&mut self, block: Block) -> Result<()> {
        // Add block to chain without validation (for loading from storage)
        {
            let mut chain = self.chain.write().unwrap();
            chain.push(block.clone());
        }
        
        // Update chain state
        self.update_chain_state(&block)?;
        
        Ok(())
    }
    
    pub fn save_to_storage(&self, storage: &crate::storage::BlockchainStorage) -> Result<()> {
        // Save all blocks
        let chain = self.chain.read().unwrap();
        for block in chain.iter() {
            storage.save_block(block)?;
        }
        
        // Save all accounts
        let accounts = self.accounts.read().unwrap();
        for (pubkey, account_state) in accounts.iter() {
            storage.save_account(pubkey, account_state)?;
        }
        
        // Save chain state
        let state = self.state.read().unwrap();
        storage.save_chain_state(&state)?;
        
        Ok(())
    }
    
    pub fn add_transaction(&self, transaction: Transaction) -> Result<()> {
        // Validate transaction
        let account_state = self.get_account_state(&transaction.from)?;
        transaction.validate(account_state.balance, account_state.nonce)?;
        
        // Add to pending transactions
        {
            let mut pending = self.pending_transactions.write().unwrap();
            pending.push(transaction);
        }
        
        Ok(())
    }
    
    pub fn mine_block(&mut self, miner_public_key: Vec<u8>) -> Result<Block> {
        let difficulty_target = generate_difficulty_target(self.get_current_difficulty());
        let mut nonce = 0u64;
        
        // Get pending transactions
        let pending_transactions = {
            let pending = self.pending_transactions.read().unwrap();
            pending.clone()
        };
        
        // Create mining reward transaction
        let reward_tx = Transaction::new(
            miner_public_key.clone(),
            TransactionType::MiningReward {
                block_height: self.get_current_height() + 1,
                amount: self.get_mining_reward(),
            },
            0, // Nonce doesn't matter for rewards
        );
        
        let mut block_transactions = vec![reward_tx];
        block_transactions.extend(pending_transactions);
        
        let mut block = Block::new(
            self.get_current_height() + 1,
            self.get_latest_block_hash(),
            block_transactions,
            self.get_current_difficulty(),
            miner_public_key,
        );
        
        // Mine the block
        loop {
            block.header.nonce = nonce;
            
            let header_blob = block.serialize_header_for_hashing();
            if verify_pow(&header_blob, nonce, &difficulty_target)? {
                break;
            }
            
            nonce += 1;
            
            // Prevent infinite loop in testing
            if nonce > 1_000_000 {
                return Err(BlockchainError::MiningError("Could not find valid nonce".to_string()));
            }
        }
        
        // Sign the block
        block.sign(&self.miner_keypair)?;
        
        Ok(block)
    }
    
    fn verify_proof_of_work(&self, block: &Block) -> Result<()> {
        let difficulty_target = generate_difficulty_target(block.header.difficulty);
        let header_blob = block.serialize_header_for_hashing();
        
        if !verify_pow(&header_blob, block.header.nonce, &difficulty_target)? {
            return Err(BlockchainError::InvalidBlock("Proof of work verification failed".to_string()));
        }
        
        Ok(())
    }
    
    fn apply_transactions(&mut self, transactions: &[Transaction]) -> Result<()> {
        let mut accounts = self.accounts.write().unwrap();
        
        for transaction in transactions {
            match &transaction.transaction_type {
                TransactionType::Transfer { to, amount } => {
                    // Deduct from sender
                    let sender_state = accounts.entry(transaction.from.clone())
                        .or_insert_with(|| AccountState {
                            balance: 0,
                            nonce: 0,
                            staked_amount: 0,
                            last_stake_time: Utc::now(),
                        });
                    
                    if sender_state.balance < *amount {
                        return Err(BlockchainError::InsufficientBalance(
                            format!("Insufficient balance for transfer")
                        ));
                    }
                    
                    sender_state.balance -= amount;
                    sender_state.nonce += 1;
                    
                    // Add to recipient
                    let recipient_state = accounts.entry(to.clone())
                        .or_insert_with(|| AccountState {
                            balance: 0,
                            nonce: 0,
                            staked_amount: 0,
                            last_stake_time: Utc::now(),
                        });
                    
                    recipient_state.balance += amount;
                }
                
                TransactionType::Stake { amount } => {
                    let account_state = accounts.entry(transaction.from.clone())
                        .or_insert_with(|| AccountState {
                            balance: 0,
                            nonce: 0,
                            staked_amount: 0,
                            last_stake_time: Utc::now(),
                        });
                    
                    if account_state.balance < *amount {
                        return Err(BlockchainError::InsufficientBalance(
                            format!("Insufficient balance for staking")
                        ));
                    }
                    
                    account_state.balance -= amount;
                    account_state.staked_amount += amount;
                    account_state.last_stake_time = Utc::now();
                    account_state.nonce += 1;
                }
                
                TransactionType::Unstake { amount } => {
                    let account_state = accounts.entry(transaction.from.clone())
                        .or_insert_with(|| AccountState {
                            balance: 0,
                            nonce: 0,
                            staked_amount: 0,
                            last_stake_time: Utc::now(),
                        });
                    
                    if account_state.staked_amount < *amount {
                        return Err(BlockchainError::InsufficientBalance(
                            format!("Insufficient staked amount")
                        ));
                    }
                    
                    account_state.staked_amount -= amount;
                    account_state.balance += amount;
                    account_state.nonce += 1;
                }
                
                TransactionType::MiningReward { amount, .. } => {
                    let account_state = accounts.entry(transaction.from.clone())
                        .or_insert_with(|| AccountState {
                            balance: 0,
                            nonce: 0,
                            staked_amount: 0,
                            last_stake_time: Utc::now(),
                        });
                    
                    account_state.balance += amount;
                    
                    // Update total supply
                    let mut state = self.state.write().unwrap();
                    state.total_supply += amount;
                }
                
                TransactionType::Governance { .. } => {
                    let account_state = accounts.entry(transaction.from.clone())
                        .or_insert_with(|| AccountState {
                            balance: 0,
                            nonce: 0,
                            staked_amount: 0,
                            last_stake_time: Utc::now(),
                        });
                    
                    account_state.nonce += 1;
                }
            }
        }
        
        Ok(())
    }
    
    fn update_chain_state(&mut self, block: &Block) -> Result<()> {
        let mut state = self.state.write().unwrap();
        
        state.total_blocks += 1;
        state.last_block_time = block.header.timestamp;
        
        // Calculate average block time
        if state.total_blocks > 1 {
            let time_diff = (block.header.timestamp - state.last_block_time).num_seconds() as u64;
            state.average_block_time = (state.average_block_time + time_diff) / 2;
        }
        
        // Adjust difficulty (simplified)
        if state.average_block_time < 25 {
            state.current_difficulty += 1;
        } else if state.average_block_time > 35 {
            state.current_difficulty = state.current_difficulty.saturating_sub(1);
        }
        
        Ok(())
    }
    
    pub fn get_chain(&self) -> Vec<Block> {
        self.chain.read().unwrap().clone()
    }
    
    pub fn get_block_by_height(&self, height: u64) -> Result<Block> {
        let chain = self.chain.read().unwrap();
        chain.get(height as usize)
            .cloned()
            .ok_or_else(|| BlockchainError::BlockNotFound(format!("Block at height {} not found", height)))
    }
    
    pub fn get_latest_block(&self) -> Block {
        let chain = self.chain.read().unwrap();
        chain.last().unwrap().clone()
    }
    
    pub fn get_latest_block_hash(&self) -> BlockHash {
        self.get_latest_block().calculate_hash()
    }
    
    pub fn get_current_height(&self) -> u64 {
        self.get_latest_block().header.height
    }
    
    pub fn get_current_difficulty(&self) -> u32 {
        self.state.read().unwrap().current_difficulty
    }
    
    pub fn get_mining_reward(&self) -> u64 {
        // Fixed mining reward: 0.005 NUMI per block
        5_000_000
    }
    
    pub fn get_account_state(&self, public_key: &[u8]) -> Result<AccountState> {
        let accounts = self.accounts.read().unwrap();
        accounts.get(public_key)
            .cloned()
            .ok_or_else(|| BlockchainError::BlockNotFound("Account not found".to_string()))
    }
    
    pub fn get_balance(&self, public_key: &[u8]) -> u64 {
        let accounts = self.accounts.read().unwrap();
        accounts.get(public_key)
            .map(|state| state.balance)
            .unwrap_or(0)
    }
    
    pub fn get_pending_transactions(&self) -> Vec<Transaction> {
        self.pending_transactions.read().unwrap().clone()
    }
    
    pub fn get_chain_state(&self) -> ChainState {
        self.state.read().unwrap().clone()
    }
    
    pub fn is_chain_valid(&self) -> bool {
        let chain = self.chain.read().unwrap();
        
        for i in 1..chain.len() {
            let current_block = &chain[i];
            let previous_block = &chain[i - 1];
            
            if current_block.validate(Some(previous_block)).is_err() {
                return false;
            }
        }
        
        true
    }
}

// Remove this duplicate implementation

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::Dilithium3Keypair;
    use crate::transaction::TransactionType;
    
    #[test]
    fn test_blockchain_creation() {
        let blockchain = NumiBlockchain::new().unwrap();
        assert_eq!(blockchain.get_current_height(), 0);
        assert!(blockchain.is_chain_valid());
    }
    
    #[test]
    fn test_mining_block() {
        let mut blockchain = NumiBlockchain::new().unwrap();
        let keypair = Dilithium3Keypair::new().unwrap();
        
        let block = blockchain.mine_block(keypair.public_key.clone()).unwrap();
        assert_eq!(block.header.height, 1);
        assert_eq!(block.get_transaction_count(), 1); // Only mining reward
    }
    
    #[test]
    fn test_transaction_processing() {
        let mut blockchain = NumiBlockchain::new().unwrap();
        let keypair = Dilithium3Keypair::new().unwrap();
        
        // Mine a block to get some balance
        let block = blockchain.mine_block(keypair.public_key.clone()).unwrap();
        blockchain.add_block(block).unwrap();
        
        // Create a transfer transaction
        let mut tx = Transaction::new(
            keypair.public_key.clone(),
            TransactionType::Transfer {
                to: vec![1, 2, 3, 4],
                amount: 1000,
            },
            1,
        );
        
        tx.sign(&keypair).unwrap();
        blockchain.add_transaction(tx).unwrap();
        
        assert_eq!(blockchain.get_pending_transactions().len(), 1);
    }
} 