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
    println!("🌐 Network Mode: P2P Mining Enabled");
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
    let wallet_address = hex::encode(&numi_core::crypto::blake3_hash(&wallet.public_key));
    println!("💰 Your Wallet Address: {}", wallet_address);
    println!("📁 Wallet File: {}", wallet_path.display());
    println!("📂 Data Directory: {}", data_dir.display());
    println!();
    
    // Create configuration with networking enabled
    let mut config = Config::development();
    config.storage.data_directory = data_dir.clone();
    config.mining.enabled = true;
    config.mining.thread_count = num_cpus::get();
    config.mining.wallet_path = wallet_path.clone();
    
    // Enable networking for live deployment
    config.network.enabled = true;
    config.network.listen_port = 8333;
    config.network.max_peers = 25;
    config.network.enable_mdns = true;
    
    // Bootstrap nodes - update these with your seed nodes for live deployment
    config.network.bootstrap_nodes = vec![
        // Add your seed node IPs here when deploying
        "/ip4/127.0.0.1/tcp/8333".to_string(),
        // "/ip4/YOUR_SEED_NODE_1/tcp/8333".to_string(),
        // "/ip4/YOUR_SEED_NODE_2/tcp/8333".to_string(),
    ];
    
    // Enable RPC for monitoring (optional)
    config.rpc.enabled = true;
    config.rpc.port = 8080;
    config.rpc.bind_address = "127.0.0.1".to_string(); // Only local access for security
    
    // Initialize storage and blockchain
    println!("🔧 Initializing blockchain...");
    let storage = Arc::new(BlockchainStorage::new(&config.storage.data_directory)?);
    
    let blockchain = match NumiBlockchain::load_from_storage(&*storage).await {
        Ok(chain) => {
            println!("📦 Loaded existing blockchain (height: {})", chain.get_current_height());
            chain
        }
        Err(_) => {
            println!("🆕 Creating new blockchain...");
            let chain = NumiBlockchain::new_with_keypair(Some(wallet.clone()))?;
            chain.save_to_storage(&*storage)?;
            println!("✅ Blockchain initialized with genesis block");
            chain
        }
    };
    
    let blockchain = Arc::new(RwLock::new(blockchain));
    let initial_balance = blockchain.read().get_balance(&numi_core::crypto::blake3_hash(&wallet.public_key));
    println!("💎 Current Balance: {} NUMI", initial_balance as f64 / 100_000_000.0);
    println!();
    
    // Start networking
    println!("🌐 Starting P2P network...");
    let mut network = NetworkManager::new()?;
    let network_addr = format!("/ip4/{}/tcp/{}", config.network.listen_address, config.network.listen_port);
    network.start(&network_addr).await?;
    println!("✅ Network started on {}", network_addr);
    
    let network_handle = network.create_handle();
    
    // Spawn network event loop
    tokio::spawn(async move {
        network.run_event_loop().await;
    });
    
    // Give network time to initialize
    time::sleep(Duration::from_secs(2)).await;
    
    // Start mining service
    println!("⛏️  Starting mining...");
    println!("🔥 Using {} CPU threads", config.mining.thread_count);
    println!("⏱️  Target block time: 10 seconds");
    println!("🌐 P2P networking enabled");
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
    println!("🌐 Connecting to other miners...");
    println!("{}", "=".repeat(70));
    
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                println!("\n🛑 Stopping miner...");
                
                // Save blockchain state
                if let Err(e) = blockchain.read().save_to_storage(&*storage) {
                    println!("⚠️  Warning: Failed to save blockchain state: {}", e);
                }
                
                println!("💾 Blockchain state saved");
                println!("👋 Thanks for mining! Your wallet and data are saved.");
                println!("📍 Wallet: {}", wallet_path.display());
                println!("📂 Data: {}", config.storage.data_directory.display());
                break;
            }
            _ = status_interval.tick() => {
                let state = blockchain.read().get_chain_state();
                let current_balance = blockchain.read().get_balance(&numi_core::crypto::blake3_hash(&wallet.public_key));
                let peer_count = network_handle.get_peer_count().await;
                
                // Check if we mined new blocks
                if current_balance > last_balance {
                    let earned = current_balance - last_balance;
                    blocks_mined += 1;
                    println!("🎉 NEW BLOCK MINED! Earned {} NUMI", earned as f64 / 100_000_000.0);
                    last_balance = current_balance;
                }
                
                println!("📊 Height: {} | Difficulty: {} | Balance: {} NUMI | Blocks Mined: {} | Peers: {}",
                    state.total_blocks,
                    state.current_difficulty,
                    current_balance as f64 / 100_000_000.0,
                    blocks_mined,
                    peer_count
                );
                
                // Connection status
                if peer_count == 0 {
                    println!("⚠️  No peers connected - mining solo");
                } else {
                    println!("✅ Connected to {} other miners", peer_count);
                }
                
                // Save periodically
                if let Err(e) = blockchain.read().save_to_storage(&*storage) {
                    println!("⚠️  Warning: Failed to save blockchain state: {}", e);
                }
            }
        }
    }
    
    // Clean shutdown
    mining_handle.abort();
    Ok(())
} 