use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use super::types::RateLimitConfig;

/// Rate limiting tracker per IP
#[derive(Debug, Clone)]
#[derive(Default)]
pub struct RateLimitEntry {
    pub requests: Vec<Instant>,
    pub blocked_until: Option<Instant>,
    pub violations: u32,
}

impl RateLimitEntry {
    pub fn new() -> Self {
        Self::default()
    }
    
    pub fn is_blocked(&self) -> bool {
        self.blocked_until.is_some_and(|blocked_until| Instant::now() < blocked_until)
    }
    
    pub fn can_make_request(&mut self, config: &RateLimitConfig) -> bool {
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
                1 => config.block_duration_tier1,
                2 => config.block_duration_tier2,
                3 => config.block_duration_tier3,
                _ => config.block_duration_tier4,
            });
            
            self.blocked_until = Some(now + block_duration);
            return false;
        }
        
        self.requests.push(now);
        true
    }
}

/// Rate limiting manager
pub struct RateLimiter {
    rate_limiter: Arc<DashMap<SocketAddr, RateLimitEntry>>,
    blocked_ips: Arc<DashMap<SocketAddr, Instant>>,
    config: RateLimitConfig,
}

impl RateLimiter {
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            rate_limiter: Arc::new(DashMap::new()),
            blocked_ips: Arc::new(DashMap::new()),
            config,
        }
    }
    
    pub fn can_make_request(&self, client_addr: SocketAddr) -> bool {
        // Check if IP is blocked
        if let Some(blocked_until) = self.blocked_ips.get(&client_addr) {
            if Instant::now() < *blocked_until {
                return false;
            } else {
                self.blocked_ips.remove(&client_addr);
            }
        }
        
        // Check rate limit
        let mut entry = self.rate_limiter
            .entry(client_addr)
            .or_default();
        
        entry.can_make_request(&self.config)
    }
    
    pub fn cleanup(&self) {
        let now = Instant::now();
        let minute_ago = now - Duration::from_secs(60);
        
        // Cleanup old rate limiting entries
        self.rate_limiter.retain(|_, entry| {
            entry.requests.retain(|&time| time > minute_ago);
            !entry.requests.is_empty() || entry.is_blocked()
        });
        
        // Cleanup expired IP blocks
        self.blocked_ips.retain(|_, blocked_until| now < *blocked_until);
        
        log::debug!("Cleaned up rate limiting data. Active entries: {}, Blocked IPs: {}", 
                   self.rate_limiter.len(), self.blocked_ips.len());
    }
    
    pub fn get_blocked_ips_count(&self) -> u32 {
        self.blocked_ips.len() as u32
    }
} 