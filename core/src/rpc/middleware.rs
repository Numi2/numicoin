use std::net::SocketAddr;
use std::sync::Arc;

use warp::{Filter, Rejection};

use crate::rpc::RpcServer;
use super::rate_limit::RateLimiter;
use super::error::RpcError;

/// Helper filter to pass RPC server to handlers
pub fn with_rpc_server(
    rpc_server: Arc<RpcServer>,
) -> impl Filter<Extract = (Arc<RpcServer>,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || rpc_server.clone())
}

/// Rate limiting filter with proper warp filter types
pub fn rate_limit_filter(
    rate_limiter: Arc<RateLimiter>,
) -> impl Filter<Extract = (), Error = Rejection> + Clone {
    warp::addr::remote()
        .and(with_rate_limiter(rate_limiter))
        .and_then(|addr: Option<SocketAddr>, rate_limiter: Arc<RateLimiter>| async move {
            let client_addr = addr.unwrap_or_else(|| "127.0.0.1:0".parse().unwrap());
            
            if !rate_limiter.can_make_request(client_addr) {
                return Err(warp::reject::custom(RpcError("Rate limit exceeded".to_string())));
            }
            
            Ok(())
        })
        .untuple_one()
}

/// Helper filter to pass rate limiter
pub fn with_rate_limiter(
    rate_limiter: Arc<RateLimiter>,
) -> impl Filter<Extract = (Arc<RateLimiter>,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || rate_limiter.clone())
} 