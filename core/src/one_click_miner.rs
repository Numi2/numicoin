use std::path::PathBuf;
use std::time::Duration;
use tokio::time;
use numi_core::{
    blockchain::NumiBlockchain,
    storage::BlockchainStorage,
    mining_service::MiningService,
    network::NetworkManager,
    crypto::Dilithium3Keypair,
    config::Config,
    Result,
};
use parking_lot::RwLock;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {   
    // Initialize simple logging
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .init();

    println!("🚀 NumiCoin One-Click Miner Starting...");
    println!("========================================");
    println!();
    
    // Get the directory where the executable is located
    let exe_dir = std::env::current_exe()
        .unwrap_or_else(|_| PathBuf::from("."))
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .to_path_buf();
    
    // Create data directory next to executable
    let data_dir = exe_dir.join("numi-data");
    std::fs::create_dir_all(&data_dir)?;
    
    // Generate or load wallet
    let wallet_path = exe_dir.join("my-wallet.json");
    let wallet = if wallet_path.exists() {
        println!("📝 Loading existing wallet...");
        Dilithium3Keypair::load_from_file(&wallet_path)?
    } else {
        println!("🔑 Creating new wallet...");
        let new_wallet = Dilithium3Keypair::new()?;
        new_wallet.save_to_file(&wallet_path)?;
        println!("✅ Wallet saved to: {}", wallet_path.display());
        new_wallet
    };
    
    // Display wallet info
    let wallet_address = hex::encode(numi_core::crypto::blake3_hash(&wallet.public_key));
    println!("💰 Your Wallet Address: {wallet_address}");
    println!("📁 Wallet File: {}", wallet_path.display());
    println!("📂 Data Directory: {}", data_dir.display());
    println!();
    
    // Create simple configuration
    let mut config = Config::development();
    config.storage.data_directory = data_dir.clone();
    config.mining.enabled = true;
    config.mining.thread_count = num_cpus::get();
    config.mining.wallet_path = wallet_path.clone();
    config.network.enabled = false; // Start in offline mode for simplicity
    config.rpc.enabled = false; // Disable RPC for simplicity
    
    // Initialize storage and blockchain
    println!("🔧 Initializing blockchain...");
    let storage = Arc::new(BlockchainStorage::new(&config.storage.data_directory)?);
    
    let blockchain = match NumiBlockchain::load_from_storage_with_config(&storage, Some(config.consensus.clone())).await {
        Ok(chain) => {
            println!("📦 Loaded existing blockchain (height: {})", chain.get_current_height());
            chain
        }
        Err(_) => {
            println!("🆕 Creating new blockchain...");
            let chain = NumiBlockchain::new_with_config(Some(config.consensus.clone()), Some(wallet.clone()))?;
            chain.save_to_storage(&storage)?;
            println!("✅ Blockchain initialized with genesis block");
            chain
        }
    };
    
    let blockchain = Arc::new(RwLock::new(blockchain));
    let wallet_address = blockchain.read().get_address_from_public_key(&wallet.public_key);
    let initial_balance = blockchain.read().get_balance(&wallet_address);
    println!("💎 Current Balance: {} NUMI", initial_balance as f64 / 100_000_000.0);
    println!();
    
    // Create a dummy network handle for mining service
    let network = NetworkManager::new(blockchain.clone())?;
    let network_handle = network.create_handle();
    
    // Start mining service
    println!("⛏️  Starting mining...");
    println!("🔥 Using {} CPU threads", config.mining.thread_count);
    println!("⏱️  Target block time: 10 seconds");
    println!();
    
    let mining_service = MiningService::new(
        blockchain.clone(),
        network_handle.clone(),
        config.mining.clone(),
        data_dir,
        Duration::from_secs(10), // Fast 10-second blocks for better user experience
    );
    
    // Start mining in background
    let mining_handle = tokio::spawn(async move {
        mining_service.start_mining_loop().await;
    });
    
    // Status display loop
    let mut status_interval = time::interval(Duration::from_secs(15));
    let mut last_balance = initial_balance;
    let mut blocks_mined = 0u64;
    
    println!("🎯 Mining started! Status updates every 15 seconds...");
    println!("💡 Press Ctrl+C to stop mining and exit");
    println!("{}", "=".repeat(60));
    
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                println!("\n🛑 Stopping miner...");
                
                // Save blockchain state
                if let Err(e) = blockchain.read().save_to_storage(&storage) {
                    println!("⚠️  Warning: Failed to save blockchain state: {e}");
                }
                
                println!("💾 Blockchain state saved");
                println!("👋 Thanks for mining! Your wallet and data are saved.");
                println!("📍 Wallet: {}", wallet_path.display());
                println!("📂 Data: {}", config.storage.data_directory.display());
                break;
            }
            _ = status_interval.tick() => {
                let state = blockchain.read().get_chain_state();
                let current_balance = blockchain.read().get_balance(&wallet_address);
                
                // Check if we mined new blocks
                if current_balance > last_balance {
                    let earned = current_balance - last_balance;
                    blocks_mined += 1;
                    println!("🎉 NEW BLOCK MINED! Earned {} NUMI", earned as f64 / 100_000_000.0);
                    last_balance = current_balance;
                }
                
                println!("📊 Height: {} | Difficulty: {} | Balance: {} NUMI | Blocks Mined: {}",
                    state.total_blocks,
                    state.current_difficulty,
                    current_balance as f64 / 100_000_000.0,
                    blocks_mined
                );
                
                // Save periodically
                if let Err(e) = blockchain.read().save_to_storage(&storage) {
                    println!("⚠️  Warning: Failed to save blockchain state: {e}");
                }
            }
        }
    }
    
    // Clean shutdown
    mining_handle.abort();
    Ok(())
} 