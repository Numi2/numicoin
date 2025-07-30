use crate::{config::Config, BlockchainError, Result, crypto::Dilithium3Keypair, transaction::{Transaction, TransactionType}};
use crate::rpc::types::{ApiResponse, BalanceResponse, StatusResponse, TransactionRequest, TransactionResponse};
use reqwest::Client;
use std::time::Duration;
use std::path::PathBuf;
use hex;
use serde_json::Value;

/// Construct the base RPC URL from config
fn rpc_base_url(config: &Config) -> String {
    // If the server is bound to all interfaces, use loopback for making requests
    let host = if config.rpc.bind_address == "0.0.0.0" {
        "127.0.0.1"
    } else {
        &config.rpc.bind_address
    };
    format!("http://{}:{}", host, config.rpc.port)
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
        return Err(BlockchainError::NetworkError(response.error.unwrap_or_else(|| "Unknown error".into())));
    }
    Ok(())
}

/// Send a transaction via RPC
pub async fn send_transaction(config: Config, wallet_path: PathBuf, to: String, amount: f64, memo: Option<String>) -> Result<()> {
    let client = Client::builder().timeout(Duration::from_secs(5)).build().map_err(|e| BlockchainError::NetworkError(e.to_string()))?;
    let base_url = rpc_base_url(&config);
    // Load sender wallet
    let data = std::fs::read_to_string(&wallet_path)?;
    let wallet: Value = serde_json::from_str(&data)?;
    let private_key_hex = wallet["private_key"].as_str().ok_or_else(|| BlockchainError::InvalidArgument("Invalid wallet format".to_string()))?;
    let secret = hex::decode(private_key_hex).map_err(|e| BlockchainError::InvalidArgument(format!("Invalid private key hex: {}", e)))?;
    let public_key_hex = wallet["public_key"].as_str().ok_or_else(|| BlockchainError::InvalidArgument("Missing public key".to_string()))?;
    let public = hex::decode(public_key_hex).map_err(|e| BlockchainError::InvalidArgument(format!("Invalid public key hex: {}", e)))?;
    let keypair = Dilithium3Keypair::from_bytes(public.clone(), secret)?;
    let sender_pubkey = public;
    let from_address = hex::encode(&sender_pubkey);

    // Fetch current nonce
    let nonce = match client.get(&format!("{}/balance/{}", base_url, from_address)).send().await {
        Ok(resp) => match resp.json::<ApiResponse<BalanceResponse>>().await {
            Ok(api_resp) if api_resp.success => api_resp.data.map(|d| d.nonce).unwrap_or(0),
            _ => 0,
        },
        Err(_) => 0,
    };

    // Parse recipient
    let recipient = hex::decode(&to).map_err(|_| BlockchainError::InvalidArgument("Invalid recipient address".to_string()))?;
    let amount_raw = (amount * 100.0).round() as u64;

    let mut tx = Transaction::new(sender_pubkey.clone(), TransactionType::Transfer { to: recipient, amount: amount_raw, memo }, nonce);
    tx.sign(&keypair)?;
    let sig_hex = tx.signature.as_ref().map(|s| hex::encode(&s.signature)).ok_or_else(|| BlockchainError::InvalidSignature("Missing signature".to_string()))?;
    let tx_req = TransactionRequest { from: from_address, to, amount: amount_raw, nonce, fee: Some(tx.fee), signature: sig_hex };
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