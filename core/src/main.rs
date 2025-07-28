use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::signal;
use parking_lot::RwLock;

use numi_core::{
    config::Config,
    blockchain::NumiBlockchain,
    storage::BlockchainStorage,
    rpc::{RpcServer, RateLimitConfig, AuthConfig},
    crypto::Dilithium3Keypair,
    transaction::{Transaction, TransactionType},
    network::NetworkManager,
    mining_service::MiningService,
    miner::Miner,
    Result,
    BlockchainError,
};
use chrono::Utc;
use reqwest::Client;
use numi_core::rpc::types::{ApiResponse, BalanceResponse, StatusResponse, TransactionRequest, TransactionResponse, MiningRequest, MiningResponse};

#[derive(Parser)]
#[command(name = "numi", about = "NumiCoin - Production blockchain node", version)]
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
    /// Start the blockchain node
    Start {
        #[arg(long)]
        mine: bool,
        
        #[arg(long, default_value = "4")]
        threads: usize,
    },
    
    /// Show node status
    Status,
    
    /// Get account balance
    Balance {
        address: String,
    },
    
    /// Send transaction
    Send {
        #[arg(long)]
        wallet: PathBuf,
        to: String,
        amount: f64,
        #[arg(long)]
        memo: Option<String>,
    },
    
    /// Create new wallet
    Wallet {
        #[arg(long, default_value = "wallet.json")]
        output: PathBuf,
    },
    
    /// Mine blocks
    Mine {
        #[arg(long)]
        wallet: PathBuf,
    },
}

// Helper to build RPC base URL
fn rpc_base_url(config: &Config) -> String {
    format!("http://{}:{}", config.rpc.bind_address, config.rpc.port)
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
        Commands::Start { mine, threads } => {
            config.mining.enabled = mine;
            config.mining.thread_count = threads;
            start_node(config).await?;
        }
        Commands::Status => {
            show_status(config).await?;
        }
        Commands::Balance { address } => {
            show_balance(config, address).await?;
        }
        Commands::Send { wallet, to, amount, memo } => {
            send_transaction(config, wallet, to, amount, memo).await?;
        }
        Commands::Wallet { output } => {
            create_wallet(output).await?;
        }
        Commands::Mine { wallet } => {
            mine_blocks(config, wallet).await?;
        }
    }
    
    Ok(())
}

async fn load_config(cli: &Cli) -> Result<Config> {
    if cli.config.exists() {
        Ok(Config::load_from_file(&cli.config)
            .map_err(|e| BlockchainError::IoError(e.to_string()))?)
    } else {
        log::info!("Creating default configuration at {}", cli.config.display());
        let config = Config::production();
        config.save_to_file(&cli.config)
            .map_err(|e| BlockchainError::IoError(e.to_string()))?;
        Ok(config)
    }
}

async fn start_node(config: Config) -> Result<()> {
    log::info!("Starting NumiCoin node...");
    
    // Initialize storage and load blockchain
    let storage = Arc::new(BlockchainStorage::new(&config.storage.data_directory)?);
    let blockchain = Arc::new(RwLock::new(
        NumiBlockchain::load_from_storage(&storage).await?
    ));
    
    // Initialize network manager
    let mut network_manager = NetworkManager::new(blockchain.clone())?;
    let network_handle = network_manager.create_handle();

    // Build the libp2p multi-address "/ip4/<listen_address>/tcp/<port>"
    let listen_multiaddr = format!("/ip4/{}/tcp/{}", 
        config.network.listen_address,
        config.network.listen_port);

    // Spawn the network manager in the background (listening + event loop)
    tokio::spawn(async move {
        if let Err(e) = async {
            network_manager.start(&listen_multiaddr).await?;
            network_manager.run_event_loop().await;
            Ok::<(), BlockchainError>(())
        }.await {
            log::error!("Network manager error: {e}");
        }
    });
    
    // Initialize miner
    let miner = Arc::new(RwLock::new(Miner::new()?));
    
    // Create rate limit config from RPC config
    let rate_limit_config = RateLimitConfig {
        requests_per_minute: config.rpc.rate_limit_requests_per_minute,
        burst_size: config.rpc.rate_limit_burst_size,
        cleanup_interval: std::time::Duration::from_secs(300),
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
    
    // Start mining service if enabled – reuse the already-initialized miner
    if config.mining.enabled {
        let mining_service = MiningService::new(
            blockchain.clone(),
            network_handle,
            miner.clone(),
            config.mining.clone(),
            config.consensus.target_block_time,
        );
        
        tokio::spawn(async move {
            mining_service.start_mining_loop().await;
        });
        
        log::info!("Mining enabled with {} threads", config.mining.thread_count);
    }
    
    log::info!("Node started successfully");
    log::info!("RPC server: http://localhost:{}", config.rpc.port);
    log::info!("Network: {}", config.network.listen_address);
    log::info!("Data directory: {}", config.storage.data_directory.display());
    
    // Wait for shutdown signal
    tokio::select! {
        _ = signal::ctrl_c() => {
            log::info!("Shutting down...");
        }
    }
    
    Ok(())
}

async fn show_status(config: Config) -> Result<()> {
    let client = Client::new();
    let url = format!("{}/status", rpc_base_url(&config));
    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| BlockchainError::NetworkError(e.to_string()))?
        .json::<ApiResponse<StatusResponse>>()
        .await
        .map_err(|e| BlockchainError::SerializationError(e.to_string()))?;

    if response.success {
        let data = response.data.ok_or_else(|| BlockchainError::InvalidArgument("No data in response".to_string()))?;
        println!("Chain Height: {}", data.total_blocks);
        println!("Total Supply: {} NUMI", data.total_supply);
        println!("Difficulty: {}", data.current_difficulty);
        println!("Best Block Hash: {}", data.best_block_hash);
        println!("Pending Transactions: {}", data.mempool_transactions);
        println!("Mempool Size: {} bytes", data.mempool_size_bytes);
        println!("Network Peers: {}", data.network_peers);
        println!("Is Syncing: {}", data.is_syncing);
    } else {
        return Err(BlockchainError::NetworkError(response.error.unwrap_or_else(|| "Unknown error".into())));
    }
    Ok(())
}

async fn show_balance(config: Config, address: String) -> Result<()> {
    let client = Client::new();
    let url = format!("{}/balance/{}", rpc_base_url(&config), address);
    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| BlockchainError::NetworkError(e.to_string()))?
        .json::<ApiResponse<BalanceResponse>>()
        .await
        .map_err(|e| BlockchainError::SerializationError(e.to_string()))?;

    if response.success {
        let data = response.data.ok_or_else(|| BlockchainError::InvalidArgument("No data in response".to_string()))?;
        println!("Address: {}", data.address);
        println!("Balance: {} NUMI", data.balance);
        println!("Nonce: {}", data.nonce);
    } else {
        return Err(BlockchainError::NetworkError(response.error.unwrap_or_else(|| "Unknown error".into())));
    }
    Ok(())
}

async fn send_transaction(config: Config, wallet_path: PathBuf, to: String, amount: f64, memo: Option<String>) -> Result<()> {
    let client = Client::new();
    let base_url = rpc_base_url(&config);

    // Load sender wallet
    let keypair = load_wallet(&wallet_path).await?;
    let sender_pubkey = keypair.public_key.clone();
    let from_address = hex::encode(&sender_pubkey);

    // Fetch current nonce via RPC (fallback to 0 if unavailable)
    let nonce = match client
        .get(&format!("{}/balance/{}", base_url, from_address))
        .send()
        .await
    {
        Ok(resp) => {
            match resp.json::<ApiResponse<BalanceResponse>>().await {
                Ok(api_resp) if api_resp.success => api_resp.data.map(|d| d.nonce).unwrap_or(0),
                _ => 0,
            }
        }
        Err(_) => 0,
    };

    // Parse recipient address
    let recipient_pubkey = hex::decode(&to)
        .map_err(|_| BlockchainError::InvalidArgument("Invalid recipient address".to_string()))?;

    let amount_raw = amount as u64;

    // Create and sign transaction
    let transaction_type = TransactionType::Transfer {
        to: recipient_pubkey,
        amount: amount_raw,
        memo,
    };

    let mut transaction = Transaction::new(sender_pubkey.clone(), transaction_type, nonce);
    transaction.sign(&keypair)?;

    let signature_hex = match &transaction.signature {
        Some(sig) => hex::encode(&sig.signature),
        None => return Err(BlockchainError::InvalidSignature("Missing signature".to_string())),
    };

    let tx_request = TransactionRequest {
        from: from_address,
        to: to.clone(),
        amount: amount_raw,
        nonce,
        fee: Some(transaction.fee),
        signature: signature_hex,
    };

    let resp = client
        .post(&format!("{}/transaction", base_url))
        .json(&tx_request)
        .send()
        .await
        .map_err(|e| BlockchainError::NetworkError(e.to_string()))?
        .json::<ApiResponse<TransactionResponse>>()
        .await
        .map_err(|e| BlockchainError::SerializationError(e.to_string()))?;

    if resp.success {
        let data = resp.data.ok_or_else(|| BlockchainError::InvalidArgument("No data in response".to_string()))?;
        println!("Transaction ID: {}", data.id);
        println!("Validation Result: {}", data.validation_result);
        println!("Status: {}", data.status);
    } else {
        return Err(BlockchainError::InvalidArgument(resp.error.unwrap_or_else(|| "Unknown error".into())));
    }
    Ok(())
}

async fn create_wallet(output: PathBuf) -> Result<()> {
    let keypair = Dilithium3Keypair::new()?;
    let public_key = keypair.public_key.clone();
    let address = hex::encode(&public_key);
    
    let wallet_data = serde_json::json!({
        "public_key": hex::encode(&public_key),
        "private_key": hex::encode(&keypair.secret_key_bytes()),
        "address": address,
        "created_at": Utc::now().to_rfc3339()
    });
    
    std::fs::write(&output, serde_json::to_string_pretty(&wallet_data)?)?;
    
    log::info!("Wallet created successfully");
    println!("File: {}", output.display());
    println!("Address: {}", address);
    
    Ok(())
}

async fn mine_blocks(config: Config, _wallet_path: PathBuf) -> Result<()> {
    log::info!("Requesting remote mining via RPC...");

    let client = Client::new();
    let base_url = rpc_base_url(&config);

    let mining_req = MiningRequest {
        threads: Some(config.mining.thread_count),
        timeout_seconds: Some(60),
    };

    let resp = client
        .post(&format!("{}/mine", base_url))
        .json(&mining_req)
        .send()
        .await
        .map_err(|e| BlockchainError::NetworkError(e.to_string()))?
        .json::<ApiResponse<MiningResponse>>()
        .await
        .map_err(|e| BlockchainError::SerializationError(e.to_string()))?;

    if resp.success {
        let data = resp.data.ok_or_else(|| BlockchainError::InvalidArgument("No data in response".to_string()))?;
        println!("{}", data.message);
        println!("Block Hash: {}", data.block_hash);
        println!("Mining Time: {} ms", data.mining_time_ms);
        println!("Hash Rate: {} H/s", data.hash_rate);
    } else {
        return Err(BlockchainError::InvalidArgument(resp.error.unwrap_or_else(|| "Mining failed".into())));
    }
    Ok(())
}

async fn load_wallet(path: &PathBuf) -> Result<Dilithium3Keypair> {
    let data = std::fs::read_to_string(path)?;
    let wallet: serde_json::Value = serde_json::from_str(&data)?;
    
    let private_key_hex = wallet["private_key"].as_str()
        .ok_or_else(|| BlockchainError::InvalidArgument("Invalid wallet format".to_string()))?;
    
    let private_key = hex::decode(private_key_hex)
        .map_err(|e| BlockchainError::InvalidArgument(format!("Invalid private key hex: {}", e)))?;
    
    Dilithium3Keypair::from_bytes(
        wallet["public_key"].as_str()
            .ok_or_else(|| BlockchainError::InvalidArgument("Missing public key".to_string()))?
            .as_bytes()
            .to_vec(),
        private_key,
    )
}


