use std::path::PathBuf;
use std::time::Duration;
use serde::{Deserialize, Serialize};
use crate::crypto::Argon2Config;

/// Main configuration for the NumiCoin blockchain node
#[derive(Debug, Clone, Serialize, Deserialize)]
#[derive(Default)]
pub struct Config {
    pub network: NetworkConfig,
    pub mining: MiningConfig,
    pub rpc: RpcConfig,
    pub security: SecurityConfig,
    pub storage: StorageConfig,
    pub consensus: ConsensusConfig,
}


impl Config {
    /// Production configuration with hardened security settings
    pub fn production() -> Self {
        Self {
            network: NetworkConfig::production(),
            mining: MiningConfig::production(),
            rpc: RpcConfig::production(),
            security: SecurityConfig::production(),
            storage: StorageConfig::production(),
            consensus: ConsensusConfig::production(),
        }
    }

    /// Development configuration with relaxed settings for testing
    pub fn development() -> Self {
        Self {
            network: NetworkConfig::development(),
            mining: MiningConfig::development(),
            rpc: RpcConfig::development(),
            security: SecurityConfig::development(),
            storage: StorageConfig::development(),
            consensus: ConsensusConfig::development(),
        }
    }

    /// Testnet configuration with testnet-specific settings
    pub fn testnet() -> Self {
        Self {
            network: NetworkConfig::testnet(),
            mining: MiningConfig::testnet(),
            rpc: RpcConfig::testnet(),
            security: SecurityConfig::testnet(),
            storage: StorageConfig::testnet(),
            consensus: ConsensusConfig::testnet(),
        }
    }

    /// Load configuration from file with environment variable overrides
    pub fn load_from_file<P: AsRef<std::path::Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let config_str = std::fs::read_to_string(path)?;
        let mut config: Config = toml::from_str(&config_str)?;
        
        // Apply environment variable overrides
        config.apply_env_overrides();
        
        Ok(config)
    }

    /// Save configuration to file
    pub fn save_to_file<P: AsRef<std::path::Path>>(&self, path: P) -> Result<(), Box<dyn std::error::Error>> {
        let config_str = toml::to_string_pretty(self)?;
        std::fs::write(path, config_str)?;
        Ok(())
    }

    /// Apply environment variable overrides
    fn apply_env_overrides(&mut self) {
        // Network overrides
        if let Ok(port) = std::env::var("NUMI_NETWORK_PORT") {
            if let Ok(port_num) = port.parse::<u16>() {
                self.network.listen_port = port_num;
            }
        }

        if let Ok(addr) = std::env::var("NUMI_NETWORK_LISTEN_ADDR") {
            self.network.listen_address = addr;
        }

        // Mining overrides
        if let Ok(threads) = std::env::var("NUMI_MINING_THREADS") {
            if let Ok(thread_count) = threads.parse::<usize>() {
                self.mining.thread_count = thread_count;
            }
        }

        // RPC overrides
        if let Ok(port) = std::env::var("NUMI_RPC_PORT") {
            if let Ok(port_num) = port.parse::<u16>() {
                self.rpc.port = port_num;
            }
        }

        if let Ok(enabled) = std::env::var("NUMI_RPC_ENABLED") {
            self.rpc.enabled = enabled.to_lowercase() == "true";
        }

        // Security overrides
        if let Ok(secret) = std::env::var("NUMI_JWT_SECRET") {
            self.security.jwt_secret = secret;
        }

        // Storage overrides
        if let Ok(path) = std::env::var("NUMI_DATA_DIR") {
            self.storage.data_directory = PathBuf::from(path);
        }
    }

    /// Validate configuration settings
    pub fn validate(&self) -> Result<(), String> {
        self.network.validate()?;
        self.mining.validate()?;
        self.rpc.validate()?;
        self.security.validate()?;
        self.storage.validate()?;
        self.consensus.validate()?;
        Ok(())
    }
}

/// Network configuration for P2P communication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub enabled: bool,
    pub listen_address: String,
    pub listen_port: u16,
    pub max_peers: usize,
    pub connection_timeout_secs: u64,
    pub bootstrap_nodes: Vec<String>,
    pub enable_upnp: bool,
    pub enable_mdns: bool,
    pub peer_discovery_interval_secs: u64,
    pub max_message_size: usize,
    pub ban_duration_secs: u64,
    pub rate_limit_per_peer: u32,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            listen_address: "0.0.0.0".to_string(),
            listen_port: 8333,
            max_peers: 50,
            connection_timeout_secs: 30,
            bootstrap_nodes: vec![
                "/ip4/127.0.0.1/tcp/8333".to_string(),
            ],
            enable_upnp: false,
            enable_mdns: true,
            peer_discovery_interval_secs: 300,
            max_message_size: 10 * 1024 * 1024, // 10MB
            ban_duration_secs: 3600, // 1 hour
            rate_limit_per_peer: 100, // messages per minute
        }
    }
}

impl NetworkConfig {
    pub fn production() -> Self {
        Self {
            max_peers: 100,
            enable_upnp: true,
            enable_mdns: false, // Disable mDNS in production
            bootstrap_nodes: vec![
                // Production bootstrap nodes would go here
                "/ip4/seed1.numicoin.org/tcp/8333".to_string(),
                "/ip4/seed2.numicoin.org/tcp/8333".to_string(),
            ],
            max_message_size: 32 * 1024 * 1024, // 32MB for production
            ..Default::default()
        }
    }

    pub fn development() -> Self {
        Self {
            max_peers: 10,
            connection_timeout_secs: 10,
            peer_discovery_interval_secs: 60,
            ban_duration_secs: 300, // 5 minutes for development
            rate_limit_per_peer: 1000, // More lenient for testing
            ..Default::default()
        }
    }

    pub fn testnet() -> Self {
        Self {
            max_peers: 20,
            connection_timeout_secs: 15,
            peer_discovery_interval_secs: 120,
            ban_duration_secs: 600, // 10 minutes for testnet
            rate_limit_per_peer: 500,
            bootstrap_nodes: vec![
                "/ip4/127.0.0.1/tcp/8334".to_string(),
                "/ip4/127.0.0.1/tcp/8335".to_string(),
            ],
            enable_mdns: true,
            ..Default::default()
        }
    }



    pub fn validate(&self) -> Result<(), String> {
        if self.listen_port == 0 {
            return Err("Listen port cannot be 0".to_string());
        }
        if self.max_peers == 0 {
            return Err("Max peers must be greater than 0".to_string());
        }
        if self.max_message_size < 1024 {
            return Err("Max message size too small".to_string());
        }
        Ok(())
    }
}

/// Mining configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MiningConfig {
    pub enabled: bool,
    pub thread_count: usize,
    pub nonce_chunk_size: u64,
    pub stats_update_interval_secs: u64,
    /// Path to the miner's wallet file (relative to data_directory)
    pub wallet_path: PathBuf,
    pub argon2_config: Argon2Config,
    pub enable_cpu_affinity: bool,
    pub thermal_throttle_temp: f32,
    pub power_limit_watts: f32,
    pub mining_pool_url: Option<String>,
    pub mining_pool_worker: Option<String>,
    pub target_block_time_secs: u64,
    pub difficulty_adjustment_interval: u64,
}

impl Default for MiningConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            thread_count: num_cpus::get(),
            nonce_chunk_size: 1_000_000,
            stats_update_interval_secs: 5,
            wallet_path: PathBuf::from("wallet.key"),
            argon2_config: Argon2Config::default(),
            enable_cpu_affinity: true,
            thermal_throttle_temp: 85.0,
            power_limit_watts: 0.0, // 0 = no limit
            mining_pool_url: None,
            mining_pool_worker: None,
            target_block_time_secs: 60,
            difficulty_adjustment_interval: 2016,
        }
    }
}

impl MiningConfig {
    /// High-performance configuration for dedicated mining hardware
    pub fn production() -> Self {
        Self {
            enabled: true,
            thread_count: num_cpus::get(),
            nonce_chunk_size: 50_000,
            stats_update_interval_secs: 2,
            wallet_path: PathBuf::from("miner-wallet.json"),
            argon2_config: Argon2Config::production(),
            enable_cpu_affinity: true,
            thermal_throttle_temp: 90.0,
            ..Default::default()
        }
    }

    /// Low-power configuration for background mining
    pub fn development() -> Self {
        Self {
            enabled: true,
            thread_count: (num_cpus::get() / 2).max(1),
            nonce_chunk_size: 1_000,
            stats_update_interval_secs: 10,
            wallet_path: PathBuf::from("miner-wallet.json"),
            argon2_config: Argon2Config::development(),
            enable_cpu_affinity: false,
            thermal_throttle_temp: 70.0,
            power_limit_watts: 50.0,
            target_block_time_secs: 10, // Faster blocks for development
            difficulty_adjustment_interval: 20,
            ..Default::default()
        }
    }

    /// Testnet configuration with testnet-specific settings
    pub fn testnet() -> Self {
        Self {
            enabled: true,
            thread_count: num_cpus::get_physical(),
            nonce_chunk_size: 5_000,
            stats_update_interval_secs: 5,
            wallet_path: PathBuf::from("miner-wallet.json"),
            argon2_config: Argon2Config::development(),
            enable_cpu_affinity: false,
            thermal_throttle_temp: 75.0,
            power_limit_watts: 0.0, // No power limit for testnet
            target_block_time_secs: 15, // Slightly slower than development
            difficulty_adjustment_interval: 30,
            ..Default::default()
        }
    }



    pub fn validate(&self) -> Result<(), String> {
        if self.thread_count == 0 {
            return Err("Thread count must be greater than 0".to_string());
        }
        if self.nonce_chunk_size == 0 {
            return Err("Nonce chunk size must be greater than 0".to_string());
        }
        if self.target_block_time_secs == 0 {
            return Err("Target block time must be greater than 0".to_string());
        }
        Ok(())
    }
}

/// RPC server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcConfig {
    pub enabled: bool,
    pub bind_address: String,
    pub port: u16,
    pub max_connections: usize,
    pub request_timeout_secs: u64,
    pub max_request_size: usize,
    pub enable_cors: bool,
    pub allowed_origins: Vec<String>,
    pub rate_limit_requests_per_minute: u32,
    pub rate_limit_burst_size: u32,
    pub enable_authentication: bool,
    pub admin_endpoints_enabled: bool,
}

impl Default for RpcConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            bind_address: "127.0.0.1".to_string(),
            port: 8080,
            max_connections: 100,
            request_timeout_secs: 30,
            max_request_size: 1024 * 1024, // 1MB
            enable_cors: true,
            allowed_origins: vec![
                "http://localhost:3000".to_string(),
                "http://127.0.0.1:3000".to_string(),
            ],
            rate_limit_requests_per_minute: 60,
            rate_limit_burst_size: 10,
            enable_authentication: false,
            admin_endpoints_enabled: false,
        }
    }
}

impl RpcConfig {
    pub fn production() -> Self {
        Self {
            bind_address: "0.0.0.0".to_string(),
            max_connections: 1000,
            rate_limit_requests_per_minute: 100,
            rate_limit_burst_size: 20,
            enable_authentication: true,
            admin_endpoints_enabled: true,
            allowed_origins: vec![
                "https://wallet.numicoin.org".to_string(),
                "https://explorer.numicoin.org".to_string(),
            ],
            ..Default::default()
        }
    }

    pub fn development() -> Self {
        Self {
            rate_limit_requests_per_minute: 1000,
            rate_limit_burst_size: 100,
            enable_authentication: false,
            admin_endpoints_enabled: true,
            allowed_origins: vec![
                "http://localhost:3000".to_string(),
                "http://localhost:3001".to_string(),
                "http://127.0.0.1:3000".to_string(),
            ],
            ..Default::default()
        }
    }

    pub fn testnet() -> Self {
        Self {
            bind_address: "0.0.0.0".to_string(),
            port: 8081, // Different port for testnet
            max_connections: 200,
            rate_limit_requests_per_minute: 500,
            rate_limit_burst_size: 50,
            enable_authentication: false,
            admin_endpoints_enabled: true,
            allowed_origins: vec![
                "http://localhost:3000".to_string(),
                "http://localhost:3001".to_string(),
                "http://127.0.0.1:3000".to_string(),
                "https://testnet.numicoin.org".to_string(),
            ],
            ..Default::default()
        }
    }



    pub fn validate(&self) -> Result<(), String> {
        if self.port == 0 {
            return Err("RPC port cannot be 0".to_string());
        }
        if self.max_connections == 0 {
            return Err("Max connections must be greater than 0".to_string());
        }
        if self.max_request_size < 1024 {
            return Err("Max request size too small".to_string());
        }
        Ok(())
    }
}

/// Security configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    pub jwt_secret: String,
    pub jwt_expiry_hours: u64,
    pub admin_api_key: String,
    pub enable_rate_limiting: bool,
    pub enable_ip_blocking: bool,
    pub max_failed_attempts: u32,
    pub block_duration_minutes: u64,
    pub enable_request_signing: bool,
    pub require_https: bool,
    pub enable_firewall: bool,
    pub trusted_proxies: Vec<String>,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            jwt_secret: std::env::var("NUMI_JWT_SECRET")
                .unwrap_or_else(|_| {
                    log::warn!("JWT_SECRET not set in environment, using cryptographically secure random value");
                    use rand::RngCore;
                    let mut rng = rand::rngs::OsRng;
                    let mut bytes = [0u8; 32];
                    rng.fill_bytes(&mut bytes);
                    hex::encode(bytes)
                }),
            jwt_expiry_hours: 1,
            admin_api_key: std::env::var("NUMI_ADMIN_KEY")
                .unwrap_or_else(|_| {
                    log::warn!("ADMIN_KEY not set in environment, using cryptographically secure random value");
                    use rand::RngCore;
                    let mut rng = rand::rngs::OsRng;
                    let mut bytes = [0u8; 32];
                    rng.fill_bytes(&mut bytes);
                    hex::encode(bytes)
                }),
            enable_rate_limiting: true,
            enable_ip_blocking: true,
            max_failed_attempts: 5,
            block_duration_minutes: 15,
            enable_request_signing: false,
            require_https: false,
            enable_firewall: false,
            trusted_proxies: Vec::new(),
        }
    }
}

impl SecurityConfig {
    pub fn production() -> Self {
        Self {
            jwt_secret: std::env::var("NUMI_JWT_SECRET")
                .unwrap_or_else(|_| {
                    log::warn!("JWT_SECRET not set in environment, using cryptographically secure random value");
                    use rand::RngCore;
                    let mut rng = rand::rngs::OsRng;
                    let mut bytes = [0u8; 32];
                    rng.fill_bytes(&mut bytes);
                    hex::encode(bytes)
                }),
            jwt_expiry_hours: 24,
            admin_api_key: std::env::var("NUMI_ADMIN_KEY")
                .unwrap_or_else(|_| {
                    log::warn!("ADMIN_KEY not set in environment, using cryptographically secure random value");
                    use rand::RngCore;
                    let mut rng = rand::rngs::OsRng;
                    let mut bytes = [0u8; 32];
                    rng.fill_bytes(&mut bytes);
                    hex::encode(bytes)
                }),
            enable_rate_limiting: true,
            enable_ip_blocking: true,
            max_failed_attempts: 3,
            block_duration_minutes: 60,
            enable_request_signing: true,
            require_https: true,
            enable_firewall: true,
            trusted_proxies: vec![
                "127.0.0.1".to_string(),
                "::1".to_string(),
            ],
        }
    }

    pub fn development() -> Self {
        Self {
            max_failed_attempts: 10,
            block_duration_minutes: 1,
            enable_request_signing: false,
            require_https: false,
            enable_firewall: false,
            ..Default::default()
        }
    }

    pub fn testnet() -> Self {
        Self {
            max_failed_attempts: 15,
            block_duration_minutes: 5,
            enable_request_signing: false,
            require_https: false,
            enable_firewall: false,
            trusted_proxies: vec![
                "127.0.0.1".to_string(),
                "::1".to_string(),
                "10.0.0.0/8".to_string(),
                "172.16.0.0/12".to_string(),
                "192.168.0.0/16".to_string(),
            ],
            ..Default::default()
        }
    }



    pub fn validate(&self) -> Result<(), String> {
        if self.jwt_secret.len() < 32 {
            return Err("JWT secret must be at least 32 characters".to_string());
        }
        if self.admin_api_key.len() < 16 {
            return Err("Admin API key must be at least 16 characters".to_string());
        }
        if self.jwt_expiry_hours == 0 {
            return Err("JWT expiry must be greater than 0".to_string());
        }
        Ok(())
    }
}

/// Storage configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    pub data_directory: PathBuf,
    pub backup_directory: Option<PathBuf>,
    pub max_database_size_mb: u64,
    pub cache_size_mb: u64,
    pub enable_compression: bool,
    pub enable_encryption: bool,
    pub auto_backup: bool,
    pub backup_interval_hours: u64,
    pub retention_days: u64,
    pub sync_mode: SyncMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SyncMode {
    Full,
    Normal,
    Fast,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            data_directory: PathBuf::from("./data"),
            backup_directory: None,
            max_database_size_mb: 10 * 1024, // 10GB
            cache_size_mb: 512, // 512MB
            enable_compression: true,
            enable_encryption: false,
            auto_backup: false,
            backup_interval_hours: 24,
            retention_days: 30,
            sync_mode: SyncMode::Normal,
        }
    }
}

impl StorageConfig {
    pub fn production() -> Self {
        Self {
            data_directory: PathBuf::from("/var/lib/numicoin"),
            backup_directory: Some(PathBuf::from("/var/backups/numicoin")),
            max_database_size_mb: 100 * 1024, // 100GB
            cache_size_mb: 2048, // 2GB
            enable_compression: true,
            enable_encryption: true,
            auto_backup: true,
            backup_interval_hours: 6,
            retention_days: 90,
            sync_mode: SyncMode::Full,
        }
    }

    pub fn development() -> Self {
        Self {
            data_directory: PathBuf::from("./dev-data"),
            backup_directory: Some(PathBuf::from("./dev-backups")),
            max_database_size_mb: 1024, // 1GB
            cache_size_mb: 128, // 128MB
            enable_compression: false,
            enable_encryption: false,
            auto_backup: false,
            sync_mode: SyncMode::Fast,
            ..Default::default()
        }
    }

    pub fn testnet() -> Self {
        Self {
            data_directory: PathBuf::from("./testnet-data"),
            backup_directory: Some(PathBuf::from("./testnet-backups")),
            max_database_size_mb: 2048, // 2GB for testnet
            cache_size_mb: 256, // 256MB
            enable_compression: true,
            enable_encryption: false,
            auto_backup: true,
            backup_interval_hours: 12,
            retention_days: 7,
            sync_mode: SyncMode::Normal,
            ..Default::default()
        }
    }



    pub fn validate(&self) -> Result<(), String> {
        if self.max_database_size_mb == 0 {
            return Err("Max database size must be greater than 0".to_string());
        }
        if self.cache_size_mb == 0 {
            return Err("Cache size must be greater than 0".to_string());
        }
        if self.retention_days == 0 {
            return Err("Retention days must be greater than 0".to_string());
        }
        Ok(())
    }
}

/// Consensus configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusConfig {
    pub target_block_time: Duration,
    pub difficulty_adjustment_interval: u64,
    pub max_block_size: usize,
    pub max_transactions_per_block: usize,
    pub min_transaction_fee: u64,
    pub max_reorg_depth: u64,
    pub checkpoint_interval: u64,
    pub finality_depth: u64,
    pub genesis_supply: u64,
    pub mining_reward_halving_interval: u64,
    pub initial_mining_reward: u64,
}

impl Default for ConsensusConfig {
    fn default() -> Self {
        Self {
            target_block_time: Duration::from_secs(30),
            difficulty_adjustment_interval: 144,
            max_block_size: 2 * 1024 * 1024, // 2MB
            max_transactions_per_block: 10000,
            min_transaction_fee: 1, // 1 NANO (aligned with transaction.rs constants)
            max_reorg_depth: 144,
            checkpoint_interval: 1000,
            finality_depth: 2016,
            genesis_supply: 50_000_000_000, // 50 NUMI (same as other blocks)
            mining_reward_halving_interval: 1_000_000,
            initial_mining_reward: 50_000_000_000, // 50 NUMI
        }
    }
}

impl ConsensusConfig {
    pub fn production() -> Self {
        Self {
            max_block_size: 8 * 1024 * 1024, // 8MB for production
            max_transactions_per_block: 50000,
            ..Default::default()
        }
    }

    pub fn development() -> Self {
        Self {
            target_block_time: Duration::from_secs(10), // Faster for testing
            difficulty_adjustment_interval: 20,
            max_block_size: 512 * 1024, // 512KB
            max_transactions_per_block: 100,
            max_reorg_depth: 10,
            checkpoint_interval: 50,
            finality_depth: 100,
            ..Default::default()
        }
    }

    pub fn testnet() -> Self {
        Self {
            target_block_time: Duration::from_secs(15), // 15 second blocks for testnet
            difficulty_adjustment_interval: 30,
            max_block_size: 1024 * 1024, // 1MB
            max_transactions_per_block: 500,
            min_transaction_fee: 1, // Same as mainnet for consistency
            max_reorg_depth: 20,
            checkpoint_interval: 100,
            finality_depth: 200,
            genesis_supply: 50_000_000_000, // 50 NUMI (same as other blocks)
            mining_reward_halving_interval: 1_000_000, // 1M blocks halving for testnet
            initial_mining_reward: 50_000_000_000, // 50 NUMI initial reward
            ..Default::default()
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.target_block_time.as_secs() == 0 {
            return Err("Target block time must be greater than 0".to_string());
        }
        if self.difficulty_adjustment_interval == 0 {
            return Err("Difficulty adjustment interval must be greater than 0".to_string());
        }
        if self.max_block_size < 1024 {
            return Err("Max block size too small".to_string());
        }
        if self.max_transactions_per_block == 0 {
            return Err("Max transactions per block must be greater than 0".to_string());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert!(config.validate().is_ok());
        assert!(config.network.enabled);
        assert_eq!(config.network.listen_port, 8333);
        assert_eq!(config.rpc.port, 8080);
    }

    #[test]
    fn test_production_config() {
        let config = Config::production();
        assert!(config.validate().is_ok());
        assert_eq!(config.network.max_peers, 100);
        assert!(config.security.enable_rate_limiting);
        assert!(config.security.require_https);
    }

    #[test]
    fn test_development_config() {
        let config = Config::development();
        assert!(config.validate().is_ok());
        assert_eq!(config.consensus.target_block_time, Duration::from_secs(10));
        assert!(!config.security.require_https);
    }

    #[test]
    fn test_testnet_config() {
        let config = Config::testnet();
        assert!(config.validate().is_ok());
        assert_eq!(config.consensus.target_block_time, Duration::from_secs(15));
        assert!(!config.security.require_https);
    }

    #[test]
    fn test_config_validation() {
        let mut config = Config::default();
        
        // Test invalid network port
        config.network.listen_port = 0;
        assert!(config.validate().is_err());
        
        // Test invalid JWT secret
        config.network.listen_port = 8333; // Fix port
        config.security.jwt_secret = "short".to_string();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_serialization() {
        let config = Config::default();
        let toml_str = toml::to_string(&config).unwrap();
        let deserialized: Config = toml::from_str(&toml_str).unwrap();
        
        assert_eq!(config.network.listen_port, deserialized.network.listen_port);
        assert_eq!(config.rpc.port, deserialized.rpc.port);
    }

    #[test]
    fn test_config_file_operations() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("config.toml");
        
        let config = Config::development();
        config.save_to_file(&config_path).unwrap();
        
        let loaded_config = Config::load_from_file(&config_path).unwrap();
        assert_eq!(config.mining.thread_count, loaded_config.mining.thread_count);
    }

    #[test]
    fn test_env_variable_overrides() {
        std::env::set_var("NUMI_NETWORK_PORT", "9999");
        std::env::set_var("NUMI_RPC_PORT", "8888");
        
        let mut config = Config::default();
        config.apply_env_overrides();
        
        assert_eq!(config.network.listen_port, 9999);
        assert_eq!(config.rpc.port, 8888);
        
        // Cleanup
        std::env::remove_var("NUMI_NETWORK_PORT");
        std::env::remove_var("NUMI_RPC_PORT");
    }
} 