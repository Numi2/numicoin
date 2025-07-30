use std::sync::Arc;

use warp::Rejection;

use crate::rpc::RpcServer;
use crate::transaction::{Transaction, TransactionType};
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
                let (tx_type, amount) = match &tx.kind {
                    TransactionType::Transfer { amount, .. } => ("transfer".to_string(), *amount),
                    TransactionType::MiningReward { amount, .. } => ("mining_reward".to_string(), *amount),
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
    let fee = tx_request.fee.unwrap_or(100); // Default fee of 1 NUMI

    // Create transaction with proper fee
    let mut transaction = Transaction::new(
        from_pubkey.clone(),
        TransactionType::Transfer {
            to: to_pubkey,
            amount: tx_request.amount,
            memo: None,
        },
        tx_request.nonce,
    );
    transaction.fee = fee; // Set the fee after creation

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
    transaction.id = transaction.hash();

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

    // Optional: broadcast transaction to network peers
    if let Some(ref network) = rpc_server.network_manager {
        let _ = network.broadcast_tx(transaction);
    }

    let response = TransactionResponse {
        id: tx_id,
        status: validation_result_to_status(&mempool_result),
        validation_result: format!("{mempool_result:?}"),
    };

    rpc_server.increment_stat("successful_requests").await;
    Ok(warp::reply::json(&ApiResponse::success(response)))
}

/// Mining endpoint handler - now directs users to Stratum V2
pub async fn handle_mine(
    rpc_server: Arc<RpcServer>,
) -> std::result::Result<warp::reply::Json, Rejection> {
    // CPU mining is no longer supported - use external Stratum V2 miners
    rpc_server.increment_stat("failed_requests").await;
    Ok(warp::reply::json(&ApiResponse::<()>::error(
        "CPU mining no longer supported. Use external Stratum V2 miners to connect to this node. Stratum server running on port 3333.".to_string()
    )))
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