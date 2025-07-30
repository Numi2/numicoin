use std::sync::Arc;
use crate::block::Block;
use tokio::time::timeout;
use std::time::Duration;

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
        total_supply,
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
    let account_state = {
        let blockchain = rpc_server.blockchain.read();
        blockchain.get_account_state_by_address(&address)
    };

    if let Some(state) = account_state {
        let response = BalanceResponse {
            address,
            balance: state.balance,
            nonce: state.nonce,
            transaction_count: state.transaction_count,
        };
        rpc_server.increment_stat("successful_requests").await;
        Ok(warp::reply::json(&ApiResponse::success(response)))
    } else {
        rpc_server.increment_stat("failed_requests").await;
        Ok(warp::reply::json(&ApiResponse::<()>::error(
            "Account not found".to_string()
        )))
    }
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
                let (tx_type, amount) = match tx.kind {
                    TransactionType::Transfer { amount, .. } => ("transfer".to_string(), amount),
                    TransactionType::MiningReward { amount, .. } => ("mining_reward".to_string(), amount),
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
                hash: hex::encode(block.calculate_hash(None).unwrap_or([0u8; 32])),
                previous_hash: hex::encode(block.header.previous_hash),
                timestamp: block.header.timestamp,
                transactions: transaction_summaries,
                transaction_count: block.transactions.len(),
                difficulty: block.header.difficulty,
                nonce: block.header.nonce,
                size_bytes: bincode::serialized_size(&block).unwrap_or(0) as usize,
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
    let fee = tx_request.fee.unwrap_or(rpc_server.blockchain.read().consensus_params().min_transaction_fee);

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
        match bincode::serialize(&tx_clone) {
            Ok(payload) => payload,
            Err(e) => {
                rpc_server.increment_stat("failed_requests").await;
                return Ok(warp::reply::json(&ApiResponse::<()>::error(
                    format!("Failed to serialize transaction for signing: {}", e)
                )));
            }
        }
    };

    let sig_struct = Dilithium3Signature {
        signature: signature_bytes.clone(),
        public_key: from_pubkey.clone(),
        message_hash: blake3_hash(&signing_payload),
        created_at: chrono::Utc::now().timestamp() as u64,
    };

    // Before adding to the mempool, perform a standalone signature verification.
    match crate::crypto::Dilithium3Keypair::verify(&signing_payload, &sig_struct, &from_pubkey) {
        Ok(true) => (), // Signature is valid
        Ok(false) => {
            rpc_server.increment_stat("failed_requests").await;
            return Ok(warp::reply::json(&ApiResponse::<()>::error(
                "Transaction signature verification failed".to_string()
            )));
        }
        Err(e) => {
            rpc_server.increment_stat("failed_requests").await;
            return Ok(warp::reply::json(&ApiResponse::<()>::error(
                format!("Error during signature verification: {}", e)
            )));
        }
    }

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

pub async fn handle_mine_block(
    _body: MineBlockRequest,
    rpc_server: Arc<RpcServer>,
) -> std::result::Result<warp::reply::Json, Rejection> {
    let (height, previous_hash, difficulty, transactions, consensus) = {
        let blockchain = rpc_server.blockchain.read();
        (
            blockchain.get_current_height() + 1,
            blockchain.get_latest_block_hash(),
            blockchain.get_current_difficulty(),
            blockchain.get_transactions_for_block(1024 * 1024, 100),
            blockchain.consensus_params(),
        )
    };

    let miner_keypair = rpc_server.miner.read().get_keypair().clone();

    let mut block_to_mine = Block::new(
        height,
        previous_hash,
        transactions,
        difficulty,
        miner_keypair.public_key.clone(),
    );

    let mining_result = timeout(
        Duration::from_secs(120),
        tokio::task::spawn_blocking(move || {
            block_to_mine.mine(&miner_keypair, &consensus).map(|_| block_to_mine)
        }),
    )
    .await;

    match mining_result {
        Ok(Ok(Ok(mined_block))) => {
            let hash = mined_block.calculate_hash(None).unwrap_or_default();
            let nonce = mined_block.header.nonce;
            let transactions_count = mined_block.transactions.len();
            
            let blockchain_write = rpc_server.blockchain.write();
            match blockchain_write.add_block(mined_block).await {
                Ok(_) => {
                    let response = MineBlockResponse {
                        height,
                        hash: hex::encode(hash),
                        transactions: transactions_count,
                        nonce,
                    };
                    rpc_server.increment_stat("successful_requests").await;
                    Ok(warp::reply::json(&ApiResponse::success(response)))
                }
                Err(e) => {
                    rpc_server.increment_stat("failed_requests").await;
                    Err(warp::reject::custom(RpcError(format!("Failed to add block to chain: {}", e))))
                }
            }
        }
        Ok(Ok(Err(e))) => {
            rpc_server.increment_stat("failed_requests").await;
            Err(warp::reject::custom(RpcError(format!("Mining failed: {}", e))))
        }
        Ok(Err(e)) => {
            rpc_server.increment_stat("failed_requests").await;
Err(warp::reject::custom(RpcError(format!("Mining task panicked: {}", e))))
        }
        Err(_) => {
            rpc_server.increment_stat("failed_requests").await;
            Err(warp::reject::custom(RpcError("Mining timed out after 120 seconds".to_string())))
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
