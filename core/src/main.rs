use clap::{Parser, Subcommand};
use numi_core::{
    blockchain::NumiBlockchain,
    storage::BlockchainStorage,
    miner::Miner,
    network::NetworkManager,
    crypto::Dilithium3Keypair,
    transaction::{Transaction, TransactionType},
    rpc::{RpcServer, RateLimitConfig, AuthConfig},
    config::Config,
    BlockchainError,
    Result,
};
use std::path::PathBuf;
use tokio;
use fs2::FileExt;
use hex;
use bincode;

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
        eprintln!("❌ Configuration validation failed: {}", e);
        std::process::exit(1);
    }

    // ------------------------------------------------------------------
    // Acquire exclusive lock on the data directory to avoid double opens
    // ------------------------------------------------------------------
    let _data_dir_lock = match acquire_data_dir_lock(&config.storage.data_directory) {
        Ok(lock) => lock,
        Err(e) => {
            eprintln!("❌ Failed to acquire data directory lock: {}", e);
            std::process::exit(1);
        }
    };
 
    log::info!("🚀 NumiCoin Node v1.0.0 starting...");
    log::info!("🔧 Environment: {:?}", cli.environment);
    log::info!("📁 Data directory: {:?}", config.storage.data_directory);
    
    match cli.command {
        Commands::Start { rpc_port, listen_addr, enable_mining, mining_threads } => {
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
                log::error!("❌ Failed to load configuration: {}", e);
                log::info!("🔧 Creating default configuration...");
                create_default_config(&cli.environment)
            }
        }
    } else {
        log::info!("🔧 Configuration file not found, creating default...");
        let config = create_default_config(&cli.environment)?;
        
        // Save the default configuration
        if let Err(e) = config.save_to_file(&cli.config) {
            log::warn!("⚠️ Failed to save default configuration: {}", e);
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
    let initial_chain = match NumiBlockchain::load_from_storage(&*storage).await {
        Ok(chain) => chain,
        Err(_) => {
            log::warn!("🆕 No existing chain found – creating new genesis");
            NumiBlockchain::new()?
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
        let miner = std::sync::Arc::new(parking_lot::RwLock::new(Miner::new()?));

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
    let mut network = NetworkManager::new()?;
    let network_addr = format!("/ip4/{}/tcp/{}", config.network.listen_address, config.network.listen_port);
    network.start(&network_addr).await?;
    log::info!("✅ Network started on {}", network_addr);

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
            network_handle_clone,
            miner,
        )?;
        
        tokio::spawn(async move {
            if let Err(e) = rpc_server.start(port).await {
                log::error!("❌ RPC server failed: {}", e);
            }
        });
        
        log::info!(
            "🚀 RPC API server spawned in background on {}:{}",
            bind_addr,
            port
        );
    }

    // ----------------------- Miner -------------------------------
    log::info!("🎯 Node is running! Press Ctrl+C to stop.");

    // Spawn background mining loop if enabled
    if config.mining.enabled {
        let blockchain_clone = blockchain.clone();
        let _storage_clone = storage.clone();
        let network_handle_clone = network_handle.clone();
        let mining_cfg = config.mining.clone();
        let target_block_time = config.consensus.target_block_time;
        tokio::spawn(async move {
            loop {
                // Snapshot chain state
                let height = blockchain_clone.read().get_current_height();
                let previous_hash = blockchain_clone.read().get_latest_block_hash();
                let difficulty = blockchain_clone.read().get_current_difficulty();
                let pending_txs = blockchain_clone.read().get_transactions_for_block(1_000_000, 1000);
                // Clone parameters for blocking closure
                let mining_cfg_clone = mining_cfg.clone();
                let height_clone = height;
                let previous_hash_clone = previous_hash;
                let difficulty_clone = difficulty;
                let pending_txs_clone = pending_txs.clone();
                // Perform mining in a blocking task
                if let Ok(Ok(Some(result))) = tokio::task::spawn_blocking(move || {
                    let mut miner = Miner::new().unwrap();
                    miner.update_config(mining_cfg_clone.into());
                    miner.mine_block(
                        height_clone + 1,
                        previous_hash_clone,
                        pending_txs_clone,
                        difficulty_clone,
                        0,
                    )
                }).await
                {
                    // On success, add and persist block, then broadcast
                    let block = result.block.clone();
                    let block_hash = hex::encode(block.calculate_hash().unwrap_or_default());
                    log::info!("⛏️ Mined block {} with hash {}", 
                        block.header.height, 
                        block_hash
                    );
                    
                    // Add the mined block to the blockchain using spawn_blocking to avoid Send trait issues
                    let blockchain_clone_for_blocking = blockchain_clone.clone();
                    let block_clone = block.clone();
                    let add_block_result = tokio::task::spawn_blocking(move || {
                        // Use futures::executor::block_on to run the async add_block method
                        futures::executor::block_on(async {
                            blockchain_clone_for_blocking.write().add_block(block_clone).await
                        })
                    }).await;
                    
                    match add_block_result {
                        Ok(Ok(true)) => {
                            log::info!("✅ Successfully added mined block {} to blockchain", block.header.height);
                            // Broadcast the block to the network
                            let _ = network_handle_clone.broadcast_block(block).await;
                        }
                        Ok(Ok(false)) => {
                            log::warn!("⚠️ Mined block {} was already in blockchain", block.header.height);
                        }
                        Ok(Err(e)) => {
                            log::error!("❌ Failed to add mined block {} to blockchain: {}", block.header.height, e);
                        }
                        Err(e) => {
                            log::error!("❌ Failed to spawn blocking task for block {}: {}", block.header.height, e);
                        }
                    }
                }
                // Wait for the configured block time before mining next block
                time::sleep(target_block_time).await;
            }
        });
    }

    // Periodic status & graceful shutdown handling
    let mut status_interval = time::interval(Duration::from_secs(10));

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                log::info!("🛑 Ctrl+C received – beginning graceful shutdown");

                // Flush blockchain state
                if let Err(e) = blockchain.read().save_to_storage(&*storage) {
                    log::error!("❌ Failed to persist chain state: {}", e);
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
    
    // Create or load miner keypair
    let keypair = if let Some(path) = miner_key_path {
        // In a real implementation, you'd load the keypair from the string
        Dilithium3Keypair::load_from_file(&path)?
    } else {
        Dilithium3Keypair::new()?
    };
    
    log::info!("🔑 Mining with public key: {}", hex::encode(&keypair.public_key));
    
    // Get pending transactions
    let pending_txs = blockchain.get_transactions_for_block(1_000_000, 1000);
    log::info!("📝 Found {} pending transactions", pending_txs.len());
    
    // Start mining
    let mut miner = Miner::new()?;
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
            log::info!("⏱️ Mining time: {:?}", mining_time);
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
            log::error!("❌ Mining failed: {}", e);
        }
    }
    
    Ok(())
}

#[allow(unused_variables)]
async fn submit_transaction_command(config: Config, from_key_path: PathBuf, to: String, amount: u64, _: Option<u64>, memo: Option<String>) -> Result<()> {
    println!("📤 Submitting transaction...");
    
    // Initialize storage and blockchain
    let storage = BlockchainStorage::new(&config.storage.data_directory)?;
    let blockchain = NumiBlockchain::load_from_storage(&storage).await?;
    
    // Create keypair for sender (in real implementation, load from wallet)
    let sender_keypair = Dilithium3Keypair::load_from_file(&from_key_path)?;
    
    // Parse recipient address (in real implementation, validate format)
    let recipient_pubkey = hex::decode(&to)
        .map_err(|e| numi_core::BlockchainError::InvalidTransaction(format!("Invalid recipient address: {}", e)))?;
    
    // Create transaction
    let mut transaction = Transaction::new(
        sender_keypair.public_key.clone(),
        TransactionType::Transfer {
            to: recipient_pubkey,
            amount,
            memo,
        },
        1, // Nonce - in real implementation, get from account state
    );
    
    // Sign transaction
    transaction.sign(&sender_keypair)?;
    
    // Submit transaction
    blockchain.add_transaction(transaction.clone()).await?;
    
    println!("✅ Transaction submitted successfully!");
    println!("🆔 Transaction ID: {}", transaction.get_hash_hex());
    println!("📤 From: {}", hex::encode(&sender_keypair.public_key));
    println!("📥 To: {}", to);
    println!("💰 Amount: {} NUMI", amount as f64 / 1_000_000_000.0);
    
    Ok(())
}

async fn sign_transaction_command(key_path: PathBuf, to: String, amount: u64, nonce: u64) -> Result<()> {
    // Load keypair and build transaction
    let keypair = Dilithium3Keypair::load_from_file(&key_path)?;
    let recipient = hex::decode(&to)
        .map_err(|e| numi_core::BlockchainError::InvalidTransaction(format!("Invalid recipient hex: {}", e)))?;
    let mut tx = Transaction::new(
        keypair.public_key.clone(),
        TransactionType::Transfer { to: recipient, amount, memo: None },
        nonce,
    );
    // Sign and serialize signature
    tx.sign(&keypair)?;
    let sig = tx.signature.as_ref().ok_or_else(|| numi_core::BlockchainError::CryptographyError("Missing signature".to_string()))?;
    let sig_bytes = bincode::serialize(sig).map_err(|e| numi_core::BlockchainError::CryptographyError(format!("Serialize error: {}", e)))?;
    println!("{}", hex::encode(sig_bytes));
    Ok(())
}

async fn show_status_command(config: Config, _detailed: bool, _format: OutputFormat) -> Result<()> {
    println!("📊 Blockchain Status");
    println!("==================");
    
    // Initialize storage and blockchain
    let storage = BlockchainStorage::new(&config.storage.data_directory)?;
    let blockchain = NumiBlockchain::load_from_storage(&storage).await?;
    
    // Get chain state
    let state = blockchain.get_chain_state();
    
    println!("📈 Total blocks: {}", state.total_blocks);
    println!("💰 Total supply: {} NUMI", state.total_supply as f64 / 1_000_000_000.0);
    println!("🎯 Current difficulty: {}", state.current_difficulty);
    println!("⏱️ Average block time: {} seconds", state.average_block_time);
    println!("🕐 Last block time: {}", state.last_block_time);
    println!("⛏️ Active miners: {}", state.active_miners);
    
    // Get latest block info
    if let Some(latest_block) = blockchain.get_latest_block() {
        println!("🔗 Latest block hash: {}", latest_block.get_hash_hex()?);
        println!("📝 Latest block transactions: {}", latest_block.get_transaction_count());
    } else {
        println!("🔗 No blocks found");
    }
    
    // Get pending transactions
    let pending_txs = blockchain.get_pending_transaction_count();
    println!("⏳ Pending transactions: {}", pending_txs);
    
    Ok(())
}

async fn show_balance_command(config: Config, address: String, _history: bool) -> Result<()> {
    println!("💰 Account Balance");
    println!("=================");
    
    // Initialize storage and blockchain
    let storage = BlockchainStorage::new(&config.storage.data_directory)?;
    let blockchain = NumiBlockchain::load_from_storage(&storage).await?;
    
    // Parse address
    let pubkey = hex::decode(&address)
        .map_err(|e| numi_core::BlockchainError::InvalidTransaction(format!("Invalid address: {}", e)))?;
    
    // Get balance
    let balance = blockchain.get_balance(&pubkey);
    
    println!("📍 Address: {}", address);
    println!("💰 Balance: {} NUMI", balance as f64 / 1_000_000_000.0);
    
    // Try to get account state for more details
    if let Ok(account_state) = blockchain.get_account_state(&pubkey) {
        println!("🔢 Nonce: {}", account_state.nonce);
        println!("📊 Transaction count: {}", account_state.transaction_count);
    }
    
    Ok(())
}

async fn init_blockchain_command(config: Config, _force: bool, _genesis_config_path: Option<PathBuf>) -> Result<()> {
    println!("🚀 Initializing new Numi blockchain...");
    
    // Create data directory
    std::fs::create_dir_all(&config.storage.data_directory)?;
    println!("✅ Created data directory: {:?}", config.storage.data_directory);
    
    // Initialize storage
    let storage = BlockchainStorage::new(&config.storage.data_directory)?;
    println!("✅ Storage initialized");
    
    // Initialize blockchain
    let blockchain = NumiBlockchain::new()?;
    println!("✅ Blockchain initialized");
    
    // Save initial state and blocks
    blockchain.save_to_storage(&storage)?;
    println!("✅ Initial state and blocks saved");
    
    // Get state for display
    let state = blockchain.get_chain_state();
    
    println!("🎉 Numi blockchain initialized successfully!");
    println!("📊 Genesis block created");
    println!("🔗 Chain height: {}", blockchain.get_current_height());
    println!("💰 Total supply: {} NUMI", state.total_supply as f64 / 1_000_000_000.0);
    
    Ok(())
}

async fn start_rpc_server_command(config: Config) -> Result<()> {
    println!("🚀 Starting Numi RPC API server...");
    
    // Initialize storage and blockchain
    let storage = BlockchainStorage::new(&config.storage.data_directory)?;
    let blockchain = NumiBlockchain::load_from_storage(&storage).await?;
    
    // Initialize network and miner
    let network_manager = NetworkManager::new()?;
    let miner = Miner::new()?;
    
    // Create and start RPC server with components
    let rpc_server = RpcServer::with_components(blockchain, storage, network_manager, miner)?;
    rpc_server.start(config.rpc.port).await?;
    
    Ok(())
}

async fn generate_key_command(output: PathBuf, format: String) -> Result<()> {
    println!("🔑 Generating new key pair...");
    
    let keypair = Dilithium3Keypair::new()?;
    
    let file_content = match format.to_lowercase().as_str() {
        "pem" => {
            let pem = keypair.to_pem()?;
            format!("-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----\n-----BEGIN PUBLIC KEY-----\n{}\n-----END PUBLIC KEY-----", pem.private_key, pem.public_key)
        }
        "json" => {
            let json = serde_json::to_string(&keypair)?;
            json
        }
        _ => {
            return Err(numi_core::BlockchainError::InvalidArgument(format!("Unsupported key format: {}", format)).into());
        }
    };
    
    std::fs::write(&output, file_content)?;
    println!("✅ Key pair generated and saved to {:?}", output);
    
    Ok(())
}

async fn create_config_command(output: PathBuf, env: Environment) -> Result<()> {
    println!("🔧 Creating default configuration file...");
    
    let config = create_default_config(&env)?;
    
    let config_content = match config.save_to_file(&output) {
        Ok(_) => {
            println!("✅ Configuration file created at {:?}", output);
            toml::to_string_pretty(&config).unwrap_or_else(|_| format!("{:#?}", config))
        }
        Err(e) => {
            println!("❌ Failed to save configuration: {}", e);
            return Err(BlockchainError::IoError(e.to_string()));
        }
    };
    
    println!("📄 Configuration content:\n{}", config_content);
    
    Ok(())
}

async fn backup_command(config: Config, output: PathBuf, compress: bool) -> Result<()> {
     use tokio::task::spawn_blocking;
     log::info!("💾 Backing up blockchain data...");
 
     let storage = BlockchainStorage::new(&config.storage.data_directory)?;
 
    // Do the initial backup synchronously (quick metadata copy)
    storage.backup_to_directory(&output)?;
 
    log::info!("✅ Backup completed successfully to {:?}", output);
 
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

        log::info!("🗜️ Backup compressed to {:?}", compressed_path);
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
            return Err(BlockchainError::InvalidArgument(format!("Backup path not found: {:?}", restore_path)));
        }

        // --------------- Optional verification -------------
        if verify {
            log::info!("🔍 Verifying backup integrity...");
            let essential_files = ["blocks", "transactions", "accounts", "chain_state"];
            for file in essential_files {
                let file_path = restore_path.join(file);
                if !file_path.exists() {
                    return Err(BlockchainError::InvalidBackup(format!("Essential file missing: {}", file)));
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
        log::info!("📝 Previous data backed up to {:?}", backup_current);

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
