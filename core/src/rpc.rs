use std::sync::Arc;
use tokio::sync::RwLock;
use warp::{Filter, Reply, Rejection, http::StatusCode};
use serde::{Deserialize, Serialize};
use crate::blockchain::NumiBlockchain;
use crate::storage::BlockchainStorage;
use crate::transaction::{Transaction, TransactionType};
use crate::crypto::Dilithium3Keypair;

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    success: bool,
    data: Option<T>,
    error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StatusResponse {
    total_blocks: u64,
    total_supply: f64,
    current_difficulty: u32,
    latest_block_hash: String,
    pending_transactions: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BalanceResponse {
    address: String,
    balance: f64,
    nonce: u64,
    staked_amount: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BlockResponse {
    height: u64,
    hash: String,
    timestamp: String,
    transactions: usize,
    difficulty: u32,
    nonce: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TransactionRequest {
    from: String,
    to: String,
    amount: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TransactionResponse {
    id: String,
    from: String,
    to: String,
    amount: f64,
    status: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MiningResponse {
    message: String,
    block_height: u64,
    hash: String,
}

pub struct RpcServer {
    blockchain: Arc<RwLock<NumiBlockchain>>,
    storage: Arc<BlockchainStorage>,
}

impl RpcServer {
    pub fn new(blockchain: NumiBlockchain, storage: BlockchainStorage) -> Self {
        Self {
            blockchain: Arc::new(RwLock::new(blockchain)),
            storage: Arc::new(storage),
        }
    }
    
    pub async fn start(self, port: u16) -> crate::Result<()> {
        let rpc_server = Arc::new(self);
        
        // API routes
        let status_route = warp::path("status")
            .and(warp::get())
            .and(with_rpc_server(rpc_server.clone()))
            .and_then(handle_status);
            
        let balance_route = warp::path("balance")
            .and(warp::path::param())
            .and(warp::get())
            .and(with_rpc_server(rpc_server.clone()))
            .and_then(handle_balance);
            
        let block_route = warp::path("block")
            .and(warp::path::param())
            .and(warp::get())
            .and(with_rpc_server(rpc_server.clone()))
            .and_then(handle_block);
            
        let transaction_route = warp::path("transaction")
            .and(warp::post())
            .and(warp::body::json())
            .and(with_rpc_server(rpc_server.clone()))
            .and_then(handle_transaction);
            
        let mine_route = warp::path("mine")
            .and(warp::post())
            .and(with_rpc_server(rpc_server.clone()))
            .and_then(handle_mine);
        
        // Combine routes
        let routes = status_route
            .or(balance_route)
            .or(block_route)
            .or(transaction_route)
            .or(mine_route)
            .with(warp::cors().allow_any_origin());
        
        println!("🚀 Starting RPC server on port {}", port);
        println!("📡 Available endpoints:");
        println!("   GET  /status");
        println!("   GET  /balance/:address");
        println!("   GET  /block/:hash");
        println!("   POST /transaction");
        println!("   POST /mine");
        
        warp::serve(routes)
            .run(([127, 0, 0, 1], port))
            .await;
        
        Ok(())
    }
}

fn with_rpc_server(
    rpc_server: Arc<RpcServer>,
) -> impl Filter<Extract = (Arc<RpcServer>,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || rpc_server.clone())
}

async fn handle_status(
    rpc_server: Arc<RpcServer>,
) -> std::result::Result<warp::reply::Json, Rejection> {
    let blockchain = rpc_server.blockchain.read().await;
    let state = blockchain.get_chain_state();
    let latest_block = blockchain.get_latest_block();
    let pending_txs = blockchain.get_pending_transactions();
    
    let response = StatusResponse {
        total_blocks: state.total_blocks,
        total_supply: state.total_supply as f64 / 1_000_000_000.0,
        current_difficulty: state.current_difficulty,
        latest_block_hash: latest_block.get_hash_hex(),
        pending_transactions: pending_txs.len(),
    };
    
    Ok(warp::reply::json(&ApiResponse {
        success: true,
        data: Some(response),
        error: None,
    }))
}

async fn handle_balance(
    address: String,
    rpc_server: Arc<RpcServer>,
) -> std::result::Result<warp::reply::Json, Rejection> {
    let blockchain = rpc_server.blockchain.read().await;
    
    // Parse address
    let pubkey = match hex::decode(&address) {
        Ok(key) => key,
        Err(_) => {
            return Ok(warp::reply::json(&ApiResponse::<BalanceResponse> {
                success: false,
                data: None,
                error: Some("Invalid address format".to_string()),
            }));
        }
    };
    
    let balance = blockchain.get_balance(&pubkey);
    
    // Try to get account state for more details
    let (nonce, staked_amount) = if let Ok(account_state) = blockchain.get_account_state(&pubkey) {
        (account_state.nonce, account_state.staked_amount)
    } else {
        (0, 0)
    };
    
    let response = BalanceResponse {
        address,
        balance: balance as f64 / 1_000_000_000.0,
        nonce,
        staked_amount: staked_amount as f64 / 1_000_000_000.0,
    };
    
    Ok(warp::reply::json(&ApiResponse {
        success: true,
        data: Some(response),
        error: None,
    }))
}

async fn handle_block(
    hash: String,
    rpc_server: Arc<RpcServer>,
) -> std::result::Result<warp::reply::Json, Rejection> {
    let blockchain = rpc_server.blockchain.read().await;
    let chain = blockchain.get_chain();
    
    // Find block by hash
    let block = chain.iter().find(|b| b.get_hash_hex() == hash);
    
    match block {
        Some(block) => {
            let response = BlockResponse {
                height: block.header.height,
                hash: block.get_hash_hex(),
                timestamp: block.header.timestamp.to_rfc3339(),
                transactions: block.get_transaction_count(),
                difficulty: block.header.difficulty,
                nonce: block.header.nonce,
            };
            
            Ok(warp::reply::json(&ApiResponse {
                success: true,
                data: Some(response),
                error: None,
            }))
        }
        None => {
            Ok(warp::reply::json(&ApiResponse::<BlockResponse> {
                success: false,
                data: None,
                error: Some("Block not found".to_string()),
            }))
        }
    }
}

async fn handle_transaction(
    tx_request: TransactionRequest,
    rpc_server: Arc<RpcServer>,
) -> std::result::Result<warp::reply::Json, Rejection> {
    let mut blockchain = rpc_server.blockchain.write().await;
    
    // Create keypair for sender (in real implementation, load from wallet)
    let sender_keypair = match Dilithium3Keypair::new() {
        Ok(keypair) => keypair,
        Err(_) => {
            return Ok(warp::reply::json(&ApiResponse::<TransactionResponse> {
                success: false,
                data: None,
                error: Some("Failed to create keypair".to_string()),
            }));
        }
    };
    
    // Parse recipient address
    let recipient_pubkey = match hex::decode(&tx_request.to) {
        Ok(key) => key,
        Err(_) => {
            return Ok(warp::reply::json(&ApiResponse::<TransactionResponse> {
                success: false,
                data: None,
                error: Some("Invalid recipient address".to_string()),
            }));
        }
    };
    
    // Create transaction
    let mut transaction = Transaction::new(
        sender_keypair.public_key.clone(),
        TransactionType::Transfer {
            to: recipient_pubkey,
            amount: tx_request.amount,
        },
        1, // Nonce - in real implementation, get from account state
    );
    
    // Sign transaction
    if let Err(_) = transaction.sign(&sender_keypair) {
        return Ok(warp::reply::json(&ApiResponse::<TransactionResponse> {
            success: false,
            data: None,
            error: Some("Failed to sign transaction".to_string()),
        }));
    }
    
    // Submit transaction
    if let Err(_) = blockchain.add_transaction(transaction.clone()) {
        return Ok(warp::reply::json(&ApiResponse::<TransactionResponse> {
            success: false,
            data: None,
            error: Some("Failed to add transaction".to_string()),
        }));
    }
    
    // Save to storage
    if let Err(_) = blockchain.save_to_storage(&rpc_server.storage) {
        return Ok(warp::reply::json(&ApiResponse::<TransactionResponse> {
            success: false,
            data: None,
            error: Some("Failed to save transaction".to_string()),
        }));
    }
    
    let response = TransactionResponse {
        id: transaction.get_hash_hex(),
        from: hex::encode(&sender_keypair.public_key),
        to: tx_request.to,
        amount: tx_request.amount as f64 / 1_000_000_000.0,
        status: "pending".to_string(),
    };
    
    Ok(warp::reply::json(&ApiResponse {
        success: true,
        data: Some(response),
        error: None,
    }))
}

async fn handle_mine(
    rpc_server: Arc<RpcServer>,
) -> std::result::Result<warp::reply::Json, Rejection> {
    let mut blockchain = rpc_server.blockchain.write().await;
    
    // Create miner keypair
    let miner_keypair = match Dilithium3Keypair::new() {
        Ok(keypair) => keypair,
        Err(_) => {
            return Ok(warp::reply::json(&ApiResponse::<MiningResponse> {
                success: false,
                data: None,
                error: Some("Failed to create miner keypair".to_string()),
            }));
        }
    };
    
    // Mine block
    let block = match blockchain.mine_block(miner_keypair.public_key.clone()) {
        Ok(block) => block,
        Err(_) => {
            return Ok(warp::reply::json(&ApiResponse::<MiningResponse> {
                success: false,
                data: None,
                error: Some("Failed to mine block".to_string()),
            }));
        }
    };
    
    // Add block to blockchain
    if let Err(_) = blockchain.add_block(block.clone()) {
        return Ok(warp::reply::json(&ApiResponse::<MiningResponse> {
            success: false,
            data: None,
            error: Some("Failed to add block".to_string()),
        }));
    }
    
    // Save to storage
    if let Err(_) = blockchain.save_to_storage(&rpc_server.storage) {
        return Ok(warp::reply::json(&ApiResponse::<MiningResponse> {
            success: false,
            data: None,
            error: Some("Failed to save block".to_string()),
        }));
    }
    
    let response = MiningResponse {
        message: format!("Block {} mined successfully", block.header.height),
        block_height: block.header.height,
        hash: block.get_hash_hex(),
    };
    
    Ok(warp::reply::json(&ApiResponse {
        success: true,
        data: Some(response),
        error: None,
    }))
} 