use clap::{Parser, Subcommand};
use numi_core::{
    blockchain::NumiBlockchain,
    storage::BlockchainStorage,
    miner::{Miner, MiningConfig},
    mining_service::MiningService,
    network::NetworkManager,
    crypto::Dilithium3Keypair,
    transaction::{Transaction, TransactionType},
    rpc::{RpcServer, RateLimitConfig, AuthConfig},
    config::Config,
    BlockchainError,
    Result,
};
use std::path::PathBuf;
use fs2::FileExt;
use tokio::net::TcpListener;
use serde::{Serialize, Deserialize};
use std::sync::Arc;
use parking_lot::RwLock;

#[derive(Debug, Serialize, Deserialize)]
struct StatusInfo {
    total_blocks: u64,
    total_supply_numi: f64,
    current_difficulty: u32,
    average_block_time: u64,
    last_block_time: String,
    active_miners: usize,
    latest_block_hash: Option<String>,
    latest_block_transactions: usize,
    pending_transactions: usize,
    network: Option<StatusNetworkInfo>,
    node_version: String,
    uptime_seconds: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct StatusNetworkInfo {
    peer_count: usize,
    connected_peers: Vec<String>,
    network_health: String,
}

// Helper function to check if a port is available
async fn is_port_available(port: u16) -> bool {
    (TcpListener::bind(format!("0.0.0.0:{port}")).await).is_ok()
}

#[derive(Parser)]
#[command(name = "numi-node")]
#[command(about = "Numi blockchain node - Quantum-safe cryptocurrency")]
#[command(version = "1.0.0")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
    
    /// Configuration file path
    #[arg(short, long, default_value = "numi.toml")]
    config: PathBuf,
    
    /// Data directory (overrides config file)
    #[arg(short, long, global = true)]
    data_dir: Option<PathBuf>,
    
    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,
    
    /// Network environment
    #[arg(short, long, default_value = "development")]
    environment: Environment,
}

#[derive(clap::ValueEnum, Clone, Debug)]
enum Environment {
    Development,
    Production,
    Testnet,
    Testing,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the blockchain node with full services
    Start {
        /// RPC server port (overrides config)
        #[arg(long)]
        rpc_port: Option<u16>,
        
        /// Network listen address (overrides config)
        #[arg(long)]
        listen_addr: Option<String>,
        
        /// Enable mining
        #[arg(long)]
        enable_mining: bool,
        
        /// Mining threads (overrides config)
        #[arg(long)]
        mining_threads: Option<usize>,
        
        /// Miner wallet file path (overrides config)
        #[arg(long)]
        miner_key: Option<PathBuf>,
    },
    
    /// Mine a single block
    Mine {
        /// Number of mining threads
        #[arg(long)]
        threads: Option<usize>,
        
        /// Mining timeout in seconds
        #[arg(long, default_value = "300")]
        timeout: u64,
        
        /// Miner keypair file
        #[arg(long)]
        miner_key: Option<PathBuf>,
    },
    
    /// Submit a transaction to the network
    Submit {
        /// Sender's private key file
        #[arg(long)]
        from_key: PathBuf,
        
        /// Recipient address (hex-encoded public key)
        #[arg(long)]
        to: String,
        
        /// Amount to transfer (in smallest units)
        #[arg(long)]
        amount: u64,
        
        /// Transaction fee (optional, calculated if not provided)
        #[arg(long)]
        fee: Option<u64>,
        
        /// Optional memo
        #[arg(long)]
        memo: Option<String>,
    },

    /// Sign a transaction payload and output the signature
    SignTransaction {
        /// Keypair file path
        #[arg(long)]
        key: PathBuf,
        /// Recipient public key (hex)
        #[arg(long)]
        to: String,
        /// Amount in smallest units
        #[arg(long)]
        amount: u64,
        /// Transaction nonce
        #[arg(long)]
        nonce: u64,
    },

    /// Get blockchain and node status
    Status {
        /// Show detailed statistics
        #[arg(long)]
        detailed: bool,
        
        /// Output format
        #[arg(long, default_value = "human")]
        format: OutputFormat,
    },
    
    /// Get account balance and information
    Balance {
        /// Account address (hex-encoded public key)
        #[arg(long)]
        address: String,
        
        /// Show transaction history
        #[arg(long)]
        history: bool,
    },
    
    /// List all accounts with their complete addresses and balances
    Accounts {
        /// Output format
        #[arg(long, default_value = "human")]
        format: OutputFormat,
        
        /// Show underlying public keys in addition to addresses
        #[arg(long)]
        full: bool,
    },
    
    /// Initialize a new blockchain with genesis block
    Init {
        /// Force initialization (overwrite existing data)
        #[arg(long)]
        force: bool,
        
        /// Genesis configuration file
        #[arg(long)]
        genesis_config: Option<PathBuf>,
    },
    
    /// Start only the RPC API server
    Rpc {
        /// RPC server port
        #[arg(long)]
        port: Option<u16>,
        
        /// Bind to all interfaces (0.0.0.0)
        #[arg(long)]
        public: bool,
    },
    
    /// Generate a new key pair
    GenerateKey {
        /// Output file for the key pair
        #[arg(long)]
        output: PathBuf,
        
        /// Key format (pem, json)
        #[arg(long, default_value = "json")]
        format: String,
    },
    
    /// Create a default configuration file
    CreateConfig {
        /// Output configuration file path
        #[arg(long, default_value = "numi.toml")]
        output: PathBuf,
        
        /// Environment type for the configuration
        #[arg(long, default_value = "development")]
        env: Environment,
    },
    
    /// Backup the blockchain data
    Backup {
        /// Backup output directory
        #[arg(long)]
        output: PathBuf,
        
        /// Compress the backup
        #[arg(long)]
        compress: bool,
    },
    
    /// Restore blockchain data from backup
    Restore {
        /// Backup directory or file
        #[arg(long)]
        input: PathBuf,
        
        /// Verify backup integrity before restoring
        #[arg(long)]
        verify: bool,
    },
}

#[derive(clap::ValueEnum, Clone, Debug)]
enum OutputFormat {
    Human,
    Json,
    Yaml,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    
    // Initialize logging based on verbosity
    if cli.verbose {
        std::env::set_var("RUST_LOG", "debug");
    } else {
        std::env::set_var("RUST_LOG", "info");
    }
    env_logger::init();
    
    // Load or create configuration
    let mut config = load_or_create_config(&cli).await?;
    
    // Apply CLI overrides to configuration
    apply_cli_overrides(&mut config, &cli);
    
    // Validate configuration
    if let Err(e) = config.validate() {
        eprintln!("❌ Configuration validation failed: {e}");
        std::process::exit(1);
    }

    // ------------------------------------------------------------------
    // Acquire exclusive lock on the data directory to avoid double opens
    // ------------------------------------------------------------------
    let _data_dir_lock = match acquire_data_dir_lock(&config.storage.data_directory) {
        Ok(lock) => lock,
        Err(e) => {
            eprintln!("❌ Failed to acquire data directory lock: {e}");
            std::process::exit(1);
        }
    };
 
    log::info!("🚀 NumiCoin Node v1.0.0 starting...");
    log::info!("🔧 Environment: {:?}", cli.environment);
    log::info!("📁 Data directory: {:?}", config.storage.data_directory);
    
    match cli.command {
        Commands::Start { rpc_port, listen_addr, enable_mining, mining_threads, miner_key } => {
            // Override config with CLI arguments
            if let Some(port) = rpc_port {
                config.rpc.port = port;
            }
            if let Some(addr) = listen_addr {
                config.network.listen_address = addr;
            }
            if let Some(threads) = mining_threads {
                config.mining.thread_count = threads;
            }
            config.mining.enabled = enable_mining;
            if let Some(miner_path) = miner_key {
                config.mining.wallet_path = miner_path;
            }
            
            start_full_node(config).await?;
        }
        Commands::Mine { threads, timeout, miner_key } => {
            mine_block_command(config, threads, timeout, miner_key).await?;
        }
        Commands::Submit { from_key, to, amount, fee, memo } => {
            submit_transaction_command(config, from_key, to, amount, fee, memo).await?;
        }
        Commands::SignTransaction { key, to, amount, nonce } => {
            sign_transaction_command(key, to, amount, nonce).await?;
        }
        Commands::Status { detailed, format } => {
            show_status_command(config, detailed, format).await?;
        }
        Commands::Balance { address, history } => {
            show_balance_command(config, address, history).await?;
        }
        Commands::Accounts { format, full } => {
            show_accounts_command(config, format, full).await?;
        }
        Commands::Init { force, genesis_config } => {
            init_blockchain_command(config, force, genesis_config).await?;
        }
        Commands::Rpc { port, public } => {
            if let Some(port) = port {
                config.rpc.port = port;
            }
            if public {
                config.rpc.bind_address = "0.0.0.0".to_string();
            }
            start_rpc_server_command(config).await?;
        }
        Commands::GenerateKey { output, format } => {
            generate_key_command(output, format).await?;
        }
        Commands::CreateConfig { output, env } => {
            create_config_command(output, env).await?;
        }
        Commands::Backup { output, compress } => {
            backup_command(config, output, compress).await?;
        }
        Commands::Restore { input, verify } => {
            restore_command(config, input, verify).await?;
        }
    }
    
    Ok(())
}

/// Load configuration from file or create default
async fn load_or_create_config(cli: &Cli) -> Result<Config> {
    if cli.config.exists() {
        log::info!("📄 Loading configuration from {:?}", cli.config);
        match Config::load_from_file(&cli.config) {
            Ok(config) => Ok(config),
            Err(e) => {
                log::error!("❌ Failed to load configuration: {e}");
                log::info!("🔧 Creating default configuration...");
                create_default_config(&cli.environment)
            }
        }
    } else {
        log::info!("🔧 Configuration file not found, creating default...");
        let config = create_default_config(&cli.environment)?;
        
        // Save the default configuration
        if let Err(e) = config.save_to_file(&cli.config) {
            log::warn!("⚠️ Failed to save default configuration: {e}");
        } else {
            log::info!("💾 Default configuration saved to {:?}", cli.config);
        }
        
        Ok(config)
    }
}

/// Create default configuration based on environment
fn create_default_config(env: &Environment) -> Result<Config> {
    let config = match env {
        Environment::Development => Config::development(),
        Environment::Production => Config::production(),
        Environment::Testnet => Config::testnet(),
        Environment::Testing => {
            let mut config = Config::development();
            config.consensus.target_block_time = std::time::Duration::from_secs(1); // Very fast for testing
            config.mining.thread_count = 1;
            config
        }
    };
    
    Ok(config)
}

/// Apply command-line overrides to configuration
fn apply_cli_overrides(config: &mut Config, cli: &Cli) {
    if let Some(ref data_dir) = cli.data_dir {
        config.storage.data_directory = data_dir.clone();
    }
    
    // Set log level based on verbosity
    if cli.verbose {
        log::debug!("🔍 Verbose logging enabled");
    }
}

// ------------------------------------------------------------------
// Storage locking helper to prevent concurrent node instances (Issue #6)
// ------------------------------------------------------------------
fn acquire_data_dir_lock<P: AsRef<std::path::Path>>(data_dir: P) -> std::io::Result<std::fs::File> {
    use std::fs::{self, OpenOptions};
    let dir = data_dir.as_ref();
    fs::create_dir_all(dir)?;
    let lock_path = dir.join(".lock");
    let lock_file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(lock_path)?;
    // Exclusive lock – if this fails another node is running
    lock_file.try_lock_exclusive()?;
    Ok(lock_file)
}


async fn start_full_node(config: Config) -> Result<()> {
    // removed unused spawn_blocking import

    log::info!("🚀 Starting Numi blockchain node...");

    // ----------------------- Storage & Chain -----------------------
    let storage = std::sync::Arc::new(BlockchainStorage::new(&config.storage.data_directory)?);
    log::info!("✅ Storage initialized at {:?}", config.storage.data_directory);

    // Load existing chain or create new one
    let initial_chain = match NumiBlockchain::load_from_storage_with_config(&storage, Some(config.consensus.clone())).await {
        Ok(chain) => chain,
        Err(_) => {
            log::warn!("🆕 No existing chain found – creating new genesis");
            NumiBlockchain::new_with_config(Some(config.consensus.clone()), None)?
        }
    };
    let blockchain = std::sync::Arc::new(parking_lot::RwLock::new(initial_chain));
    log::info!("✅ Blockchain ready (height: {})", blockchain.read().get_current_height());

    // Prepare RPC configuration if enabled
    let rpc_config = if config.rpc.enabled {
        let rate_limit_cfg = RateLimitConfig {
            requests_per_minute: config.rpc.rate_limit_requests_per_minute,
            burst_size: config.rpc.rate_limit_burst_size,
            cleanup_interval: std::time::Duration::from_secs(config.rpc.request_timeout_secs),
        };
        let mut auth_cfg = AuthConfig::default();
        auth_cfg.require_auth = config.rpc.enable_authentication;

        let blockchain_clone = blockchain.clone();
        let _storage_clone = storage.clone();
        
        // Use consistent wallet path resolution for RPC server
        let miner = std::sync::Arc::new(parking_lot::RwLock::new(
            Miner::with_config_and_data_dir(MiningConfig::default(), config.storage.data_directory.clone())?
        ));

        // Store RPC config for later use after network is started
        Some((
            blockchain_clone,
            _storage_clone,
            rate_limit_cfg,
            auth_cfg,
            miner,
            config.rpc.port,
            config.rpc.bind_address.clone(),
        ))
    } else {
        None
    };
    use tokio::time::{self, Duration};

    // ----------------------- Networking ---------------------------
    let mut network = NetworkManager::new(blockchain.clone())?;
    let network_addr = format!("/ip4/{}/tcp/{}", config.network.listen_address, config.network.listen_port);
    network.start(&network_addr).await?;
    log::info!("✅ Network started on {network_addr}");

    // Spawn the async event-loop so it doesn't block our main task
    let network_handle = network.create_handle();
    tokio::spawn(async move {
        // Run the network event loop (no error return expected)
        network.run_event_loop().await;
    });

    // Start RPC server after network is initialized
    if let Some((blockchain_clone, _storage_clone, rate_limit_cfg, auth_cfg, miner, port, bind_addr)) = rpc_config {
        let network_handle_clone = network_handle.clone();
        
        let rpc_server = RpcServer::with_shared_components(
            blockchain_clone,
            _storage_clone,
            rate_limit_cfg,
            auth_cfg,
            config.rpc.clone(),
            network_handle_clone,
            miner,
        )?;
        
        tokio::spawn(async move {
            // Check if the port is available before starting the server
            let mut port_to_use = port;
            let max_attempts = 5;
            
            // Find an available port
            for attempt in 1..=max_attempts {
                if is_port_available(port_to_use).await {
                    break;
                } else if attempt < max_attempts {
                    log::warn!("⚠️ Port {} is in use, trying port {} (attempt {}/{})", 
                             port_to_use, port_to_use + 1, attempt, max_attempts);
                    port_to_use += 1;
                } else {
                    log::error!("❌ Could not find available port after {max_attempts} attempts");
                    return;
                }
            }
            
            // Start the server on the available port
            if let Err(e) = rpc_server.start(port_to_use).await {
                log::error!("❌ RPC server failed to start on port {port_to_use}: {e}");
            } else {
                log::info!("✅ RPC server started successfully on port {port_to_use}");
            }
        });
        
        log::info!(
            "🚀 RPC API server spawned in background on {bind_addr}:{port}"
        );
    }

    // ----------------------- Mining Service -----------------------
    if config.mining.enabled {
        let mining_service = MiningService::new(
            blockchain.clone(),
            network_handle.clone(),
            config.mining.clone(),
            config.storage.data_directory.clone(),
            config.consensus.target_block_time,
        );
        
        tokio::spawn(async move {
            mining_service.start_mining_loop().await;
        });
    }

    log::info!("🎯 Node is running! Press Ctrl+C to stop.");

    // Periodic status & graceful shutdown handling
    let mut status_interval = time::interval(Duration::from_secs(10));

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                log::info!("🛑 Ctrl+C received – beginning graceful shutdown");

                // Flush blockchain state
                if let Err(e) = blockchain.read().save_to_storage(&storage) {
                    log::error!("❌ Failed to persist chain state: {e}");
                }

                // Background miner tasks will be dropped on shutdown

                log::info!("👋 Shutdown complete. Goodbye!");
                break;
            }
            _ = status_interval.tick() => {
                let state = blockchain.read().get_chain_state();
                let peer_cnt = network_handle.get_peer_count().await;
                log::info!("📈 Status – Blocks: {}, Difficulty: {}, Peers: {}", 
                    state.total_blocks, state.current_difficulty, peer_cnt);
            }
        }
    }

    Ok(())
}

async fn mine_block_command(config: Config, threads: Option<usize>, _timeout: u64, miner_key_path: Option<PathBuf>) -> Result<()> {
    log::info!("⛏️ Starting mining operation...");
    
    // Initialize storage and blockchain
    let storage = BlockchainStorage::new(&config.storage.data_directory)?;
    let blockchain = NumiBlockchain::load_from_storage(&storage).await?;
    
    // Create or load miner keypair using consistent path resolution
    let keypair = if let Some(path) = miner_key_path {
        // Use provided path directly
        Dilithium3Keypair::load_from_file(&path)?
    } else {
        // Use configured wallet path with data directory resolution
        let wallet_path = config.mining.wallet_path.clone();
        let resolved_path = if wallet_path.is_absolute() {
            wallet_path
        } else {
            config.storage.data_directory.join(&wallet_path)
        };
        
        match Dilithium3Keypair::load_from_file(&resolved_path) {
            Ok(kp) => {
                log::info!("🔑 Loaded existing miner wallet from {:?}", resolved_path);
                kp
            }
            Err(_) => {
                log::info!("🔑 Creating new miner keypair (no existing wallet found at {:?})", resolved_path);
                let kp = Dilithium3Keypair::new()?;
                
                // Ensure parent directory exists
                if let Some(parent) = resolved_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                
                kp.save_to_file(&resolved_path)?;
                log::info!("✅ New miner wallet saved to {:?}", resolved_path);
                kp
            }
        }
    };
    
    log::info!("🔑 Mining with public key: {}", hex::encode(&keypair.public_key));
    
    // Get pending transactions
    let pending_txs = blockchain.get_transactions_for_block(1_000_000, 1000);
    log::info!("📝 Found {} pending transactions", pending_txs.len());
    
    // Start mining with the loaded keypair
    let mut miner = Miner::with_config_and_keypair(MiningConfig::default(), keypair)?;
    if let Some(t) = threads {
        let mut cfg = miner.get_config().clone();
        cfg.thread_count = t;
        miner.update_config(cfg);
    }
    let start_time = std::time::Instant::now();
    
    let mining_result = miner.mine_block(
        blockchain.get_current_height() + 1,
        blockchain.get_latest_block_hash(),
        pending_txs,
        blockchain.get_current_difficulty(),
        0,
    );
    
    match mining_result {
        Ok(Some(result)) => {
            let mining_time = start_time.elapsed();
            log::info!("🎉 Block mined successfully!");
            log::info!("📊 Block height: {}", result.block.header.height);
            log::info!("🔢 Nonce: {}", result.nonce);
            log::info!("⏱️ Mining time: {mining_time:?}");
            log::info!("⚡ Hash rate: {} H/s", result.hash_rate);
            
            // Add block to blockchain
            blockchain.add_block(result.block).await?;
            
            // Save to storage
            blockchain.save_to_storage(&storage)?;
            log::info!("✅ Block added to blockchain and saved to storage");
        }
        Ok(None) => {
            log::info!("⏹️ Mining stopped");
        }
        Err(e) => {
            log::error!("❌ Mining failed: {e}");
        }
    }
    
    Ok(())
}

async fn parse_recipient_address(to: &str) -> Result<Vec<u8>> {
    if to.len() == 64 {
        // Input is hashed address hex (32 bytes)
        hex::decode(to)
            .map_err(|e| BlockchainError::InvalidTransaction(format!("Invalid address hex: {e}")))
    } else if to.len() == 128 {
        // Input is public key hex (64 bytes) - derive address from it
        let pk_bytes = hex::decode(to)
            .map_err(|e| BlockchainError::InvalidTransaction(format!("Invalid public key hex: {e}")))?;
        if pk_bytes.len() != 64 {
            return Err(BlockchainError::InvalidTransaction(
                "Invalid public key length: expected 64 bytes".to_string(),
            ));
        }
        Ok(numi_core::crypto::blake3_hash(&pk_bytes).to_vec())
    } else if PathBuf::from(to).exists() {
        // Input is wallet JSON file: load keypair and derive address from public key
        let file_content = std::fs::read_to_string(to)
            .map_err(|e| BlockchainError::IoError(format!("Failed to read wallet file: {e}")))?;
        let keypair: Dilithium3Keypair = serde_json::from_str(&file_content)
            .map_err(|e| BlockchainError::SerializationError(format!("Invalid wallet file: {e}")))?;
        Ok(numi_core::crypto::blake3_hash(&keypair.public_key).to_vec())
    } else {
        Err(BlockchainError::InvalidTransaction("Invalid recipient address: expected 64-char hashed address, 128-char public key, or wallet file path".to_string()))
    }
}

async fn submit_transaction_command(config: Config, from_key_path: PathBuf, to: String, amount: u64, fee: Option<u64>, memo: Option<String>) -> Result<()> {
    log::info!("📤 Submitting transaction...");
    
    // Initialize storage and blockchain
    let storage = BlockchainStorage::new(&config.storage.data_directory)?;
    let blockchain = NumiBlockchain::load_from_storage(&storage).await?;
    
    // Create keypair for sender
    let sender_keypair = Dilithium3Keypair::load_from_file(&from_key_path)?;
    
    // Parse recipient address - handle different formats like balance command
    let recipient_address = parse_recipient_address(&to).await?;
    
    // Validate recipient address length
    if recipient_address.len() != 32 {
        return Err(numi_core::BlockchainError::InvalidTransaction(
            format!("Invalid recipient address length: expected 32 bytes, got {}", recipient_address.len())
        ));
    }
    
    let nonce = blockchain.get_account_state_or_default(&sender_keypair.public_key).nonce + 1;
    
    // Create transaction with custom fee or calculate minimum fee
    let mut transaction = if let Some(custom_fee) = fee {
        // Use custom fee
        Transaction::new_with_fee(
            sender_keypair.public_key.clone(),
            TransactionType::Transfer {
                to: recipient_address,
                amount,
                memo,
            },
            nonce,
            custom_fee,
            0, // No gas limit for simple transfers
        )
    } else {
        // Calculate minimum fee for transaction size (estimate ~500 bytes for typical transfer)
        let estimated_size = 500;
        let fee_info = numi_core::transaction::TransactionFee::minimum_for_size(estimated_size)?;
        
        Transaction::new_with_fee(
            sender_keypair.public_key.clone(),
            TransactionType::Transfer {
                to: recipient_address,
                amount,
                memo,
            },
            nonce,
            fee_info.total,
            0, // No gas limit for simple transfers
        )
    };
    
    // Check sender balance
    let sender_address = blockchain.get_address_from_public_key(&sender_keypair.public_key);
    let sender_balance = blockchain.get_balance(&sender_address);
    let total_cost = amount + transaction.fee;
    if sender_balance < total_cost {
        return Err(numi_core::BlockchainError::InvalidTransaction(
            format!("Insufficient balance: {} NUMI < {} NUMI (amount + fee)", 
                   sender_balance as f64 / 100.0, 
                   total_cost as f64 / 100.0)
        ));
    }
    
    // Sign transaction
    transaction.sign(&sender_keypair)?;
    
    // Submit transaction
    blockchain.add_transaction(transaction.clone()).await?;
    
    log::info!("✅ Transaction submitted successfully!");
    log::info!("🆔 Transaction ID: {}", transaction.get_hash_hex());
    log::info!("📤 From: {}", hex::encode(&sender_keypair.public_key));
    log::info!("📥 To: {to}");
    log::info!("💰 Amount: {} NUMI", amount as f64 / 100.0);
    log::info!("💸 Fee: {} NUMI", transaction.fee as f64 / 100.0);  
    
    Ok(())
}

async fn sign_transaction_command(key_path: PathBuf, to: String, amount: u64, nonce: u64) -> Result<()> {
    // Load keypair and build transaction
    let keypair = Dilithium3Keypair::load_from_file(&key_path)?;
    let recipient = hex::decode(&to)
        .map_err(|e| numi_core::BlockchainError::InvalidTransaction(format!("Invalid recipient hex: {e}")))?;
    let mut tx = Transaction::new(
        keypair.public_key.clone(),
        TransactionType::Transfer { to: recipient, amount, memo: None },
        nonce,
    );
    // Sign and serialize signature
    tx.sign(&keypair)?;
    let sig = tx.signature.as_ref().ok_or_else(|| numi_core::BlockchainError::CryptographyError("Missing signature".to_string()))?;
    let sig_bytes = bincode::serialize(sig).map_err(|e| numi_core::BlockchainError::CryptographyError(format!("Serialize error: {e}")))?;
    println!("{}", hex::encode(sig_bytes));
    Ok(())
}

async fn show_status_command(config: Config, detailed: bool, format: OutputFormat) -> Result<()> {
    // Initialize storage and blockchain
    let storage = BlockchainStorage::new(&config.storage.data_directory)?;
    let blockchain = NumiBlockchain::load_from_storage(&storage).await?;
    
    // Get chain state
    let state = blockchain.get_chain_state();
    
    // Get latest block info
    let latest_block_info = if let Some(latest_block) = blockchain.get_latest_block() {
        Some((latest_block.get_hash_hex()?, latest_block.get_transaction_count()))
    } else {
        None
    };
    
    // Get pending transactions
    let pending_txs = blockchain.get_pending_transaction_count();
    
    // Get network info (mock for now since we don't have easy access to network manager)
    let network_info = if detailed {
        Some(StatusNetworkInfo {
            peer_count: 0, // Would need network manager access
            connected_peers: vec![], // Would need network manager access
            network_health: "Unknown".to_string(), // Would need network manager access
        })
    } else {
        None
    };
    
    let status_info = StatusInfo {
        total_blocks: state.total_blocks,
        total_supply_numi: state.total_supply as f64 / 100.0,
        current_difficulty: state.current_difficulty,
        average_block_time: state.average_block_time,
        last_block_time: state.last_block_time.to_string(),
        active_miners: state.active_miners,
        latest_block_hash: latest_block_info.as_ref().map(|(hash, _)| hash.clone()),
        latest_block_transactions: latest_block_info.as_ref().map(|(_, count)| *count).unwrap_or(0),
        pending_transactions: pending_txs,
        network: network_info,
        node_version: "1.0.0".to_string(),
        uptime_seconds: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(), // Approximation
    };
    
    match format {
        OutputFormat::Json => {
            let json = serde_json::to_string_pretty(&status_info)
                .map_err(|e| numi_core::BlockchainError::SerializationError(e.to_string()))?;
            println!("{json}");
        }
        OutputFormat::Yaml => {
            let yaml = serde_yaml::to_string(&status_info)
                .map_err(|e| numi_core::BlockchainError::SerializationError(e.to_string()))?;
            println!("{yaml}");
        }
        OutputFormat::Human => {
            println!("Blockchain Status");
            println!("==================");
            println!("Total blocks: {}", status_info.total_blocks);
            println!("Total supply: {} NUMI", status_info.total_supply_numi);
            println!("Current difficulty: {}", status_info.current_difficulty);
            println!("Average block time: {} seconds", status_info.average_block_time);
            println!("Last block time: {}", status_info.last_block_time);
            println!("Active miners: {}", status_info.active_miners);
            
            if let Some(hash) = &status_info.latest_block_hash {
                println!("Latest block hash: {hash}");
                println!("Latest block transactions: {}", status_info.latest_block_transactions);
            } else {
                println!("No blocks found");
            }
            
            println!("Pending transactions: {}", status_info.pending_transactions);
            println!("Node version: {}", status_info.node_version);
            
            if detailed {
                if let Some(network) = &status_info.network {
                    println!("Network peers: {}", network.peer_count);
                    println!("Network health: {}", network.network_health);
                }
                println!("Node uptime: {} seconds", status_info.uptime_seconds);
            }
        }
    }
    
    Ok(())
}

async fn show_accounts_command(config: Config, format: OutputFormat, full: bool) -> Result<()> {
    // Initialize storage and blockchain
    let storage = BlockchainStorage::new(&config.storage.data_directory)?;
    let blockchain = NumiBlockchain::load_from_storage(&storage).await?;
    
    // Get all accounts from blockchain memory using the public method
    let accounts = blockchain.get_all_accounts();
    
    if accounts.is_empty() {
        println!("ℹ️  No accounts found");
        return Ok(());
    }
    
    let total_accounts = accounts.len();
    
    match format {
        OutputFormat::Human => {
            println!("📊 Account List");
            println!("===============");
            println!("📈 Total accounts: {total_accounts}");
            println!();
            
            for (public_key, account) in &accounts {
                // Always show the user-friendly Base58 address
                let base58_address = blockchain.get_address_from_public_key(public_key);
                
                println!("🏦 Address: {}", base58_address);
                println!("   Balance: {} NUMI", account.balance as f64 / 100.0);
                println!("   Nonce: {}", account.nonce);
                println!("   Transactions: {}", account.transaction_count);
                
                // Optionally show the full public key hex if requested
                if full {
                    println!("   Public Key: {}", hex::encode(public_key));
                }
                println!();
            }
            
            if !full {
                println!("💡 Use --full to see the underlying public keys");
                println!("   All addresses shown are ready to use with the balance command");
            }
        }
        OutputFormat::Json => {
            use serde_json::json;
            let mut accounts_json = Vec::new();
            
            for (public_key, account) in &accounts {
                // Always use the user-friendly Base58 address
                let base58_address = blockchain.get_address_from_public_key(public_key);
                
                let mut account_data = json!({
                    "address": base58_address,
                    "balance": account.balance as f64 / 100.0,
                    "nonce": account.nonce,
                    "transaction_count": account.transaction_count
                });
                
                // Optionally include the full public key hex if requested
                if full {
                    account_data["public_key"] = json!(hex::encode(public_key));
                }
                
                accounts_json.push(account_data);
            }
            
            let result = json!({
                "accounts": accounts_json,
                "total_count": total_accounts
            });
            
            println!("{}", serde_json::to_string_pretty(&result).unwrap());
        }
        OutputFormat::Yaml => {
            // For YAML output, create a similar structure but output as YAML
            use serde_yaml;
            use serde_json::json;
            
            let mut accounts_yaml = Vec::new();
            
            for (public_key, account) in &accounts {
                // Always use the user-friendly Base58 address
                let base58_address = blockchain.get_address_from_public_key(public_key);
                
                let mut account_data = json!({
                    "address": base58_address,
                    "balance": account.balance as f64 / 100.0,
                    "nonce": account.nonce,
                    "transaction_count": account.transaction_count
                });
                
                // Optionally include the full public key hex if requested
                if full {
                    account_data["public_key"] = json!(hex::encode(public_key));
                }
                
                accounts_yaml.push(account_data);
            }
            
            let result = json!({
                "accounts": accounts_yaml,
                "total_count": total_accounts
            });
            
            println!("{}", serde_yaml::to_string(&result).unwrap());
        }
    }
    
    Ok(())
}

async fn show_balance_command(config: Config, address: String, history: bool) -> Result<()> {
    log::info!("💰 Account Balance");
    log::info!("=================");
    
    // Initialize storage and blockchain
    let storage = BlockchainStorage::new(&config.storage.data_directory)?;
    let blockchain = NumiBlockchain::load_from_storage(&storage).await?;
    
    // Determine the input type and get the appropriate public key bytes
    let (public_key_bytes, display_address) = if address.len() == 64 {
        // Input is public key hex (64 chars = 32 bytes) - this is what accounts command shows
        let pk_bytes = hex::decode(&address)
            .map_err(|e| BlockchainError::InvalidTransaction(format!("Invalid public key hex: {e}")))?;
        let addr = blockchain.get_address_from_public_key(&pk_bytes);
        (pk_bytes, addr)
    } else if address.len() == 128 {
        // Input is full public key hex (128 chars = 64 bytes) - derive address from it
        let pk_bytes = hex::decode(&address)
            .map_err(|e| BlockchainError::InvalidTransaction(format!("Invalid public key hex: {e}")))?;
        if pk_bytes.len() != 64 {
            return Err(BlockchainError::InvalidTransaction(
                "Invalid public key length: expected 64 bytes".to_string(),
            ));
        }
        let hashed_pk = numi_core::crypto::blake3_hash(&pk_bytes).to_vec();
        let addr = blockchain.get_address_from_public_key(&hashed_pk);
        (hashed_pk, addr)
    } else if address.len() > 1000 && address.chars().all(|c| c.is_ascii_hexdigit()) {
        // Input is very long hex string (raw public key from accounts command)
        let pk_bytes = hex::decode(&address)
            .map_err(|e| BlockchainError::InvalidTransaction(format!("Invalid long public key hex: {e}")))?;
        let addr = blockchain.get_address_from_public_key(&pk_bytes);
        (pk_bytes, addr)
    } else if PathBuf::from(&address).exists() {
        // Input is wallet JSON file
        let file_content = std::fs::read_to_string(&address)
            .map_err(|e| BlockchainError::IoError(format!("Failed to read wallet file: {e}")))?;
        let keypair: Dilithium3Keypair = serde_json::from_str(&file_content)
            .map_err(|e| BlockchainError::SerializationError(format!("Invalid wallet file: {e}")))?;
        let pk_bytes = numi_core::crypto::blake3_hash(&keypair.public_key).to_vec();
        let addr = blockchain.get_address_from_public_key(&pk_bytes);
        (pk_bytes, addr)
    } else if NumiBlockchain::is_valid_address(&address) {
        // Input is Base58 address - find matching public key
        let mut found_pubkey = None;
        for entry in blockchain.get_all_accounts() {
            let (pubkey, _) = entry;
            if blockchain.get_address_from_public_key(&pubkey) == address {
                found_pubkey = Some(pubkey);
                break;
            }
        }
        match found_pubkey {
            Some(pk) => (pk, address.clone()),
            None => {
                log::info!("📍 Address: {}", address);
                log::info!("💰 Balance: 0 NUMI");
                log::info!("");
                log::info!("ℹ️  This address is valid but has no transactions.");
                return Ok(());
            }
        }
    } else {
        // Invalid format
        return Err(BlockchainError::InvalidTransaction(
            format!("Invalid address format: {}. Expected Base58 address, hex public key, or wallet file path.", address)
        ));
    };
 
    // Get balance using public key bytes
    let balance = blockchain.get_balance_by_pubkey(&public_key_bytes);
 
    log::info!("📍 Address: {}", display_address);
    log::info!("💰 Balance: {} NUMI", balance as f64 / 100.0);
    
    // If balance is 0, provide helpful guidance
    if balance == 0 {
        log::info!("");
        log::info!("ℹ️  Balance is 0 NUMI. This could mean:");
        log::info!("   • This address has no funds");
        log::info!("   • The address format is incorrect");
        log::info!("   • You're using a truncated address from logs");
        log::info!("");
        log::info!("💡 If you copied this address from the account list, use:");
        log::info!("   cargo run --release -- accounts --full");
        log::info!("   to get complete 64-character addresses");
    }
    
    // Try to get account state for more details
    if let Ok(account_state) = blockchain.get_account_state(&public_key_bytes) {
        log::info!("🔢 Nonce: {}", account_state.nonce);
        log::info!("📊 Transaction count: {}", account_state.transaction_count);
    }
    
    // Show transaction history if requested
    if history {
        log::info!("\n📜 Transaction History");
        log::info!("=====================");
        
        // Get transactions involving this address (simplified implementation)
        let mut transaction_count = 0;
        let max_history = 10; // Limit to last 10 transactions
        
        // Iterate through recent blocks to find transactions
        let current_height = blockchain.get_current_height();
        let start_height = current_height.saturating_sub(10);
        
        for height in (start_height..=current_height).rev() {
            if let Some(block) = blockchain.get_block_by_height(height) {
                for transaction in block.transactions {
                    // Check if this transaction involves our address
                    let is_sender = public_key_bytes == transaction.from;
                    let is_receiver = match &transaction.transaction_type {
                        TransactionType::Transfer { to, .. } => {
                            public_key_bytes == *to
                        }
                        TransactionType::MiningReward { pool_address, .. } => {
                            if let Some(pool_addr) = pool_address {
                                public_key_bytes == *pool_addr
                            } else {
                                // Mining reward to miner (sender)
                                is_sender
                            }
                        }
                        _ => false,
                    };
                    
                    if is_sender || is_receiver {
                        transaction_count += 1;
                        if transaction_count <= max_history {
                            let direction = if is_sender { "📤 Sent" } else { "📥 Received" };
                            let amount = match &transaction.transaction_type {
                                TransactionType::Transfer { amount, .. } => *amount,
                                TransactionType::MiningReward { amount, .. } => *amount,
                                _ => 0,
                            };
                            
                            log::info!("{} {} NUMI - Block {} - TX: {}", 
                                     direction, 
                                     amount as f64 / 100.0,
                                     height,
                                     transaction.get_hash_hex());
                            
                            if is_sender {
                                log::info!("   💸 Fee: {} NUMI", transaction.fee as f64 / 100.0);
                            }
                        }
                    }
                }
            }
        }
        
        if transaction_count == 0 {
            log::info!("📝 No transactions found for this address");
        } else if transaction_count > max_history {
            log::info!("📋 Showing {max_history} most recent transactions ({transaction_count} total)");
        }
    }
    
    Ok(())
}

async fn init_blockchain_command(config: Config, _force: bool, _genesis_config_path: Option<PathBuf>) -> Result<()> {
    log::info!("🚀 Initializing new Numi blockchain...");
    
    // Create data directory
    std::fs::create_dir_all(&config.storage.data_directory)?;
    log::info!("✅ Created data directory: {:?}", config.storage.data_directory);
    
    // Initialize storage
    let storage = BlockchainStorage::new(&config.storage.data_directory)?;
    log::info!("✅ Storage initialized");
    
    // Use consistent wallet path resolution for genesis miner
    let wallet_path = config.mining.wallet_path.clone();
    let resolved_wallet_path = if wallet_path.is_absolute() {
        wallet_path
    } else {
        config.storage.data_directory.join(&wallet_path)
    };
    
    let miner_keypair = match Dilithium3Keypair::load_from_file(&resolved_wallet_path) {
        Ok(kp) => {
            log::info!("🔑 Loaded existing miner wallet from {:?}", resolved_wallet_path);
            kp
        }
        Err(_) => {
            let kp = Dilithium3Keypair::new()?;
            
            // Ensure parent directory exists
            if let Some(parent) = resolved_wallet_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            
            kp.save_to_file(&resolved_wallet_path)?;
            log::info!("🔑 Generated new miner wallet at {:?}", resolved_wallet_path);
            kp
        }
    };
    
    // Initialize blockchain with specified miner keypair and consensus config
    let blockchain = NumiBlockchain::new_with_config(Some(config.consensus.clone()), Some(miner_keypair))?;
    log::info!("✅ Blockchain initialized with miner wallet {:?}", resolved_wallet_path);
    
    // Save initial state and blocks
    blockchain.save_to_storage(&storage)?;
    log::info!("✅ Initial state and blocks saved");
    
    // Get state for display
    let state = blockchain.get_chain_state();
    
    println!("Numi blockchain initialized successfully");
    println!("Genesis block created");
    println!("Chain height: {}", blockchain.get_current_height());
    println!("Total supply: {} NUMI", state.total_supply as f64 / 100.0);
    
    Ok(())
}

async fn start_rpc_server_command(config: Config) -> Result<()> {
    log::info!("🚀 Starting Numi RPC API server...");

    // Initialize storage and blockchain
    let storage = BlockchainStorage::new(&config.storage.data_directory)?;
    let blockchain = NumiBlockchain::load_from_storage(&storage).await?;
    let blockchain = Arc::new(RwLock::new(blockchain));

    // Initialize network and miner with consistent wallet path resolution
    let network_manager = NetworkManager::new(blockchain.clone())?;
    
    // Use consistent wallet path resolution
    let miner = Miner::with_config_and_data_dir(MiningConfig::default(), config.storage.data_directory.clone())?;
    
    // Create and start RPC server with components
    let network_handle = network_manager.create_handle();
    let rpc_server = RpcServer::with_shared_components(
        blockchain, 
        Arc::new(storage), 
        RateLimitConfig::default(),
        AuthConfig::default(),
        config.rpc.clone(),
        network_handle, 
        Arc::new(RwLock::new(miner))
    )?;
    rpc_server.start(config.rpc.port).await?;
    
    Ok(())
}

async fn generate_key_command(output: PathBuf, format: String) -> Result<()> {
    log::info!("🔑 Generating new key pair...");
    
    let keypair = Dilithium3Keypair::new()?;
    
    let file_content = match format.to_lowercase().as_str() {
        "pem" => {
            let pem = keypair.to_pem()?;
            format!("-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----\n-----BEGIN PUBLIC KEY-----\n{}\n-----END PUBLIC KEY-----", pem.private_key, pem.public_key)
        }
        "json" => {
            
            serde_json::to_string(&keypair)?
        }
        _ => {
            return Err(numi_core::BlockchainError::InvalidArgument(format!("Unsupported key format: {format}")));
        }
    };
    
    std::fs::write(&output, file_content)?;
    log::info!("✅ Key pair generated and saved to {output:?}");
    
    Ok(())
}

async fn create_config_command(output: PathBuf, env: Environment) -> Result<()> {
    log::info!("🔧 Creating default configuration file...");
    
    let config = create_default_config(&env)?;
    
    let config_content = match config.save_to_file(&output) {
        Ok(_) => {
            log::info!("✅ Configuration file created at {output:?}");
            toml::to_string_pretty(&config).unwrap_or_else(|_| format!("{config:#?}"))
        }
        Err(e) => {
            log::error!("❌ Failed to save configuration: {e}");
            return Err(BlockchainError::IoError(e.to_string()));
        }
    };
    
    log::info!("📄 Configuration content:\n{config_content}");
    
    Ok(())
}

async fn backup_command(config: Config, output: PathBuf, compress: bool) -> Result<()> {
     use tokio::task::spawn_blocking;
     log::info!("💾 Backing up blockchain data...");
 
     let storage = BlockchainStorage::new(&config.storage.data_directory)?;
 
    // Do the initial backup synchronously (quick metadata copy)
    storage.backup_to_directory(&output)?;
 
    log::info!("✅ Backup completed successfully to {output:?}");
 
    if compress {
        let compressed_path = output.with_extension("tar.gz");
        let compressed_path_clone = compressed_path.clone();
        let out_dir = output.clone();
        spawn_blocking(move || -> Result<()> {
            let tar_gz = std::fs::File::create(&compressed_path_clone)?;
            let enc = flate2::write::GzEncoder::new(tar_gz, flate2::Compression::default());
            let mut tar = tar::Builder::new(enc);
            tar.append_dir_all(".", &out_dir)?;
            tar.finish()?;

            // Ensure archive exists before deleting raw backup
            if compressed_path_clone.exists() {
                std::fs::remove_dir_all(&out_dir)?;
            }
            Ok(())
        }).await??;

        log::info!("🗜️ Backup compressed to {compressed_path:?}");
    }
 
    Ok(())
}

async fn restore_command(config: Config, input: PathBuf, verify: bool) -> Result<()> {
    use tokio::task::spawn_blocking;
    log::info!("📥 Restoring blockchain data from backup...");

    // Clone inputs for blocking task
    let cfg_clone = config.clone();
    spawn_blocking(move || -> Result<()> {
        let is_compressed = input.extension().and_then(|s| s.to_str()) == Some("gz");

        // ---------------- Extract if needed ----------------
        let restore_path = if is_compressed {
            log::info!("🗜️ Extracting compressed backup...");
            let tar_gz = std::fs::File::open(&input)?;
            let dec = flate2::read::GzDecoder::new(tar_gz);
            let mut archive = tar::Archive::new(dec);
            let temp_dir = std::env::temp_dir().join("numi_restore");
            std::fs::create_dir_all(&temp_dir)?;
            archive.unpack(&temp_dir)?;
            temp_dir
                } else {
            input.clone()
        };

        if !restore_path.exists() {
            return Err(BlockchainError::InvalidArgument(format!("Backup path not found: {restore_path:?}")));
        }

        // --------------- Optional verification -------------
        if verify {
            log::info!("🔍 Verifying backup integrity...");
            let essential_files = ["blocks", "transactions", "accounts", "chain_state"];
            for file in essential_files {
                let file_path = restore_path.join(file);
                if !file_path.exists() {
                    return Err(BlockchainError::InvalidBackup(format!("Essential file missing: {file}")));
                }
            }
            log::info!("✅ Backup integrity verified");
        }

        // --------------- Backup current data ---------------
        let backup_current = cfg_clone.storage.data_directory.with_extension("backup_before_restore");
        if cfg_clone.storage.data_directory.exists() {
            log::info!("💾 Creating backup of current data...");
            std::fs::create_dir_all(&backup_current)?;
            for entry in std::fs::read_dir(&cfg_clone.storage.data_directory)? {
                let entry = entry?;
                let src = entry.path();
                let dst = backup_current.join(entry.file_name());
                if src.is_dir() {
                    std::fs::create_dir_all(&dst)?;
                } else {
                    std::fs::copy(&src, &dst)?;
                }
            }
        }

        // ------------------ Restore ------------------------
        log::info!("📁 Restoring data to {:?}...", cfg_clone.storage.data_directory);
        std::fs::create_dir_all(&cfg_clone.storage.data_directory)?;
        for entry in std::fs::read_dir(&restore_path)? {
            let entry = entry?;
            let src = entry.path();
            let dst = cfg_clone.storage.data_directory.join(entry.file_name());
            if src.is_dir() {
                std::fs::create_dir_all(&dst)?;
                copy_dir_recursive(&src, &dst)?;
            } else {
                std::fs::copy(&src, &dst)?;
            }
        }

        log::info!("✅ Restore completed successfully");
        log::info!("📝 Previous data backed up to {backup_current:?}");

        // Clean up temporary extraction dir
        if is_compressed {
            let _ = std::fs::remove_dir_all(&restore_path);
        }

        Ok(())
    }).await??;

    Ok(())
}

/// Recursively copy directory contents
fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    
    Ok(())
}


