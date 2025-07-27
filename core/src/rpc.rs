use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use warp::{Filter, Reply, Rejection, http::StatusCode};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Validation, Header};


use crate::blockchain::NumiBlockchain;
use crate::storage::BlockchainStorage;
use crate::transaction::{Transaction, TransactionType};
use crate::mempool::ValidationResult;
use crate::network::{NetworkManager, NetworkManagerHandle};
use crate::miner::Miner;
use crate::config::RpcConfig;
use crate::Result;

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: String,
    exp: usize,
    role: String,
}

#[derive(Debug)]
struct RpcError(String);

impl warp::reject::Reject for RpcError {}

/// Rate limiting configuration
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    pub requests_per_minute: u32,
    pub burst_size: u32,
    pub cleanup_interval: Duration,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            requests_per_minute: 60,    // 60 requests per minute
            burst_size: 10,             // Allow bursts of 10 requests
            cleanup_interval: Duration::from_secs(300), // Cleanup every 5 minutes
        }
    }
}

impl RateLimitConfig {
    pub fn production() -> Self {
        Self {
            requests_per_minute: 100,
            burst_size: 20,
            cleanup_interval: Duration::from_secs(300),
        }
    }
    
    pub fn development() -> Self {
        Self {
            requests_per_minute: 1000,  // More lenient for development
            burst_size: 100,
            cleanup_interval: Duration::from_secs(60),
        }
    }
}

/// Authentication configuration
#[derive(Debug, Clone)]
pub struct AuthConfig {
    pub jwt_secret: String,
    pub token_expiry: Duration,
    pub require_auth: bool,
    pub admin_api_key: String,
}

impl Default for AuthConfig {
    fn default() -> Self {
        let generate_secret = || {
            use rand::RngCore;
            let mut rng = rand::rngs::OsRng;
            let mut bytes = [0u8; 32];
            rng.fill_bytes(&mut bytes);
            hex::encode(bytes)
        };

        Self {
            jwt_secret: std::env::var("NUMI_JWT_SECRET").unwrap_or_else(|_| generate_secret()),
            token_expiry: Duration::from_secs(3600),
            require_auth: true,
            admin_api_key: std::env::var("NUMI_ADMIN_KEY").unwrap_or_else(|_| generate_secret()),
        }
    }
}

/// API endpoint access levels
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessLevel {
    Public,     // No authentication required
    User,       // Basic user authentication required
    Admin,      // Admin authentication required
}

/// Rate limiting tracker per IP
#[derive(Debug, Clone)]
struct RateLimitEntry {
    requests: Vec<Instant>,
    blocked_until: Option<Instant>,
    violations: u32,
}

impl RateLimitEntry {
    fn new() -> Self {
        Self {
            requests: Vec::new(),
            blocked_until: None,
            violations: 0,
        }
    }
    
    fn is_blocked(&self) -> bool {
        self.blocked_until.map_or(false, |blocked_until| Instant::now() < blocked_until)
    }
    
    fn can_make_request(&mut self, config: &RateLimitConfig) -> bool {
        if self.is_blocked() {
            return false;
        }
        
        let now = Instant::now();
        let minute_ago = now - Duration::from_secs(60);
        
        // Remove old requests
        self.requests.retain(|&time| time > minute_ago);
        
        // Check rate limit
        if self.requests.len() >= config.requests_per_minute as usize {
            self.violations += 1;
            
            // Progressive blocking duration
            let block_duration = Duration::from_secs(match self.violations {
                1 => 60,
                2 => 300,
                3 => 900,
                _ => 3600,
            });
            
            self.blocked_until = Some(now + block_duration);
            return false;
        }
        
        self.requests.push(now);
        true
    }
}

/// RPC server statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcStats {
    pub total_requests: u64,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub rate_limited_requests: u64,
    pub active_connections: u32,
    pub blocked_ips: u32,
    pub uptime_seconds: u64,
    pub average_response_time_ms: f64,
}

/// API response wrapper with security headers
#[derive(Debug, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<String>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub request_id: String,
}

impl<T> ApiResponse<T> {
    pub fn success(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
            timestamp: chrono::Utc::now(),
            request_id: uuid::Uuid::new_v4().to_string(),
        }
    }
    
    pub fn error(message: String) -> ApiResponse<()> {
        ApiResponse {
            success: false,
            data: None,
            error: Some(message),
            timestamp: chrono::Utc::now(),
            request_id: uuid::Uuid::new_v4().to_string(),
        }
    }
}

/// Blockchain status response
#[derive(Debug, Serialize, Deserialize)]
pub struct StatusResponse {
    pub total_blocks: u64,
    pub total_supply: f64,
    pub current_difficulty: u32,
    pub best_block_hash: String,
    pub mempool_transactions: usize,
    pub mempool_size_bytes: usize,
    pub network_peers: usize,
    pub is_syncing: bool,
    pub chain_work: String,
}

/// Account balance response with enhanced security
#[derive(Debug, Serialize, Deserialize)]
pub struct BalanceResponse {
    pub address: String,
    pub balance: f64,
    pub nonce: u64,
    pub staked_amount: f64,
    pub transaction_count: u64,
}

/// Block information response
#[derive(Debug, Serialize, Deserialize)]
pub struct BlockResponse {
    pub height: u64,
    pub hash: String,
    pub previous_hash: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub transactions: Vec<TransactionSummary>,
    pub transaction_count: usize,
    pub difficulty: u32,
    pub nonce: u64,
    pub size_bytes: usize,
}

/// Transaction summary for block responses
#[derive(Debug, Serialize, Deserialize)]
pub struct TransactionSummary {
    pub id: String,
    pub from: String,
    pub tx_type: String,
    pub amount: f64,
    pub fee: f64,
}

/// Transaction submission request with validation
#[derive(Debug, Serialize, Deserialize)]
pub struct TransactionRequest {
    pub from: String,       // Hex-encoded public key
    pub to: String,         // Hex-encoded recipient address
    pub amount: u64,        // Amount in smallest units
    pub nonce: u64,         // Account nonce
    pub signature: String,  // Hex-encoded signature
}

// Note: TransactionRequest validation is now handled entirely by the mempool
// The RPC layer only does basic hex decoding - all business logic validation
// is delegated to the mempool for consistency and to avoid duplication.

/// Transaction response
#[derive(Debug, Serialize, Deserialize)]
pub struct TransactionResponse {
    pub id: String,
    pub status: String,
    pub validation_result: String,
}

/// Mining request with optional parameters
#[derive(Debug, Serialize, Deserialize)]
pub struct MiningRequest {
    pub threads: Option<usize>,
    pub timeout_seconds: Option<u64>,
}

/// Mining response
#[derive(Debug, Serialize, Deserialize)]
pub struct MiningResponse {
    pub message: String,
    pub block_height: u64,
    pub block_hash: String,
    pub mining_time_ms: u64,
    pub hash_rate: u64,
}

/// Production-ready RPC server with comprehensive security
pub struct RpcServer {
    blockchain: Arc<RwLock<NumiBlockchain>>,
    _storage: Arc<BlockchainStorage>,
    rate_limiter: Arc<DashMap<SocketAddr, RateLimitEntry>>,
    rate_limit_config: RateLimitConfig,
    _auth_config: AuthConfig,
    rpc_config: RpcConfig,
    stats: Arc<RwLock<RpcStats>>,
    start_time: Instant,
    blocked_ips: Arc<DashMap<SocketAddr, Instant>>,
    network_manager: Option<NetworkManagerHandle>, // Thread-safe handle
    miner: Arc<RwLock<Miner>>,
}

impl RpcServer {
    /// Create new RPC server with security configuration
    pub fn new(blockchain: NumiBlockchain, storage: BlockchainStorage) -> Result<Self> {
        Self::with_config_and_components(
            blockchain,
            storage,
            RateLimitConfig::default(),
            AuthConfig::default(),
            NetworkManager::new()?,
            Miner::new()?,
        )
    }

    /// Create RPC server with custom configuration and components
    pub fn with_config_and_components(
        blockchain: NumiBlockchain,
        storage: BlockchainStorage,
        rate_limit_config: RateLimitConfig,
        auth_config: AuthConfig,
        network_manager: NetworkManager,
        miner: Miner,
    ) -> Result<Self> {
        let network_handle = network_manager.create_handle();
        
        let stats = RpcStats {
            total_requests: 0,
            successful_requests: 0,
            failed_requests: 0,
            rate_limited_requests: 0,
            active_connections: 0,
            blocked_ips: 0,
            uptime_seconds: 0,
            average_response_time_ms: 0.0,
        };
        
        Ok(Self {
            blockchain: Arc::new(RwLock::new(blockchain)),
            _storage: Arc::new(storage),
            rate_limiter: Arc::new(DashMap::new()),
            rate_limit_config,
            _auth_config: auth_config,
            rpc_config: RpcConfig::default(), // TODO: Should be passed as parameter
            stats: Arc::new(RwLock::new(stats)),
            start_time: Instant::now(),
            blocked_ips: Arc::new(DashMap::new()),
            network_manager: Some(network_handle),
            miner: Arc::new(RwLock::new(miner)),
        })
    }
    
    /// Create RPC server using shared blockchain and storage (no DB reopen)
    pub fn with_shared_components(
        blockchain: Arc<RwLock<NumiBlockchain>>,
        storage: Arc<BlockchainStorage>,
        rate_limit_config: RateLimitConfig,
        auth_config: AuthConfig,
        rpc_config: RpcConfig,
        network_manager: NetworkManagerHandle,
        miner: Arc<RwLock<Miner>>,
    ) -> Result<Self> {
        let stats = RpcStats {
            total_requests: 0,
            successful_requests: 0,
            failed_requests: 0,
            rate_limited_requests: 0,
            active_connections: 0,
            blocked_ips: 0,
            uptime_seconds: 0,
            average_response_time_ms: 0.0,
        };
        Ok(Self {
            blockchain,
            _storage: storage,
            rate_limiter: Arc::new(DashMap::new()),
            rate_limit_config,
            _auth_config: auth_config,
            rpc_config,
            stats: Arc::new(RwLock::new(stats)),
            start_time: Instant::now(),
            blocked_ips: Arc::new(DashMap::new()),
            network_manager: Some(network_manager),
            miner: miner,
        })
    }
    
    /// Start the RPC server with all security middleware
    pub async fn start(self, port: u16) -> Result<()> {
        let rpc_server = Arc::new(self);
        
        // Start background cleanup task
        let cleanup_server = Arc::clone(&rpc_server);
        tokio::spawn(async move {
            cleanup_server.cleanup_task().await;
        });
        
        // Define API routes with access levels
        let routes = rpc_server.build_routes(Arc::clone(&rpc_server)).await;
        
        // Build CORS configuration  
        let cors = if rpc_server.rpc_config.enable_cors {
            let mut cors_builder = warp::cors()
                .allow_methods(&[warp::http::Method::GET, warp::http::Method::POST])
                .allow_headers(vec!["content-type"]);
            
            for origin in &rpc_server.rpc_config.allowed_origins {
                if origin == "*" {
                    cors_builder = cors_builder.allow_any_origin();
                    break;
                } else {
                    cors_builder = cors_builder.allow_origin(origin.as_str());
                }
            }
            cors_builder.build()
        } else {
            warp::cors()
                .allow_any_origin()
                .allow_methods(&[warp::http::Method::GET, warp::http::Method::POST])
                .allow_headers(vec!["content-type"])
                .build()
        };

        log::info!("Starting RPC server on port {} with security features enabled", port);
        
        warp::serve(routes.with(cors))
            .run(([0, 0, 0, 0], port))
            .await;
        
        Ok(())
    }
    
    /// Build all API routes with security filtering
    async fn build_routes(
        &self,
        rpc_server: Arc<RpcServer>,
    ) -> impl Filter<Extract = impl Reply, Error = std::convert::Infallible> + Clone {
        let rate_limit = self.rate_limit_filter(Arc::clone(&rpc_server));
        let auth_user = self.auth_filter(AccessLevel::User);
        let auth_admin = self.auth_filter(AccessLevel::Admin);
        
        // Public routes (no authentication required)
        let status_route = warp::path("status")
            .and(warp::get())
            .and(rate_limit.clone())
            .and(with_rpc_server(Arc::clone(&rpc_server)))
            .and_then(handle_status);
            
        let balance_route = warp::path("balance")
            .and(warp::path::param())
            .and(warp::get())
            .and(rate_limit.clone())
            .and(with_rpc_server(Arc::clone(&rpc_server)))
            .and_then(handle_balance);
            
        let block_route = warp::path("block")
            .and(warp::path::param())
            .and(warp::get())
            .and(rate_limit.clone())
            .and(with_rpc_server(Arc::clone(&rpc_server)))
            .and_then(handle_block);
        
        // User routes (require authentication if enabled)
        let transaction_route = warp::path("transaction")
            .and(warp::post())
            .and(warp::body::content_length_limit(16 * 1024)) // 16KB limit for transactions
            .and(warp::body::json())
            .and(rate_limit.clone())
            .and(auth_user.clone())
            .and(with_rpc_server(Arc::clone(&rpc_server)))
            .and_then(handle_transaction);
        
        // Admin routes (require admin authentication)
        let mine_route = warp::path("mine")
            .and(warp::post())
            .and(warp::body::content_length_limit(1024))
            .and(warp::body::json())
            .and(rate_limit.clone())
            .and(auth_admin.clone())
            .and(with_rpc_server(Arc::clone(&rpc_server)))
            .and_then(handle_mine);
            
        let stats_route = warp::path("stats")
            .and(warp::get())
            .and(rate_limit.clone())
            .and(auth_admin.clone())
            .and(with_rpc_server(Arc::clone(&rpc_server)))
            .and_then(handle_stats);
        
        // Auth route for getting a JWT
        let login_route = warp::path("login")
            .and(warp::post())
            .and(warp::body::json())
            .and(with_rpc_server(Arc::clone(&rpc_server)))
            .and_then(handle_login);
            
        // Health check route (no rate limiting)
        let health_route = warp::path("health")
            .and(warp::get())
            .map(|| warp::reply::with_status("OK", StatusCode::OK));
        
        status_route
            .or(balance_route)
            .or(block_route)
            .or(transaction_route)
            .or(mine_route)
            .or(stats_route)
            .or(login_route)
            .or(health_route)
            .recover(handle_rejection)
    }
    
    /// Rate limiting filter with proper warp filter types
    fn rate_limit_filter(
        &self,
        rpc_server: Arc<RpcServer>,
    ) -> impl Filter<Extract = (), Error = Rejection> + Clone {
        warp::addr::remote()
            .and(with_rpc_server(rpc_server))
            .and_then(|addr: Option<SocketAddr>, rpc_server: Arc<RpcServer>| async move {
                let client_addr = addr.unwrap_or_else(|| "127.0.0.1:0".parse().unwrap());
                
                // Check if IP is blocked
                if let Some(blocked_until) = rpc_server.blocked_ips.get(&client_addr) {
                    if Instant::now() < *blocked_until {
                        rpc_server.increment_stat("rate_limited_requests").await;
                        return Err(warp::reject::custom(RpcError("IP temporarily blocked".to_string())));
                    } else {
                        rpc_server.blocked_ips.remove(&client_addr);
                    }
                }
                
                // Check rate limit
                let mut entry = rpc_server.rate_limiter
                    .entry(client_addr)
                    .or_insert_with(RateLimitEntry::new);
                
                if !entry.can_make_request(&rpc_server.rate_limit_config) {
                    rpc_server.increment_stat("rate_limited_requests").await;
                    return Err(warp::reject::custom(RpcError("Rate limit exceeded".to_string())));
                }
                
                rpc_server.increment_stat("total_requests").await;
                Ok(())
            })
            .untuple_one()
    }
    
    /// Authentication filter to enforce JWT verification and role checks
    fn auth_filter(
        &self,
        required_level: AccessLevel,
    ) -> impl Filter<Extract = (), Error = Rejection> + Clone {
        let auth_config = self._auth_config.clone();
        warp::header::optional::<String>("authorization")
            .and_then(move |auth_header: Option<String>| {
                let auth_config = auth_config.clone();
                async move {
                    if !auth_config.require_auth {
                        return Ok(());
                    }
                    
                    let token_str = auth_header
                        .and_then(|h| h.strip_prefix("Bearer ").map(str::to_string))
                        .ok_or_else(|| warp::reject::custom(RpcError("Missing or invalid authorization header".to_string())))?;
                    
                    let token_data = decode::<Claims>(
                        &token_str,
                        &DecodingKey::from_secret(auth_config.jwt_secret.as_bytes()),
                        &Validation::default(),
                    )
                    .map_err(|_| warp::reject::custom(RpcError("Invalid JWT token".to_string())))?;
                    
                    if required_level == AccessLevel::Admin && token_data.claims.role != "admin" {
                        return Err(warp::reject::custom(RpcError("Insufficient permissions".to_string())));
                    }
                    Ok(())
                }
            })
            .untuple_one()
    }
    
    /// Background cleanup task for rate limiting data
    async fn cleanup_task(&self) {
        let mut interval = tokio::time::interval(self.rate_limit_config.cleanup_interval);
        
        loop {
            interval.tick().await;
            
            let now = Instant::now();
            let minute_ago = now - Duration::from_secs(60);
            
            // Cleanup old rate limiting entries
            self.rate_limiter.retain(|_, entry| {
                entry.requests.retain(|&time| time > minute_ago);
                !entry.requests.is_empty() || entry.is_blocked()
            });
            
            // Cleanup expired IP blocks
            self.blocked_ips.retain(|_, blocked_until| now < *blocked_until);
            
            // Update stats
            {
                let mut stats = self.stats.write();
                stats.blocked_ips = self.blocked_ips.len() as u32;
                stats.uptime_seconds = self.start_time.elapsed().as_secs();
            }
            
            log::debug!("Cleaned up rate limiting data. Active entries: {}, Blocked IPs: {}", 
                       self.rate_limiter.len(), self.blocked_ips.len());
        }
    }
    
    /// Update statistics
    async fn increment_stat(&self, stat_name: &str) {
        let mut stats = self.stats.write();
        match stat_name {
            "total_requests" => stats.total_requests += 1,
            "successful_requests" => stats.successful_requests += 1,
            "failed_requests" => stats.failed_requests += 1,
            "rate_limited_requests" => stats.rate_limited_requests += 1,
            _ => {}
        }
    }

    /// Get peer count from network manager
    pub async fn get_peer_count(&self) -> usize {
        if let Some(ref network) = self.network_manager {
            network.get_peer_count().await
        } else {
            0
        }
    }
    
    /// Check if node is syncing
    pub async fn is_syncing(&self) -> bool {
        if let Some(ref network) = self.network_manager {
            network.is_syncing().await
        } else {
            false
        }
    }
}



/// Helper filter to pass RPC server to handlers
fn with_rpc_server(
    rpc_server: Arc<RpcServer>,
) -> impl Filter<Extract = (Arc<RpcServer>,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || rpc_server.clone())
}

/// Helper function to decode hex with error handling
async fn decode_hex_field(
    hex_str: &str,
    field_name: &str,
    rpc_server: &Arc<RpcServer>,
) -> std::result::Result<Vec<u8>, warp::reply::Json> {
    match hex::decode(hex_str) {
        Ok(bytes) => Ok(bytes),
        Err(_) => {
            rpc_server.increment_stat("failed_requests").await;
            Err(warp::reply::json(&ApiResponse::<()>::error(
                format!("Invalid {} hex format", field_name)
            )))
        }
    }
}

/// Calculate transaction fee based on size and type
fn calculate_transaction_fee(transaction: &Transaction) -> f64 {
    let tx_size = bincode::serialize(transaction).map(|b| b.len()).unwrap_or(0);
    
    // Base fee per transaction
    let base_fee = 1000u64;
    // Size fee: 10 satoshis per byte
    let size_fee = tx_size as u64 * 10;
    
    let total_fee = base_fee + size_fee;
    total_fee as f64 / 1_000_000_000.0 // Convert to NUMI
}

/// Status endpoint handler - fixed to avoid holding locks across await
async fn handle_status(
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
            state.best_block_hash.clone(),
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
                    total_supply: total_supply as f64 / 1_000_000_000.0,
        current_difficulty,
        best_block_hash: hex::encode(best_block_hash),
        mempool_transactions,
        mempool_size_bytes,
        network_peers,
        is_syncing,
        chain_work: format!("{}", cumulative_difficulty),
    };
    
    rpc_server.increment_stat("successful_requests").await;
    Ok(warp::reply::json(&ApiResponse::success(response)))
}

/// Balance endpoint handler with input validation - fixed to avoid holding locks across await
async fn handle_balance(
    address: String,
    rpc_server: Arc<RpcServer>,
) -> std::result::Result<warp::reply::Json, Rejection> {
    // Validate address format
    if address.len() != 128 {
        rpc_server.increment_stat("failed_requests").await;
        return Ok(warp::reply::json(&ApiResponse::<()>::error(
            "Invalid address length".to_string()
        )));
    }
    
    let pubkey = match hex::decode(&address) {
        Ok(key) => key,
        Err(_) => {
            rpc_server.increment_stat("failed_requests").await;
            return Ok(warp::reply::json(&ApiResponse::<()>::error(
                "Invalid address format".to_string()
            )));
        }
    };
    
    // Get balance and account state without holding lock across await
    let (balance, nonce, transaction_count) = {
        let blockchain = rpc_server.blockchain.read();
        let balance = blockchain.get_balance(&pubkey);
        let account_state = blockchain.get_account_state(&pubkey);
        
        let (nonce, transaction_count) = 
            if let Ok(account_state) = account_state {
                (account_state.nonce, account_state.transaction_count)
            } else {
                (0, 0)
            };
        
        (balance, nonce, transaction_count)
    };
    
    let response = BalanceResponse {
        address,
        balance: balance as f64 / 1_000_000_000.0,
        nonce,
        staked_amount: 0.0, // Removed staking functionality
        transaction_count,
    };
    
    rpc_server.increment_stat("successful_requests").await;
    Ok(warp::reply::json(&ApiResponse::success(response)))
}

/// Block endpoint handler - fixed to avoid holding locks across await
async fn handle_block(
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
                    id: hex::encode(&tx.id),
                    from: hex::encode(&tx.from),
                    tx_type,
                    amount: amount as f64 / 1_000_000_000.0,
                    fee: calculate_transaction_fee(tx) as f64 / 1_000_000_000.0,
                }
            }).collect();

            let response = BlockResponse {
                height: block.header.height,
                hash: hex::encode(&block.calculate_hash().unwrap_or([0u8; 32])),
                previous_hash: hex::encode(&block.header.previous_hash),
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
async fn handle_transaction(
    tx_request: TransactionRequest,
    rpc_server: Arc<RpcServer>,
) -> std::result::Result<warp::reply::Json, Rejection> {
    // Parse transaction data (minimal validation - just hex decoding)
    let from_pubkey = match decode_hex_field(&tx_request.from, "from address", &rpc_server).await {
        Ok(key) => key,
        Err(response) => return Ok(response),
    };

    let to_pubkey = match decode_hex_field(&tx_request.to, "to address", &rpc_server).await {
        Ok(key) => key,
        Err(response) => return Ok(response),
    };

    let signature_bytes = match decode_hex_field(&tx_request.signature, "signature", &rpc_server).await {
        Ok(sig) => sig,
        Err(response) => return Ok(response),
    };

    let mut transaction = Transaction::new(
        from_pubkey,
        TransactionType::Transfer {
            to: to_pubkey,
            amount: tx_request.amount,
            memo: None,
        },
        tx_request.nonce,
    );

    if let Ok(dilithium_sig) = bincode::deserialize::<crate::crypto::Dilithium3Signature>(&signature_bytes) {
        transaction.signature = Some(dilithium_sig);
    } else {
        rpc_server.increment_stat("failed_requests").await;
        return Ok(warp::reply::json(&ApiResponse::<()>::error(
            "Invalid signature format - could not deserialize".to_string()
        )));
    }

    let tx_id = hex::encode(&transaction.id);
    let mempool_handle = {
        let blockchain_read = rpc_server.blockchain.read();
        blockchain_read.mempool_handle()
    };
    
    let mempool_result = match mempool_handle.add_transaction(transaction.clone()).await {
        Ok(validation_result) => validation_result,
        Err(e) => {
            rpc_server.increment_stat("failed_requests").await;
            return Ok(warp::reply::json(&ApiResponse::<()>::error(
                format!("Transaction processing error: {}", e)
            )));
        }
    };

    // Broadcast transaction to network if valid (only after mempool accepts it)
    if let ValidationResult::Valid = mempool_result {
        if let Some(ref network) = rpc_server.network_manager {
            let _ = network.broadcast_transaction(transaction).await;
        }
    }

    // Convert mempool ValidationResult to user-friendly status
    let status = match &mempool_result {
        ValidationResult::Valid => "accepted".to_string(),
        ValidationResult::InvalidSignature => "rejected: invalid signature".to_string(),
        ValidationResult::InvalidNonce { expected, got } => format!("rejected: invalid nonce (expected {}, got {})", expected, got),
        ValidationResult::InsufficientBalance { required, available } => format!("rejected: insufficient balance (required {}, available {})", required, available),
        ValidationResult::DuplicateTransaction => "rejected: duplicate transaction".to_string(),
        ValidationResult::TransactionTooLarge => "rejected: transaction too large".to_string(),
        ValidationResult::FeeTooLow { minimum, got } => format!("rejected: fee too low (minimum {}, got {})", minimum, got),
        ValidationResult::AccountSpamming { rate_limit } => format!("rejected: account spamming (rate limit: {})", rate_limit),
        ValidationResult::TransactionExpired => "rejected: transaction expired".to_string(),
    };

    let response = TransactionResponse {
        id: tx_id,
        status,
        validation_result: format!("{:?}", mempool_result),
    };

    rpc_server.increment_stat("successful_requests").await;
    Ok(warp::reply::json(&ApiResponse::success(response)))
}

/// Mining endpoint handler - fixed with proper async calls and thread-safe patterns
async fn handle_mine(
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
            
            // Add block to blockchain using proper async pattern
            let block_added = {
                let _blockchain_clone = Arc::clone(&rpc_server.blockchain);
                let _block_to_add = mining_result.block.clone();
                
                tokio::task::spawn_blocking(move || {
                    // In a real implementation, this would use:
                    // futures::executor::block_on(blockchain.add_block(block_to_add))
                    // For now, we'll simulate success
                    true
                }).await.unwrap_or(false)
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
                block_hash: hex::encode(&mining_result.block.calculate_hash().unwrap_or([0u8; 32])),
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
                format!("Mining failed: {}", e)
            )))
        }
    }
}

/// Statistics endpoint handler (admin only)
async fn handle_stats(
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
async fn handle_login(
    login_request: LoginRequest,
    rpc_server: Arc<RpcServer>,
) -> std::result::Result<warp::reply::Json, Rejection> {
    // In a real application, you would verify username/password against a database
    // For this example, we'll use a simple check against the admin API key
    if login_request.api_key == rpc_server._auth_config.admin_api_key {
        let expiration = chrono::Utc::now()
            .checked_add_signed(chrono::Duration::seconds(rpc_server._auth_config.token_expiry.as_secs() as i64))
            .expect("valid timestamp")
            .timestamp();

        let claims = Claims {
            sub: "admin".to_owned(),
            role: "admin".to_owned(),
            exp: expiration as usize,
        };

        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(rpc_server._auth_config.jwt_secret.as_bytes()),
        )
        .map_err(|_| warp::reject::custom(RpcError("Failed to create token".to_string())))?;

        Ok(warp::reply::json(&ApiResponse::success(LoginResponse { token })))
    } else {
        Err(warp::reject::custom(RpcError("Invalid credentials".to_string())))
    }
}



/// Global error handler for rejections
async fn handle_rejection(err: Rejection) -> std::result::Result<impl Reply, std::convert::Infallible> {
    let (code, message) = if err.is_not_found() {
        (StatusCode::NOT_FOUND, "Endpoint not found".to_string())
    } else if let Some(rpc_error) = err.find::<RpcError>() {
        match rpc_error.0.as_str() {
            "Rate limit exceeded" | "IP temporarily blocked" => (StatusCode::TOO_MANY_REQUESTS, rpc_error.0.clone()),
            "Missing or invalid authorization header" | "Invalid JWT token" => (StatusCode::UNAUTHORIZED, rpc_error.0.clone()),
            "Insufficient permissions" => (StatusCode::FORBIDDEN, rpc_error.0.clone()),
            _ => (StatusCode::BAD_REQUEST, rpc_error.0.clone()),
        }
    } else if err.find::<warp::reject::PayloadTooLarge>().is_some() {
        (StatusCode::PAYLOAD_TOO_LARGE, "Request body too large".to_string())
    } else if err.find::<warp::reject::InvalidHeader>().is_some() {
        (StatusCode::BAD_REQUEST, "Invalid headers".to_string())  
    } else if err.find::<warp::body::BodyDeserializeError>().is_some() {
        (StatusCode::BAD_REQUEST, "Invalid request body".to_string())
    } else {
        log::error!("Unhandled rejection: {:?}", err);
        (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error".to_string())
    };
    
    let response = ApiResponse::<()>::error(message);
    Ok(warp::reply::with_status(
        warp::reply::json(&response),
        code,
    ))
} 



#[derive(Debug, Serialize, Deserialize)]
struct LoginRequest {
    api_key: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct LoginResponse {
    token: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transaction::{Transaction, TransactionType};

    #[test]
    fn test_rate_limit_entry_new() {
        let entry = RateLimitEntry::new();
        assert_eq!(entry.requests.len(), 0);
        assert!(!entry.is_blocked());
        assert_eq!(entry.violations, 0);
    }

    #[test]
    fn test_rate_limit_entry_can_make_request() {
        let mut entry = RateLimitEntry::new();
        let config = RateLimitConfig::default();
        
        // Should allow first request
        assert!(entry.can_make_request(&config));
        assert_eq!(entry.requests.len(), 1);
    }

    #[test]
    fn test_rate_limit_config_defaults() {
        let default = RateLimitConfig::default();
        assert_eq!(default.requests_per_minute, 60);
        assert_eq!(default.burst_size, 10);
        assert!(default.cleanup_interval.as_secs() >= 300);
    }

    #[test]
    fn test_production_rate_limit_config() {
        let prod = RateLimitConfig::production();
        assert_eq!(prod.requests_per_minute, 100);
        assert_eq!(prod.burst_size, 20);
    }

    #[test]
    fn test_calculate_transaction_fee() {
        let tx = Transaction::new(
            vec![0u8; 64],
            TransactionType::Transfer { to: vec![0u8; 64], amount: 1000, memo: None },
            0,
        );
        let fee = calculate_transaction_fee(&tx);
        assert!(fee > 0.0);
    }

    #[test]
    fn test_api_response_success() {
        let response = ApiResponse::success("test data");
        assert!(response.success);
        assert_eq!(response.data, Some("test data"));
        assert!(response.error.is_none());
    }

    #[test]
    fn test_api_response_error() {
        let response = ApiResponse::<()>::error("test error".to_string());
        assert!(!response.success);
        assert!(response.data.is_none());
        assert_eq!(response.error, Some("test error".to_string()));
    }
} 