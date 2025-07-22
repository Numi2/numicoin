use clap::{Parser, Subcommand};
use numi_core::{
    blockchain::NumiBlockchain,
    storage::BlockchainStorage,
    miner::Miner,
    network::NetworkManager,
    crypto::Dilithium3Keypair,
    transaction::{Transaction, TransactionType},
    rpc::RpcServer,
    Result,
};
use std::path::PathBuf;
use tokio;

#[derive(Parser)]
#[command(name = "numi-node")]
#[command(about = "Numi blockchain node - Quantum-safe cryptocurrency")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
    
    #[arg(long, default_value = "./data")]
    data_dir: PathBuf,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the blockchain node
    Start {
        #[arg(long, default_value = "8080")]
        port: u16,
        
        #[arg(long)]
        listen_addr: Option<String>,
    },
    
    /// Mine a new block
    Mine {
        #[arg(long)]
        miner_key: Option<String>,
    },
    
    /// Submit a transaction
    Submit {
        #[arg(long)]
        from: String,
        
        #[arg(long)]
        to: String,
        
        #[arg(long)]
        amount: u64,
    },
    
    /// Get blockchain status
    Status,
    
    /// Get account balance
    Balance {
        #[arg(long)]
        address: String,
    },
    
    /// Initialize a new blockchain
    Init,
    
    /// Start RPC API server
    Rpc {
        #[arg(long, default_value = "8080")]
        port: u16,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    
    let cli = Cli::parse();
    
    match cli.command {
        Commands::Start { port, listen_addr } => {
            start_node(cli.data_dir, port, listen_addr).await?;
        }
        Commands::Mine { miner_key } => {
            mine_block(cli.data_dir, miner_key).await?;
        }
        Commands::Submit { from, to, amount } => {
            submit_transaction(cli.data_dir, from, to, amount).await?;
        }
        Commands::Status => {
            show_status(cli.data_dir).await?;
        }
        Commands::Balance { address } => {
            show_balance(cli.data_dir, address).await?;
        }
        Commands::Init => {
            init_blockchain(cli.data_dir).await?;
        }
        Commands::Rpc { port } => {
            start_rpc_server(cli.data_dir, port).await?;
        }
    }
    
    Ok(())
}

async fn start_node(data_dir: PathBuf, _port: u16, listen_addr: Option<String>) -> Result<()> {
    println!("🚀 Starting Numi blockchain node...");
    
    // Initialize storage
    let _storage = BlockchainStorage::new(&data_dir)?;
    println!("✅ Storage initialized at {:?}", data_dir);
    
    // Initialize blockchain
    let blockchain = NumiBlockchain::new()?;
    println!("✅ Blockchain initialized");
    
    // Initialize network with libp2p
    let mut network = NetworkManager::new()?;
    let network_addr = listen_addr.unwrap_or_else(|| "/ip4/0.0.0.0/tcp/0".to_string());
    network.start(&network_addr).await?;
    println!("✅ Network started on {}", network_addr);
    
    // Initialize miner
    let _miner = Miner::new()?;
    println!("✅ Miner initialized");
    
    println!("🎯 Node is running! Press Ctrl+C to stop.");
    println!("📊 Chain height: {}", blockchain.get_current_height());
    println!("🔗 Connected peers: {}", network.get_peer_count().await);
    
    // Keep the node running
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
        
        // Print periodic status
        let state = blockchain.get_chain_state();
        println!("📈 Status - Blocks: {}, Difficulty: {}, Supply: {} NUMI", 
                state.total_blocks, state.current_difficulty, state.total_supply);
    }
}

async fn mine_block(data_dir: PathBuf, miner_key: Option<String>) -> Result<()> {
    println!("⛏️ Starting mining operation...");
    
    // Initialize storage and blockchain
    let storage = BlockchainStorage::new(&data_dir)?;
    let blockchain = NumiBlockchain::load_from_storage(&storage).await?;
    
    // Create or load miner keypair
    let keypair = if let Some(_key_str) = miner_key {
        // In a real implementation, you'd load the keypair from the string
        Dilithium3Keypair::new()?
    } else {
        Dilithium3Keypair::new()?
    };
    
    println!("🔑 Mining with public key: {}", hex::encode(&keypair.public_key));
    
    // Get pending transactions
    let pending_txs = blockchain.get_transactions_for_block(1_000_000, 1000);
    println!("📝 Found {} pending transactions", pending_txs.len());
    
    // Start mining
    let mut miner = Miner::new()?;
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
            println!("🎉 Block mined successfully!");
            println!("📊 Block height: {}", result.block.header.height);
            println!("🔢 Nonce: {}", result.nonce);
            println!("⏱️ Mining time: {:?}", mining_time);
            println!("⚡ Hash rate: {} H/s", result.hash_rate);
            
            // Add block to blockchain
            blockchain.add_block(result.block).await?;
            
            // Save to storage
            blockchain.save_to_storage(&storage)?;
            println!("✅ Block added to blockchain and saved to storage");
        }
        Ok(None) => {
            println!("⏹️ Mining stopped");
        }
        Err(e) => {
            println!("❌ Mining failed: {}", e);
        }
    }
    
    Ok(())
}

async fn submit_transaction(data_dir: PathBuf, _from: String, to: String, amount: u64) -> Result<()> {
    println!("📤 Submitting transaction...");
    
    // Initialize storage and blockchain
    let storage = BlockchainStorage::new(&data_dir)?;
    let blockchain = NumiBlockchain::load_from_storage(&storage).await?;
    
    // Create keypair for sender (in real implementation, load from wallet)
    let sender_keypair = Dilithium3Keypair::new()?;
    
    // Parse recipient address (in real implementation, validate format)
    let recipient_pubkey = hex::decode(&to)
        .map_err(|e| numi_core::BlockchainError::InvalidTransaction(format!("Invalid recipient address: {}", e)))?;
    
    // Create transaction
    let mut transaction = Transaction::new(
        sender_keypair.public_key.clone(),
        TransactionType::Transfer {
            to: recipient_pubkey,
            amount,
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

async fn show_status(data_dir: PathBuf) -> Result<()> {
    println!("📊 Blockchain Status");
    println!("==================");
    
    // Initialize storage and blockchain
    let storage = BlockchainStorage::new(&data_dir)?;
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
        println!("🔗 Latest block hash: {}", latest_block.get_hash_hex());
        println!("📝 Latest block transactions: {}", latest_block.get_transaction_count());
    } else {
        println!("🔗 No blocks found");
    }
    
    // Get pending transactions
    let pending_txs = blockchain.get_pending_transaction_count();
    println!("⏳ Pending transactions: {}", pending_txs);
    
    Ok(())
}

async fn show_balance(data_dir: PathBuf, address: String) -> Result<()> {
    println!("💰 Account Balance");
    println!("=================");
    
    // Initialize storage and blockchain
    let storage = BlockchainStorage::new(&data_dir)?;
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
        println!("🔒 Staked amount: {} NUMI", account_state.staked_amount as f64 / 1_000_000_000.0);
    }
    
    Ok(())
}

async fn init_blockchain(data_dir: PathBuf) -> Result<()> {
    println!("🚀 Initializing new Numi blockchain...");
    
    // Create data directory
    std::fs::create_dir_all(&data_dir)?;
    println!("✅ Created data directory: {:?}", data_dir);
    
    // Initialize storage
    let storage = BlockchainStorage::new(&data_dir)?;
    println!("✅ Storage initialized");
    
    // Initialize blockchain
    let blockchain = NumiBlockchain::new()?;
    println!("✅ Blockchain initialized");
    
    // Save initial state
    let state = blockchain.get_chain_state();
    storage.save_chain_state(&state)?;
    println!("✅ Initial state saved");
    
    println!("🎉 Numi blockchain initialized successfully!");
    println!("📊 Genesis block created");
    println!("🔗 Chain height: {}", blockchain.get_current_height());
    println!("💰 Total supply: {} NUMI", state.total_supply as f64 / 1_000_000_000.0);
    
    Ok(())
}

async fn start_rpc_server(data_dir: PathBuf, port: u16) -> Result<()> {
    println!("🚀 Starting Numi RPC API server...");
    
    // Initialize storage and blockchain
    let storage = BlockchainStorage::new(&data_dir)?;
    let blockchain = NumiBlockchain::load_from_storage(&storage).await?;
    
    // Initialize network and miner
    let network_manager = NetworkManager::new()?;
    let miner = Miner::new()?;
    
    // Create and start RPC server with components
    let rpc_server = RpcServer::with_components(blockchain, storage, network_manager, miner)?;
    rpc_server.start(port).await?;
    
    Ok(())
}
