use crate::{config::Config, BlockchainError, Result, crypto::{Dilithium3Keypair, self}, transaction::{Transaction, TransactionType}};
use crate::rpc::types::{ApiResponse, BalanceResponse, StatusResponse, TransactionRequest, TransactionResponse};
use reqwest::Client;
use std::time::Duration;
use std::path::PathBuf;
use hex;

/// Construct the base RPC URL from config
fn rpc_base_url(config: &Config) -> String {
    // If the server is bound to all interfaces, use loopback for making requests
    let host = if config.rpc.bind_address == "0.0.0.0" {
        "127.0.0.1"
    } else {
        &config.rpc.bind_address
    };
    format!("{}://{}:{}", if config.security.require_https { "https" } else { "http" }, host, config.rpc.port)
}

/// Show chain status via RPC
pub async fn show_status(config: Config) -> Result<()> {
    let client = Client::builder().timeout(Duration::from_secs(5)).build().map_err(|e| BlockchainError::NetworkError(e.to_string()))?;
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
        // Convert atomic units (nano = 1/100 NUMI) to display NUMI with 2 decimals
        println!("Total Supply: {:.2} NUMI", data.total_supply as f64 / 100.0);
        println!("Difficulty: {}", data.current_difficulty);
        println!("Best Block Hash: {}", data.best_block_hash);
        println!("Pending Transactions: {}", data.mempool_transactions);
        println!("Mempool Size: {} bytes", data.mempool_size_bytes);
        println!("Network Peers: {}", data.network_peers);
        if data.network_peers == 0 {
            println!("Is Syncing: {} (no peers - single node or isolated)", data.is_syncing);
        } else {
            println!("Is Syncing: {}", data.is_syncing);
        }
    } else {
        return Err(BlockchainError::NetworkError(response.error.unwrap_or_else(|| "Unknown error".into())));
    }
    Ok(())
}

/// Show account balance via RPC
pub async fn show_balance(config: Config, address: String) -> Result<()> {
    let client = Client::builder().timeout(Duration::from_secs(5)).build().map_err(|e| BlockchainError::NetworkError(e.to_string()))?;
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
        println!("Balance: {:.2} NUMI", data.balance as f64 / 100.0);
        println!("Nonce: {}", data.nonce);
    } else {
        // If the account is not found, the RPC returns an error. We can still display the address.
        if response.error.as_deref() == Some("Account not found") {
            println!("Address: {}", address);
            println!("Balance: 0.00 NUMI");
            println!("Nonce: 0");
        } else {
            return Err(BlockchainError::NetworkError(response.error.unwrap_or_else(|| "Unknown error".into())));
        }
    }
    Ok(())
}

/// Send a transaction via RPC
pub async fn send_transaction(config: Config, wallet_path: PathBuf, to: String, amount: f64, memo: Option<String>) -> Result<()> {
    let client = Client::builder().timeout(Duration::from_secs(5)).build().map_err(|e| BlockchainError::NetworkError(e.to_string()))?;
    let base_url = rpc_base_url(&config);
    // Use the secure keypair loader to ensure file permissions are checked.
    let keypair = Dilithium3Keypair::load_from_file(&wallet_path)?;
    let sender_pubkey = keypair.public_key_bytes().to_vec();
    let from_pubkey_hex = hex::encode(&sender_pubkey);
    let from_address_derived = crypto::derive_address_from_public_key(&sender_pubkey)?;

    // Fetch current nonce using the derived address
    let url = format!("{}/balance/{}", base_url, from_address_derived);
    let nonce_response = client.get(&url).send().await
        .map_err(|e| BlockchainError::NetworkError(format!("Failed to fetch nonce from {}: {}", url, e)))?;
    
    let current_nonce = if nonce_response.status().is_success() {
        let api_resp = nonce_response.json::<ApiResponse<BalanceResponse>>().await
            .map_err(|e| BlockchainError::SerializationError(format!("Failed to deserialize nonce response: {}", e)))?;
        if api_resp.success {
            api_resp.data.map(|d| d.nonce).ok_or_else(|| BlockchainError::NetworkError("API response for nonce was successful but contained no data".to_string()))?
        } else {
            return Err(BlockchainError::NetworkError(format!("API error when fetching nonce: {}", api_resp.error.unwrap_or_else(|| "Unknown error".to_string()))));
        }
    } else {
        let error_body = nonce_response.text().await.unwrap_or_else(|_| "unknown error".to_string());
        return Err(BlockchainError::NetworkError(format!("Failed to fetch nonce. Server response: {}", error_body)));
    };

    // The `to` address is provided in user-friendly Base58. The RPC endpoint
    // expects a hex-encoded public key. This is an inconsistency that should be
    // fixed in a future version. For now, we will work around it by assuming the
    // recipient's public key is required, not their address.
    // TODO: Refactor RPC endpoint to accept Base58 addresses directly.
    let recipient_pubkey_hex = &to;


    // Parse recipient
    let recipient = hex::decode(recipient_pubkey_hex).map_err(|_| BlockchainError::InvalidArgument(format!("Invalid recipient public key hex: '{}'", recipient_pubkey_hex)))?;
    // Use integer arithmetic for currency to avoid floating point inaccuracies.
    // The input `amount` is in NUMI, so we convert to the base unit (NANO).
    let amount_raw = (amount * 100.0).round() as u64;

    let new_nonce = current_nonce + 1;
    let mut tx = Transaction::new(sender_pubkey.clone(), TransactionType::Transfer { to: recipient, amount: amount_raw, memo }, new_nonce);
    tx.sign(&keypair)?;
    let sig_hex = tx.signature.as_ref().map(|s| hex::encode(&s.signature)).ok_or_else(|| BlockchainError::InvalidSignature("Missing signature".to_string()))?;
    let tx_req = TransactionRequest { from: from_pubkey_hex, to: to.clone(), amount: amount_raw, nonce: new_nonce, fee: Some(tx.fee), signature: sig_hex };
    let resp = client.post(&format!("{}/transaction", base_url)).json(&tx_req).send().await.map_err(|e| BlockchainError::NetworkError(e.to_string()))?.json::<ApiResponse<TransactionResponse>>().await.map_err(|e| BlockchainError::SerializationError(e.to_string()))?;
    if let Some(data) = resp.data { println!("Transaction ID: {}", data.id); println!("Validation Result: {}", data.validation_result); println!("Status: {}", data.status); } else {
        return Err(BlockchainError::InvalidArgument(resp.error.unwrap_or_else(|| "Unknown error".into())));
    }
    Ok(())
}

/// Inform user about Stratum V2 mining (CPU mining no longer supported)
pub async fn mine_blocks(config: Config, _wallet_path: PathBuf) -> Result<()> {
    println!("❌ CPU mining is no longer supported in this version.");
    println!("🚀 Use external Stratum V2 miners instead:");
    println!();
    println!("  • Numichain Server: {}:{}", 
        config.mining.stratum_bind_address, 
        config.mining.stratum_bind_port
    );
    println!("  • Protocol: Stratum V2 with Noise XX encryption");
    println!("  • Features: BLAKE3 share validation, Dilithium3 signatures");
    println!();
    println!("📖 Connection example:");
    println!("  1. Connect to the Stratum V2 port");
    println!("  2. Perform Noise XX handshake");
    println!("  3. Open standard mining channel");
    println!("  4. Receive jobs and submit shares");
    println!();
    
    log::info!("Directed user to Stratum V2 mining (CPU mining disabled)");
    Ok(())
}