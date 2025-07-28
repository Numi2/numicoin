pub mod types;
pub mod error;
pub mod auth;
pub mod rate_limit;
pub mod middleware;
pub mod handlers;

use std::sync::Arc;
use std::time::Instant;

use parking_lot::RwLock;
use warp::{Filter, Reply, http::StatusCode};

use crate::blockchain::NumiBlockchain;
use crate::storage::BlockchainStorage;
use crate::network::{NetworkManager, NetworkManagerHandle};
use crate::miner::Miner;
use crate::config::RpcConfig;
use crate::Result;

pub use types::*;
pub use error::handle_rejection;

use auth::AuthManager;
use rate_limit::RateLimiter;
use middleware::{with_rpc_server, rate_limit_filter};
use handlers::*;

/// Production-ready RPC server with comprehensive security
pub struct RpcServer {
    pub blockchain: Arc<parking_lot::RwLock<NumiBlockchain>>,
    pub _storage: Arc<BlockchainStorage>,
    pub rate_limiter: Arc<RateLimiter>,
    pub auth_manager: Arc<AuthManager>,
    pub rpc_config: RpcConfig,
    pub stats: Arc<RwLock<RpcStats>>,
    pub start_time: Instant,
    pub network_manager: Option<NetworkManagerHandle>,
    pub miner: Arc<RwLock<Miner>>,
}

impl RpcServer {
    /// Create new RPC server with security configuration
    pub fn new(blockchain: NumiBlockchain, storage: BlockchainStorage) -> Result<Self> {
        let blockchain_arc = Arc::new(parking_lot::RwLock::new(blockchain));
        let network_manager = NetworkManager::new(blockchain_arc.clone())?;
        let miner = Miner::new()?;
        
        Self::with_config_and_components(
            Arc::try_unwrap(blockchain_arc).map_err(|_| crate::BlockchainError::StorageError("Failed to unwrap blockchain".to_string()))?.into_inner(),
            storage,
            RateLimitConfig::default(),
            AuthConfig::default(),
            RpcConfig::default(),
            network_manager,
            miner,
        )
    }

    /// Create RPC server with custom configuration and components
    pub fn with_config_and_components(
        blockchain: NumiBlockchain,
        storage: BlockchainStorage,
        rate_limit_config: RateLimitConfig,
        auth_config: AuthConfig,
        rpc_config: RpcConfig,
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
            rate_limiter: Arc::new(RateLimiter::new(rate_limit_config)),
            auth_manager: Arc::new(AuthManager::new(auth_config)),
            rpc_config,
            stats: Arc::new(RwLock::new(stats)),
            start_time: Instant::now(),
            network_manager: Some(network_handle),
            miner: Arc::new(RwLock::new(miner)),
        })
    }
    
    /// Create RPC server using shared blockchain and storage (no DB reopen)
    pub fn with_shared_components(
        blockchain: Arc<parking_lot::RwLock<NumiBlockchain>>,
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
            rate_limiter: Arc::new(RateLimiter::new(rate_limit_config)),
            auth_manager: Arc::new(AuthManager::new(auth_config)),
            rpc_config,
            stats: Arc::new(RwLock::new(stats)),
            start_time: Instant::now(),
            network_manager: Some(network_manager),
            miner,
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

        log::info!("Starting RPC server on port {port} with security features enabled");
        
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
        let rate_limit = rate_limit_filter(Arc::clone(&rpc_server.rate_limiter));
        let auth_admin = rpc_server.auth_manager.auth_filter(AccessLevel::Admin);
        let auth_manager = Arc::clone(&rpc_server.auth_manager);
        
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
        
        // Public routes (no authentication required) - making blockchain open to the people
        let transaction_route = warp::path("transaction")
            .and(warp::post())
            .and(warp::body::content_length_limit(16 * 1024)) // 16KB limit for transactions
            .and(warp::body::json())
            .and(rate_limit.clone())
            .and(with_rpc_server(Arc::clone(&rpc_server)))
            .and_then(handle_transaction);
        
        // Public routes (no authentication required) - making mining open to the people
        let mine_route = warp::path("mine")
            .and(warp::post())
            .and(warp::body::content_length_limit(1024))
            .and(warp::body::json())
            .and(rate_limit.clone())
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
            .and(with_auth_manager(auth_manager))
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
    
    /// Background cleanup task for rate limiting data
    async fn cleanup_task(&self) {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(300)); // 5 minutes
        
        loop {
            interval.tick().await;
            
            // Cleanup rate limiting data
            self.rate_limiter.cleanup();
            
            // Update stats
            {
                let mut stats = self.stats.write();
                stats.blocked_ips = self.rate_limiter.get_blocked_ips_count();
                stats.uptime_seconds = self.start_time.elapsed().as_secs();
            }
        }
    }
    
    /// Update statistics
    pub async fn increment_stat(&self, stat_name: &str) {
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

/// Helper filter to pass auth manager to handlers
fn with_auth_manager(
    auth_manager: Arc<AuthManager>,
) -> impl Filter<Extract = (Arc<AuthManager>,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || auth_manager.clone())
} 