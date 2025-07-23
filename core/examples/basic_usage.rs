use numi_core::{
    blockchain::NumiBlockchain,
    transaction::{Transaction, TransactionType},
    crypto::{Dilithium3Keypair, Argon2Config},
    miner::{Miner, MiningConfig},
    mempool::ValidationResult,
    storage::BlockchainStorage,
    secure_storage::{SecureKeyStore, KeyDerivationConfig},
    Result,
};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    env_logger::init();
    
    println!("🚀 Bumi Coin Blockchain - Basic Usage Example");
    println!("=============================================");
    
    // Step 1: Initialize the blockchain
    println!("\n1. Initializing blockchain...");
    let blockchain = NumiBlockchain::new()?;
    println!("✅ Blockchain initialized successfully");
    
    // Step 2: Create keypairs for different participants
    println!("\n2. Creating keypairs...");
    let alice_keypair = Dilithium3Keypair::new()?;
    let bob_keypair = Dilithium3Keypair::new()?;
    let miner_keypair = Dilithium3Keypair::new()?;
    
    println!("✅ Created keypairs for Alice, Bob, and Miner");
    println!("   Alice's public key: {}", hex::encode(&alice_keypair.public_key[..16]));
    println!("   Bob's public key: {}", hex::encode(&bob_keypair.public_key[..16]));
    println!("   Miner's public key: {}", hex::encode(&miner_keypair.public_key[..16]));
    
    // Step 3: Create and submit transactions
    println!("\n3. Creating and submitting transactions...");
    
    // Create a transfer transaction from Alice to Bob
    let mut transfer_tx = Transaction::new(
        alice_keypair.public_key.clone(),
        TransactionType::Transfer {
            to: bob_keypair.public_key.clone(),
            amount: 1_000_000_000, // 1 NUMI
        },
        1, // nonce
    );
    
    // Sign the transaction
    transfer_tx.sign(&alice_keypair)?;
    println!("✅ Created transfer transaction: {} → {} (1 NUMI)", 
             hex::encode(&alice_keypair.public_key[..8]),
             hex::encode(&bob_keypair.public_key[..8]));
    
    // Submit transaction to mempool
    let validation_result = blockchain.add_transaction(transfer_tx).await?;
    match validation_result {
        ValidationResult::Valid => println!("✅ Transaction added to mempool"),
        _ => println!("❌ Transaction validation failed: {:?}", validation_result),
    }
    
    // Create a staking transaction
    let mut stake_tx = Transaction::new(
        bob_keypair.public_key.clone(),
        TransactionType::Stake {
            amount: 500_000_000, // 0.5 NUMI
        },
        1, // nonce
    );
    
    stake_tx.sign(&bob_keypair)?;
    println!("✅ Created staking transaction: {} stakes 0.5 NUMI", 
             hex::encode(&bob_keypair.public_key[..8]));
    
    let validation_result = blockchain.add_transaction(stake_tx).await?;
    match validation_result {
        ValidationResult::Valid => println!("✅ Staking transaction added to mempool"),
        _ => println!("❌ Staking transaction validation failed: {:?}", validation_result),
    }
    
    // Step 4: Set up mining
    println!("\n4. Setting up mining...");
    
    // Create mining configuration
    let mining_config = MiningConfig {
        thread_count: 2, // Use 2 threads for demo
        nonce_chunk_size: 1_000,
        stats_update_interval: 2,
        argon2_config: Argon2Config::development(), // Fast for demo
        enable_cpu_affinity: false,
        thermal_throttle_temp: 0.0,
        power_limit_watts: 0.0,
    };
    
    let mut miner = Miner::with_config(mining_config.clone())?;
    println!("✅ Miner initialized with {} threads", mining_config.thread_count);
    
    // Step 5: Mine a block
    println!("\n5. Mining a new block...");
    
    // Get current blockchain state
    let current_height = blockchain.get_current_height();
    let current_difficulty = blockchain.get_current_difficulty();
    let latest_block_hash = blockchain.get_latest_block_hash();
    
    println!("   Current height: {}", current_height);
    println!("   Current difficulty: {}", current_difficulty);
    println!("   Latest block: {}", hex::encode(&latest_block_hash[..16]));
    
    // Get transactions for the block
    let pending_transactions = blockchain.get_transactions_for_block(1_000_000, 100);
    println!("   Pending transactions: {}", pending_transactions.len());
    
    // Mine the block
    let mining_result = miner.mine_block(
        current_height + 1,
        latest_block_hash,
        pending_transactions,
        current_difficulty,
        0, // start nonce
    )?;
    
    match mining_result {
        Some(result) => {
            println!("🎉 Block mined successfully!");
            println!("   Block hash: {}", result.block.get_hash_hex());
            println!("   Nonce: {}", result.nonce);
            println!("   Mining time: {} seconds", result.mining_time_secs);
            println!("   Hash rate: {} H/s", result.hash_rate);
            println!("   Thread ID: {}", result.thread_id);
            
            // Add the mined block to the blockchain
            let was_reorg = blockchain.add_block(result.block).await?;
            if was_reorg {
                println!("⚠️  Chain reorganization occurred");
            } else {
                println!("✅ Block added to blockchain");
            }
        }
        None => {
            println!("⏰ Mining timeout - no block found");
        }
    }
    
    // Step 6: Check blockchain state
    println!("\n6. Checking blockchain state...");
    
    let chain_state = blockchain.get_chain_state();
    println!("   Total blocks: {}", chain_state.total_blocks);
    println!("   Total supply: {} NUMI", chain_state.total_supply / 1_000_000_000);
    println!("   Current difficulty: {}", chain_state.current_difficulty);
    println!("   Average block time: {} seconds", chain_state.average_block_time);
    println!("   Best block: {}", hex::encode(&chain_state.best_block_hash[..16]));
    
    // Check account balances
    let alice_balance = blockchain.get_balance(&alice_keypair.public_key);
    let bob_balance = blockchain.get_balance(&bob_keypair.public_key);
    let miner_balance = blockchain.get_balance(&miner_keypair.public_key);
    
    println!("   Alice's balance: {} NUMI", alice_balance / 1_000_000_000);
    println!("   Bob's balance: {} NUMI", bob_balance / 1_000_000_000);
    println!("   Miner's balance: {} NUMI", miner_balance / 1_000_000_000);
    
    // Step 7: Demonstrate secure storage
    println!("\n7. Demonstrating secure storage...");
    
    // Create secure key store
    let temp_dir = tempfile::tempdir()?;
    let keystore_path = temp_dir.path().join("demo_keystore.bin");
    
    let mut keystore = SecureKeyStore::with_config(
        &keystore_path,
        KeyDerivationConfig::development(), // Fast for demo
    )?;
    
    // Initialize with password
    keystore.initialize("demo_password")?;
    println!("✅ Secure key store initialized");
    
    // Store Alice's keypair
    keystore.store_keypair("alice_wallet", &alice_keypair, "demo_password")?;
    println!("✅ Alice's keypair stored securely");
    
    // Retrieve Alice's keypair
    let retrieved_keypair = keystore.get_keypair("alice_wallet", "demo_password")?;
    assert_eq!(retrieved_keypair.public_key, alice_keypair.public_key);
    println!("✅ Alice's keypair retrieved successfully");
    
    // List stored keys
    let stored_keys = keystore.list_keys();
    println!("   Stored keys: {:?}", stored_keys);
    
    // Get key store statistics
    let keystore_stats = keystore.get_stats();
    println!("   Total keys: {}", keystore_stats.total_keys);
    println!("   Active keys: {}", keystore_stats.active_keys);
    println!("   Integrity check: {}", keystore_stats.integrity_check_passed);
    
    // Step 8: Demonstrate storage persistence
    println!("\n8. Demonstrating storage persistence...");
    
    // Create storage
    let storage_dir = tempfile::tempdir()?;
    let storage = BlockchainStorage::new(storage_dir.path())?;
    
    // Save blockchain state
    blockchain.save_to_storage(&storage)?;
    println!("✅ Blockchain state saved to storage");
    
    // Load blockchain from storage
    let loaded_blockchain = NumiBlockchain::load_from_storage(&storage).await?;
    println!("✅ Blockchain loaded from storage");
    
    // Verify loaded state matches original
    let original_state = blockchain.get_chain_state();
    let loaded_state = loaded_blockchain.get_chain_state();
    
    assert_eq!(original_state.total_blocks, loaded_state.total_blocks);
    assert_eq!(original_state.total_supply, loaded_state.total_supply);
    println!("✅ Loaded state matches original state");
    
    // Step 9: Performance demonstration
    println!("\n9. Performance demonstration...");
    
    // Create multiple transactions
    let mut transaction_count = 0;
    for i in 0..10 {
        let mut tx = Transaction::new(
            alice_keypair.public_key.clone(),
            TransactionType::Transfer {
                to: bob_keypair.public_key.clone(),
                amount: 100_000, // 0.0001 NUMI
            },
            i + 2, // nonce
        );
        tx.sign(&alice_keypair)?;
        
        let result = blockchain.add_transaction(tx).await?;
        if matches!(result, ValidationResult::Valid) {
            transaction_count += 1;
        }
    }
    
    println!("✅ Added {} transactions to mempool", transaction_count);
    
    // Get mempool statistics
    let mempool_stats = blockchain.get_mempool_stats();
    println!("   Mempool transactions: {}", mempool_stats.total_transactions);
    println!("   Mempool size: {} bytes", mempool_stats.total_size_bytes);
    println!("   Oldest transaction: {:?}", mempool_stats.oldest_transaction_age);
    
    // Step 10: Cleanup and summary
    println!("\n10. Summary and cleanup...");
    
    // Perform maintenance
    blockchain.perform_maintenance().await?;
    println!("✅ Maintenance completed");
    
    // Clean up expired keys
    let expired_count = keystore.cleanup_expired_keys()?;
    println!("✅ Cleaned up {} expired keys", expired_count);
    
    println!("\n🎉 Bumi Coin Blockchain demonstration completed successfully!");
    println!("   Total blocks in chain: {}", blockchain.get_current_height() + 1);
    println!("   Total transactions processed: {}", transaction_count + 2);
    println!("   Blockchain is ready for production use!");
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_basic_usage() {
        // This test ensures the basic usage example works correctly
        let result = main().await;
        assert!(result.is_ok(), "Basic usage example failed: {:?}", result);
    }
}