use crate::{
    crypto::Dilithium3Keypair,
    Result,
};
use std::path::Path;

/// Shared wallet and mining utility functions
pub struct WalletManager;

impl WalletManager {
    /// Calculate mining reward based on configurable halving schedule
    pub fn calculate_mining_reward(height: u64) -> u64 {
        Self::calculate_mining_reward_with_config(height, &Default::default())
    }
    
    /// Calculate mining reward with custom configuration
    pub fn calculate_mining_reward_with_config(height: u64, config: &crate::config::ConsensusConfig) -> u64 {
        let halving_interval = config.mining_reward_halving_interval;
        let initial_reward = config.initial_mining_reward;

        let halvings = height / halving_interval;
        if halvings >= 64 {
            return 0;
        }
        initial_reward >> halvings
    }
    
    /// Load or create miner wallet with consistent logic
    pub fn load_or_create_miner_wallet(data_directory: &Path) -> Result<Dilithium3Keypair> {
        let wallet_path = data_directory.join("miner-wallet.json");
        
        match Dilithium3Keypair::load_from_file(&wallet_path) {
            Ok(kp) => {
                log::info!("🔑 Loaded existing miner wallet from {wallet_path:?}");
                Ok(kp)
            }
            Err(_) => {
                log::info!("🔑 Creating new miner keypair (no existing wallet found at {wallet_path:?})");
                let kp = Dilithium3Keypair::new()?;
                
                // Ensure parent directory exists
                if let Some(parent) = wallet_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                
                if let Err(e) = kp.save_to_file(&wallet_path) {
                    log::warn!("⚠️ Failed to save new keypair to {wallet_path:?}: {e}");
                } else {
                    log::info!("✅ New miner wallet saved to {wallet_path:?}");
                }
                Ok(kp)
            }
        }
    }
    
    /// Load or create miner wallet with custom path
    pub fn load_or_create_miner_wallet_at_path(wallet_path: &Path) -> Result<Dilithium3Keypair> {
        match Dilithium3Keypair::load_from_file(wallet_path) {
            Ok(kp) => {
                log::warn!("🔑 Loaded existing miner wallet from {wallet_path:?}");
                Ok(kp)
            }
            Err(_) => {
                log::warn!("🔑 Creating new miner keypair (no wallet found at {wallet_path:?})");
                let kp = Dilithium3Keypair::new()?;
                
                // Ensure parent directory exists
                if let Some(parent) = wallet_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                
                // Save the new keypair
                if let Err(e) = kp.save_to_file(wallet_path) {
                    log::warn!("⚠️ Failed to save new keypair to {wallet_path:?}: {e}");
                } else {
                    log::info!("✅ New miner wallet saved to {wallet_path:?}");
                }
                Ok(kp)
            }
        }
    }
}

/// Miner key management for Stratum V2 - CPU mining removed, handled by external miners
pub struct Miner {
    /// Miner's keypair for signing blocks
    keypair: Dilithium3Keypair,
}

impl Miner {
    /// Create new miner: only initializes the keypair
    pub fn new() -> Result<Self> {
        let kp = Dilithium3Keypair::new()?;
        Ok(Self { keypair: kp })
    }
    
    /// Create miner from existing keypair file
    pub fn from_wallet_path(wallet_path: &Path) -> Result<Self> {
        let keypair = WalletManager::load_or_create_miner_wallet_at_path(wallet_path)?;
        Ok(Self { keypair })
    }
    
    /// Get the miner's public key for identification
    pub fn get_public_key(&self) -> Vec<u8> {
        self.keypair.public_key_bytes().to_vec()
    }

    /// Access the full keypair for signing operations
    pub fn get_keypair(&self) -> &Dilithium3Keypair {
        &self.keypair
    }
    
    /// Get the miner's address for balance tracking
    pub fn get_address(&self) -> String {
        // This would use the same address derivation as the blockchain
        crate::crypto::derive_address_from_public_key(&self.get_public_key())
            .unwrap_or_else(|_| "invalid_address".to_string())
    }
} 

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halving_schedule_default_config() {
        let cfg = crate::config::ConsensusConfig::default();
        let interval = cfg.mining_reward_halving_interval;
        let initial = cfg.initial_mining_reward;

        // Reward at height 0 = initial reward
        assert_eq!(WalletManager::calculate_mining_reward_with_config(0, &cfg), initial);

        // Exactly at halving interval -> half
        assert_eq!(WalletManager::calculate_mining_reward_with_config(interval, &cfg), initial >> 1);

        // After two halvings
        assert_eq!(WalletManager::calculate_mining_reward_with_config(interval * 2, &cfg), initial >> 2);
    }
}