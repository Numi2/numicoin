use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::signal;
use numi_core::RwLock;
use futures::channel::mpsc;
use crossbeam::channel::bounded;

use numi_core::{
    config::Config,
    blockchain::NumiBlockchain,
    storage::BlockchainStorage,
    rpc::{RpcServer, RateLimitConfig, AuthConfig, client::{show_status, show_balance, send_transaction}},
    crypto::{Dilithium3Keypair, derive_address_from_public_key},
    network::NetworkManager,
    mining_service::MiningService,
    miner::Miner,
    local_miner::LocalMiner,
    Result,
    BlockchainError,
};
use numi_core::stratum_server::StratumV2Server;

#[derive(Parser)]
#[command(name = "numi", about = "NumiCoin", version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
    
    #[arg(short, long, default_value = "numi.toml")]
    config: PathBuf,
    
    #[arg(short, long)]
    data_dir: Option<PathBuf>,
    
    #[arg(short, long)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the blockchain node with optional Stratum V2 mining server
    Node {
        #[arg(long, help = "Enable Stratum V2 mining server")]
        stratum: bool,
        #[arg(long, help = "Enable local CPU mining")]
        mining: bool,
        #[arg(long, help = "Number of CPU threads for local mining")]
        threads: Option<usize>,
    },
    
    /// Show blockchain and node status  
    Status,
    
    /// Wallet operations
    Wallet {
        #[command(subcommand)]
        wallet_cmd: WalletCommands,
    },
    
    /// Send a transaction
    Send {
        #[arg(long, help = "Path to wallet file")]
        wallet: PathBuf,
        #[arg(help = "Recipient address")]
        to: String,
        #[arg(help = "Amount to send (NUMI)")]
        amount: f64,
        #[arg(long, help = "Optional memo")]
        memo: Option<String>,
    },
    
    /// Show mining information (Stratum V2)
    Mining,
}

#[derive(Subcommand)]
enum WalletCommands {
    /// Create a new wallet
    Create {
        #[arg(long, default_value = "wallet.json", help = "Output file path")]
        output: PathBuf,
    },
    
    /// Check wallet balance
    Balance {
        #[arg(help = "Wallet address or file path")]
        address: String,
    },
}

// CLI subcommand handlers
async fn handle_wallet_create(output: PathBuf) -> Result<()> {
    println!("🔑 Creating new wallet...");
    let keypair = Dilithium3Keypair::new()?;
    
    // Ensure parent directory exists
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    
    keypair.save_to_file(&output)?;
    let address = derive_address_from_public_key(&keypair.public_key_bytes())?;
    
    println!("✅ Wallet created successfully!");
    println!("   File: {}", output.display());
    println!("   Address: {}", address);
    println!();
    println!("⚠️  IMPORTANT: Keep this wallet file secure!");
    println!("   Loss of this file means loss of funds.");
    
    Ok(())
}

async fn handle_wallet_balance(address_or_file: String, config: Config) -> Result<()> {
    // Check if it's a file path or an address
    if std::path::Path::new(&address_or_file).exists() {
        // It's a wallet file - load it and get the address
        let keypair = Dilithium3Keypair::load_from_file(&address_or_file)?;
        let address = derive_address_from_public_key(&keypair.public_key_bytes())?;
        show_balance_for_address(address, config).await
    } else {
        // It's an address directly
        show_balance_for_address(address_or_file, config).await
    }
}

async fn show_balance_for_address(address: String, config: Config) -> Result<()> {
    show_balance(config, address).await
}

async fn handle_send(wallet: PathBuf, to: String, amount: f64, memo: Option<String>, config: Config) -> Result<()> {
    send_transaction(config, wallet, to, amount, memo).await
}

async fn handle_mining_info(config: Config) -> Result<()> {

    println!("Numicoin uses Stratum V2 protocol for mining.");
  
    println!("🔗 Connection Details:");
    println!("   Server: {}:{}", 
        config.mining.stratum_bind_address, 
        config.mining.stratum_bind_port
    );
    println!("   Protocol: Stratum V2 with Noise XX encryption");
    println!("   Features: BLAKE3 validation, Dilithium3 signatures");
    println!();
    println!("📖 How to Connect:");
    println!("   1. Use any Stratum V2 compatible miner");
    println!("   2. Point it to the server address above");
    println!("   3. Mining server will distribute work automatically");
    println!("   4. Rewards go to the node operator's wallet");
    println!();
    if !config.mining.enabled {
        println!("⚠️  Mining server is currently DISABLED");
        println!("   Start the node with --mining to enable it:");
        println!("   numi-core node --mining");
    } else {
        println!("✅ Mining server is ENABLED and ready for connections");
    }
    println!();
    
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    
    // Setup logging
    if cli.verbose {
        std::env::set_var("RUST_LOG", "debug");
    } else {
        std::env::set_var("RUST_LOG", "info");
    }
    env_logger::init();
    
    // Load configuration
    let mut config = load_config(&cli).await?;
    
    if let Some(data_dir) = cli.data_dir {
        config.storage.data_directory = data_dir;
    }
    
    match cli.command {
        Commands::Node { stratum, mining, threads } => start_node(stratum, mining, threads, config).await?,
        Commands::Status => show_status(config).await?,
        Commands::Wallet { wallet_cmd } => {
            match wallet_cmd {
                WalletCommands::Create { output } => handle_wallet_create(output).await?,
                WalletCommands::Balance { address } => handle_wallet_balance(address, config).await?,
            }
        },
        Commands::Send { wallet, to, amount, memo } => handle_send(wallet, to, amount, memo, config).await?,
        Commands::Mining => handle_mining_info(config).await?,
    }
    
    Ok(())
}

async fn load_config(cli: &Cli) -> Result<Config> {
    if cli.config.exists() {
        Ok(Config::load_from_file(&cli.config)
            .map_err(|e| BlockchainError::IoError(e.to_string()))?)
    } else {
        // In a production environment, we should fail if the config is missing.
        // For this audit, we'll retain the dev-friendly auto-creation.
        log::warn!("Configuration file not found at {}. Creating a default development configuration.", cli.config.display());
        let config = Config::development();
        config.save_to_file(&cli.config)
            .map_err(|e| BlockchainError::IoError(e.to_string()))?;
        Ok(config)
    }
}

async fn start_node(stratum: bool, mining: bool, threads: Option<usize>, mut config: Config) -> Result<()> {
    config.mining.enabled = stratum;
    config.mining.local_mining_enabled = mining;
    if let Some(t) = threads {
        config.mining.cpu_threads = t;
    }
    
    if stratum {
        println!("🚀 Starting NumiCoin node with Stratum V2 mining server");
        println!("   Miners can connect to: {}:{}", 
            config.mining.stratum_bind_address, 
            config.mining.stratum_bind_port
        );
    } else {
        println!("🚀 Starting NumiCoin node (mining server disabled)");
    }
    
    if mining {
        println!("🖥️  Local CPU mining enabled ({} threads)", config.mining.cpu_threads);
    }

    log::info!("Starting NumiCoin node...");
    
    // Initialize storage and load blockchain
    let storage = Arc::new(BlockchainStorage::new(&config.storage.data_directory)?);
    let blockchain = Arc::new(RwLock::new(
        NumiBlockchain::load_from_storage(&storage, config.consensus.clone()).await?
    ));
    
    // Initialize network manager
    let (in_tx, _in_rx) = mpsc::unbounded();
    let (network_manager, network_handle) = NetworkManager::new(&config.network, in_tx)?;

    // Spawn the network manager in the background (event processing)
    tokio::spawn(async move {
        network_manager.run().await;
    });
    
    // Initialize miner
    let miner = Arc::new(RwLock::new(Miner::new(&config)?));
    
    // Create channel for Stratum connection tracking
    let (stratum_signal_tx, stratum_signal_rx) = bounded::<bool>(1);
    
    // Start LocalMiner if enabled
    let local_miner = if config.mining.local_mining_enabled {
        log::info!("🖥️  Starting local CPU miner with {} threads", config.mining.cpu_threads);
        Some(LocalMiner::spawn(
            blockchain.clone(),
            miner.clone(),
            config.mining.cpu_threads,
            config.consensus.clone(),
            stratum_signal_rx,
        ))
    } else {
        None
    };
    
    // Create rate limit config from RPC config
    let rate_limit_config = RateLimitConfig {
        requests_per_minute: config.rpc.rate_limit_requests_per_minute,
        burst_size: config.rpc.rate_limit_burst_size,
        cleanup_interval: std::time::Duration::from_secs(300),
        block_duration_tier1: 60,
        block_duration_tier2: 300,
        block_duration_tier3: 900,
        block_duration_tier4: 3600,
    };
    
    // Create auth config from security config
    let auth_config = AuthConfig {
        jwt_secret: config.security.jwt_secret.clone(),
        token_expiry: std::time::Duration::from_secs(config.security.jwt_expiry_hours * 3600),
        require_auth: config.rpc.enable_authentication,
        admin_api_key: config.security.admin_api_key.clone(),
    };
    
    // Start RPC server
    let rpc_server = RpcServer::with_shared_components(
        blockchain.clone(),
        storage.clone(),
        rate_limit_config,
        auth_config,
        config.rpc.clone(),
        network_handle.clone(),
        miner.clone(),
    )?;
    
    // Start RPC server in background
    let rpc_port = config.rpc.port;
    tokio::spawn(async move {
        if let Err(e) = rpc_server.start(rpc_port).await {
            log::error!("RPC server error: {}", e);
        }
    });
    
    // Start Stratum server if mining is enabled – offload PoW via Stratum
    if config.mining.enabled {
        // Build the mining service and wrap in Arc for sharing
        let mining_service = Arc::new(
            MiningService::new(
                blockchain.clone(),
                network_handle.clone(),
                miner.clone(),
                config.mining.clone(),
                config.consensus.clone(),
            )
        );
        let bind = format!("{}:{}", config.mining.stratum_bind_address, config.mining.stratum_bind_port);
        let bind_clone = bind.clone();
        tokio::spawn(async move {
            log::info!("🚀 Starting Stratum mining server on {}", bind_clone);
            let stratum_server = StratumV2Server::with_connection_tracking(mining_service, Some(stratum_signal_tx));
            if let Err(e) = stratum_server.start().await {
                log::error!("Stratum server error: {}", e);
            }
        });
        log::info!("Stratum server launched on {}", bind);
    }
    
    log::info!("Node started successfully");
    log::info!("RPC server: http://localhost:{}", config.rpc.port);
    log::info!("Network: {}", config.network.listen_address);
    log::info!("Data directory: {}", config.storage.data_directory.display());
    
    // Wait for shutdown signal
    tokio::select! {
        _ = signal::ctrl_c() => {
            log::info!("Shutting down...");
            
            // Shutdown local miner if running
            if let Some(ref miner) = local_miner {
                log::info!("🛑 Stopping local miner...");
                miner.shutdown();
            }
        }
    }
    
    Ok(())
}
