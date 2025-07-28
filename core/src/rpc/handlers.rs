use std::sync::Arc;
use std::time::Instant;

use warp::Rejection;

use crate::rpc::RpcServer;
use crate::transaction::{Transaction, TransactionType, TransactionFee, MIN_TRANSACTION_FEE};
use super::types::*;
use super::auth::AuthManager;
use super::error::RpcError;

/// Status endpoint handler - fixed to avoid holding locks across await
pub async fn handle_status(
    rpc_server: Arc<RpcServer>,
) -> std::result::Result<warp::reply::Json, Rejection> {
    // Get blockchain state without holding lock across await
    let (total_blocks, total_supply, current_difficulty, best_block_hash, cumulative_difficulty, mempool_transactions, mempool_size_bytes) = {
        let blockchain = rpc_server.blockchain.read();
        let state = blockchain.get_chain_state();
        let mempool_stats = blockchain.get_mempool_stats();
        (
            state.total_blocks,
            state.total_supply,
            state.current_difficulty,
            state.best_block_hash,
            state.cumulative_difficulty,
            blockchain.get_pending_transaction_count(),
            mempool_stats.total_size_bytes,
        )
    };
    
    // Now make async calls without holding the lock
    let network_peers = rpc_server.get_peer_count().await;
    let is_syncing = rpc_server.is_syncing().await;
    
    let response = StatusResponse {
        total_blocks,
        total_supply: total_supply as f64 / 100.0,
        current_difficulty,
        best_block_hash: hex::encode(best_block_hash),
        mempool_transactions,
        mempool_size_bytes,
        network_peers,
        is_syncing,
        chain_work: format!("{cumulative_difficulty}"),
    };
    
    rpc_server.increment_stat("successful_requests").await;
    Ok(warp::reply::json(&ApiResponse::success(response)))
}

/// Balance endpoint handler with input validation - fixed to avoid holding locks across await
pub async fn handle_balance(
    address: String,
    rpc_server: Arc<RpcServer>,
) -> std::result::Result<warp::reply::Json, Rejection> {
    // Get balance and account state without holding lock across await
    let (balance, nonce, transaction_count) = {
        let blockchain = rpc_server.blockchain.read();
        let balance = blockchain.get_balance(&address);
        
        // For get_account_state, we need to convert address to public key
        // But since this function expects public key, let's skip it for now
        let (nonce, transaction_count) = (0, 0);
        
        (balance, nonce, transaction_count)
    };
    
    let response = BalanceResponse {
        address,
        balance: balance as f64 / 100.0,
        nonce,
        staked_amount: 0.0, // Removed staking functionality
        transaction_count,
    };
    
    rpc_server.increment_stat("successful_requests").await;
    Ok(warp::reply::json(&ApiResponse::success(response)))
}

/// Block endpoint handler - fixed to avoid holding locks across await
pub async fn handle_block(
    hash_or_height: String,
    rpc_server: Arc<RpcServer>,
) -> std::result::Result<warp::reply::Json, Rejection> {
    // Get block data without holding lock across await
    let block = {
        let blockchain = rpc_server.blockchain.read();
        
        // Try to parse as height first, then as hash
        if let Ok(height) = hash_or_height.parse::<u64>() {
            blockchain.get_block_by_height(height)
        } else if hash_or_height.len() == 64 {
            // Assume it's a hash
            match hex::decode(&hash_or_height) {
                Ok(hash_bytes) => {
                    if hash_bytes.len() == 32 {
                        let mut hash_array = [0u8; 32];
                        hash_array.copy_from_slice(&hash_bytes);
                        blockchain.get_block_by_hash(&hash_array)
                    } else {
                        None
                    }
                }
                Err(_) => None,
            }
        } else {
            None
        }
    };
    
    match block {
        Some(block) => {
            // Calculate transaction summaries without holding lock
            let transaction_summaries: Vec<TransactionSummary> = block.transactions.iter().map(|tx| {
                let (tx_type, amount) = match &tx.transaction_type {
                    TransactionType::Transfer { amount, .. } => ("transfer".to_string(), *amount),
                    TransactionType::MiningReward { amount, .. } => ("mining_reward".to_string(), *amount),
                    TransactionType::ContractDeploy { .. } | TransactionType::ContractCall { .. } => ("contract".to_string(), 0),
                };
                
                TransactionSummary {
                    id: hex::encode(tx.id),
                    from: hex::encode(&tx.from),
                    tx_type,
                    amount: amount as f64 / 100.0,
                    fee: get_transaction_fee_display(tx),
                }
            }).collect();

            let response = BlockResponse {
                height: block.header.height,
                hash: hex::encode(block.calculate_hash().unwrap_or([0u8; 32])),
                previous_hash: hex::encode(block.header.previous_hash),
                timestamp: block.header.timestamp,
                transactions: transaction_summaries,
                transaction_count: block.transactions.len(),
                difficulty: block.header.difficulty,
                nonce: block.header.nonce,
                size_bytes: std::mem::size_of_val(&block),
            };

            rpc_server.increment_stat("successful_requests").await;
            Ok(warp::reply::json(&ApiResponse::success(response)))
        }
        None => {
            rpc_server.increment_stat("failed_requests").await;
            Ok(warp::reply::json(&ApiResponse::<()>::error(
                "Block not found".to_string()
            )))
        }
    }
}

/// Transaction endpoint handler - delegates all validation to mempool
pub async fn handle_transaction(
    tx_request: TransactionRequest,
    rpc_server: Arc<RpcServer>,
) -> std::result::Result<warp::reply::Json, Rejection> {
    // Parse transaction data (minimal validation - just hex decoding)
    let from_pubkey = match decode_hex_field(&tx_request.from, "from address").await {
        Ok(key) => key,
        Err(msg) => {
            rpc_server.increment_stat("failed_requests").await;
            return Ok(warp::reply::json(&ApiResponse::<()>::error(msg)));
        }
    };

    let to_pubkey = match decode_hex_field(&tx_request.to, "to address").await {
        Ok(key) => key,
        Err(msg) => {
            rpc_server.increment_stat("failed_requests").await;
            return Ok(warp::reply::json(&ApiResponse::<()>::error(msg)));
        }
    };

    let signature_bytes = match decode_hex_field(&tx_request.signature, "signature").await {
        Ok(sig) => sig,
        Err(msg) => {
            rpc_server.increment_stat("failed_requests").await;
            return Ok(warp::reply::json(&ApiResponse::<()>::error(msg)));
        }
    };

    // Determine fee: use provided fee or calculate minimum
    let fee = if let Some(custom_fee) = tx_request.fee {
        custom_fee
    } else {
        // Calculate minimum fee for transaction size (estimate ~500 bytes for typical transfer)
        let estimated_size = 500;
        match TransactionFee::minimum_for_size(estimated_size) {
            Ok(fee_info) => fee_info.total,
            Err(_) => MIN_TRANSACTION_FEE,
        }
    };

    // Create transaction with proper fee
    let mut transaction = Transaction::new_with_fee(
        from_pubkey.clone(),
        TransactionType::Transfer {
            to: to_pubkey,
            amount: tx_request.amount,
            memo: None,
        },
        tx_request.nonce,
        fee,
        0, // No gas limit for simple transfers
    );

    // Set signature and recalculate transaction ID
    // In the new (v2) API the client sends *only* the detached Dilithium3 signature
    // bytes hex-encoded.  We reconstruct the full `Dilithium3Signature` struct here
    // using the already-provided sender public key and a freshly calculated
    // message hash.

    use crate::crypto::{blake3_hash, Dilithium3Signature, DILITHIUM3_SIGNATURE_SIZE, DILITHIUM3_PUBKEY_SIZE};

    // Validate basic sizes to give helpful error messages early.
    if signature_bytes.len() != DILITHIUM3_SIGNATURE_SIZE {
        rpc_server.increment_stat("failed_requests").await;
        return Ok(warp::reply::json(&ApiResponse::<()>::error(
            format!(
                "Invalid signature length: expected {} bytes, got {}",
                DILITHIUM3_SIGNATURE_SIZE,
                signature_bytes.len()
            ),
        )));
    }
    if from_pubkey.len() != DILITHIUM3_PUBKEY_SIZE {
        rpc_server.increment_stat("failed_requests").await;
        return Ok(warp::reply::json(&ApiResponse::<()>::error(
            "Invalid sender public key size".to_string(),
        )));
    }

    // Recreate the payload that was originally signed by the client: the
    // transaction without its signature or ID.  We serialise the struct via
    // `bincode`; this avoids calling the now-private `serialize_for_signing`
    // helper inside `Transaction`.
    let signing_payload: Vec<u8> = {
        let mut tx_clone = transaction.clone();
        tx_clone.signature = None;
        tx_clone.id = [0u8; 32];
        bincode::serialize(&tx_clone).unwrap_or_default()
    };

    let sig_struct = Dilithium3Signature {
        signature: signature_bytes.clone(),
        public_key: from_pubkey.clone(),
        message_hash: blake3_hash(&signing_payload),
        created_at: chrono::Utc::now().timestamp() as u64,
    };

    transaction.signature = Some(sig_struct);
    // Recalculate transaction ID now that the signature is populated so that the
    // txid commits to the signature as well.
    transaction.id = transaction.calculate_hash();

    let tx_id = hex::encode(transaction.id);
    let mempool_handle = {
        let blockchain_read = rpc_server.blockchain.read();
        blockchain_read.mempool_handle()
    };
    
    let mempool_result = match mempool_handle.add_transaction(transaction.clone()).await {
        Ok(validation_result) => validation_result,
        Err(e) => {
            rpc_server.increment_stat("failed_requests").await;
            return Ok(warp::reply::json(&ApiResponse::<()>::error(
                format!("Transaction processing error: {e}")
            )));
        }
    };

    // Broadcast transaction to network if valid (only after mempool accepts it)
    if let crate::mempool::ValidationResult::Valid = mempool_result {
        if let Some(ref network) = rpc_server.network_manager {
            let _ = network.broadcast_transaction(transaction).await;
        }
    }

    let response = TransactionResponse {
        id: tx_id,
        status: validation_result_to_status(&mempool_result),
        validation_result: format!("{mempool_result:?}"),
    };

    rpc_server.increment_stat("successful_requests").await;
    Ok(warp::reply::json(&ApiResponse::success(response)))
}

/// Mining endpoint handler - fixed with proper async calls and thread-safe patterns
pub async fn handle_mine(
    mining_request: MiningRequest,
    rpc_server: Arc<RpcServer>,
) -> std::result::Result<warp::reply::Json, Rejection> {
    // Check if admin endpoints are enabled
    if !rpc_server.rpc_config.admin_endpoints_enabled {
        rpc_server.increment_stat("failed_requests").await;
        return Ok(warp::reply::json(&ApiResponse::<()>::error(
            "Admin endpoints are disabled".to_string()
        )));
    }
    let start_time = Instant::now();
    
    // Get current blockchain state for mining using proper async pattern
    let (current_height, previous_hash, difficulty, pending_transactions) = {
        let blockchain_clone = Arc::clone(&rpc_server.blockchain);
        tokio::task::spawn_blocking(move || {
            let blockchain = blockchain_clone.read();
            let current_height = blockchain.get_current_height();
            let previous_hash = blockchain.get_latest_block_hash();
            let difficulty = blockchain.get_current_difficulty();
            let pending_transactions = blockchain.get_transactions_for_block(1_000_000, 1000); // 1MB, 1000 txs max
            (current_height, previous_hash, difficulty, pending_transactions)
        }).await.unwrap_or((0, [0; 32], 1, Vec::new()))
    };
    
    // Configure mining based on request
    let _thread_count = mining_request.threads.unwrap_or_else(num_cpus::get);
    let timeout_ms = mining_request.timeout_seconds.unwrap_or(60) * 1000;
    
    // Mine block using proper async pattern with timeout
    let mining_result = {
        let miner_clone = Arc::clone(&rpc_server.miner);
        
        // Create a timeout for mining operation
        let mining_future = tokio::task::spawn_blocking(move || {
            let mut miner = miner_clone.write();
            miner.mine_block(
                current_height + 1,
                previous_hash,
                pending_transactions,
                difficulty,
                0, // start_nonce
            )
        });
        
        // Apply timeout to mining operation
        match tokio::time::timeout(
            std::time::Duration::from_millis(timeout_ms),
            mining_future
        ).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(crate::BlockchainError::MiningError("Mining task failed".to_string())),
            Err(_) => Err(crate::BlockchainError::MiningError("Mining timeout".to_string())),
        }
    };
    
    match mining_result {
        Ok(Some(mining_result)) => {
            let mining_time = start_time.elapsed();
            
            // Add the mined block to the blockchain
            let block_added = {
                let blockchain_arc = Arc::clone(&rpc_server.blockchain);
                let block_to_add = mining_result.block.clone();

                // Offload potentially heavy validation onto a blocking thread; avoids
                // `!Send` issues with parking_lot guards inside an async future.
                tokio::task::spawn_blocking(move || {
                    let blockchain_ref = blockchain_arc.read();
                    futures::executor::block_on(async move {
                        match blockchain_ref.add_block(block_to_add).await {
                            Ok(res) => res,
                            Err(e) => {
                                log::error!("Failed to add mined block: {e}");
                                false
                            }
                        }
                    })
                })
                .await
                .unwrap_or(false)
            };
            
            // Broadcast block to network if successfully added
            if block_added {
                if let Some(ref network) = rpc_server.network_manager {
                    let _ = network.broadcast_block(mining_result.block.clone()).await;
                }
            }
            
            let response = MiningResponse {
                message: if block_added { 
                    "Block mined and added to blockchain".to_string() 
                } else { 
                    "Block mined but failed to add to blockchain".to_string() 
                },
                block_height: mining_result.block.header.height,
                block_hash: hex::encode(mining_result.block.calculate_hash().unwrap_or([0u8; 32])),
                mining_time_ms: mining_time.as_millis() as u64,
                hash_rate: mining_result.hash_rate,
            };

            rpc_server.increment_stat("successful_requests").await;
            Ok(warp::reply::json(&ApiResponse::success(response)))
        }
        Ok(None) => {
            rpc_server.increment_stat("failed_requests").await;
            Ok(warp::reply::json(&ApiResponse::<()>::error(
                "Mining timed out or was stopped".to_string()
            )))
        }
        Err(e) => {
            rpc_server.increment_stat("failed_requests").await;
            Ok(warp::reply::json(&ApiResponse::<()>::error(
                format!("Mining failed: {e}")
            )))
        }
    }
}

/// Statistics endpoint handler (admin only)
pub async fn handle_stats(
    rpc_server: Arc<RpcServer>,
) -> std::result::Result<warp::reply::Json, Rejection> {
    // Check if admin endpoints are enabled
    if !rpc_server.rpc_config.admin_endpoints_enabled {
        rpc_server.increment_stat("failed_requests").await;
        return Ok(warp::reply::json(&ApiResponse::<()>::error(
            "Admin endpoints are disabled".to_string()
        )));
    }
    let stats = rpc_server.stats.read().clone();
    rpc_server.increment_stat("successful_requests").await;
    Ok(warp::reply::json(&ApiResponse::success(stats)))
}

/// Login handler to generate JWT
pub async fn handle_login(
    login_request: LoginRequest,
    auth_manager: Arc<AuthManager>,
) -> std::result::Result<warp::reply::Json, Rejection> {
    if auth_manager.verify_api_key(&login_request.api_key) {
        match auth_manager.create_jwt("admin") {
            Ok(token) => Ok(warp::reply::json(&ApiResponse::success(LoginResponse { token }))),
            Err(e) => Err(warp::reject::custom(RpcError(e))),
        }
    } else {
        Err(warp::reject::custom(RpcError("Invalid credentials".to_string())))
    }
} 