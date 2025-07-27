use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use scrypt::{scrypt, Params as ScryptParams};
use serde::{Deserialize, Serialize};

use crate::crypto::{Dilithium3Keypair, derive_key, generate_random_bytes, blake3_hash, Hash};
use crate::{Result, BlockchainError};

// Security features implemented:
// - AES-256-GCM encryption with unique nonces and authentication tags
//
// - No key recovery by design (security feature)
// - Both public and private keys must be stored together
// - No key derivation support for enhanced security

/// Key derivation configuration for Scrypt
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyDerivationConfig {
    /// CPU/memory cost parameter (N)
    pub cost: u32,
    /// Block size parameter (r)
    pub block_size: u32,
    /// Parallelization parameter (p)
    pub parallelization: u32,
    /// Output key length
    pub key_length: usize,
    /// Salt length for randomization
    pub salt_length: usize,
}

impl Default for KeyDerivationConfig {
    fn default() -> Self {
        Self {
            cost: 1048576,      // 2^20, strong protection
            block_size: 8,       // Standard value
            parallelization: 1,  // Single-threaded
            key_length: 32,      // 256 bits
            salt_length: 32,     // 256-bit salt
        }
    }
}

impl KeyDerivationConfig {
    /// High security configuration for production
    pub fn high_security() -> Self {
        Self {
            cost: 2097152,      // 2^21, very strong
            block_size: 8,
            parallelization: 2,  // Dual-threaded
            key_length: 32,
            salt_length: 32,
        }
    }
    
    /// Fast configuration for development/testing
    pub fn development() -> Self {
        Self {
            cost: 16384,        // 2^14, much faster
            block_size: 8,
            parallelization: 1,
            key_length: 32,
            salt_length: 16,     // Shorter salt for speed
        }
    }
    
    pub fn test() -> Self {
        Self {
            cost: 1024,         // 2^10, very fast for tests
            block_size: 8,
            parallelization: 1,
            key_length: 32,
            salt_length: 16,
        }
    }
    
    /// Validate configuration parameters
    pub fn validate(&self) -> Result<()> {
        if self.cost < 1024 || self.cost > 67108864 {
            return Err(BlockchainError::CryptographyError(
                "Invalid Scrypt cost parameter".to_string()));
        }
        if self.block_size == 0 || self.block_size > 256 {
            return Err(BlockchainError::CryptographyError(
                "Invalid Scrypt block size".to_string()));
        }
        if self.parallelization == 0 || self.parallelization > 64 {
            return Err(BlockchainError::CryptographyError(
                "Invalid Scrypt parallelization parameter".to_string()));
        }
        if self.key_length < 16 || self.key_length > 64 {
            return Err(BlockchainError::CryptographyError(
                "Invalid key length".to_string()));
        }
        Ok(())
    }
}

/// Encrypted key entry with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedKeyEntry {
    /// Key identifier/name
    pub id: String,
    /// Encrypted key material
    pub encrypted_data: Vec<u8>,
    /// AES-GCM nonce (12 bytes)
    pub nonce: Vec<u8>,
    /// AES-GCM authentication tag (16 bytes)
    pub auth_tag: Vec<u8>,
    /// Scrypt salt for key derivation
    pub salt: Vec<u8>,
    /// Key derivation parameters
    pub kdf_params: KeyDerivationConfig,
    /// Creation timestamp
    pub created_at: u64,
    /// Last accessed timestamp
    pub last_accessed: u64,
    /// Expiry timestamp (0 = never expires)
    pub expires_at: u64,
    /// Key version for migration support
    pub version: u32,
    /// Additional metadata
    pub metadata: HashMap<String, String>,
}

impl EncryptedKeyEntry {
    /// Check if key has expired
    pub fn is_expired(&self) -> bool {
        if self.expires_at == 0 {
            return false; // Never expires
        }
        
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        now > self.expires_at
    }
    
    /// Update last accessed time
    pub fn touch(&mut self) {
        self.last_accessed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
    }
    
    /// Get age of the key in seconds
    pub fn age_seconds(&self) -> u64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        now.saturating_sub(self.created_at)
    }
}

/// Key store statistics and health information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyStoreStats {
    pub total_keys: usize,
    pub active_keys: usize,
    pub expired_keys: usize,
    pub oldest_key_age_seconds: u64,
    pub newest_key_age_seconds: u64,
    pub total_size_bytes: u64,
    pub last_backup_timestamp: u64,
    pub integrity_check_passed: bool,
}

/// Secure encrypted key storage with advanced security features
pub struct SecureKeyStore {
    /// Storage file path
    storage_path: PathBuf,
    /// Encrypted key entries
    keys: HashMap<String, EncryptedKeyEntry>,
    /// Master password hash for verification
    password_hash: Option<Hash>,
    /// Salt used for master password derivation
    password_salt: Vec<u8>,
    /// Key derivation configuration
    kdf_config: KeyDerivationConfig,
    /// Auto-save changes to disk
    auto_save: bool,
    /// Backup configuration
    backup_interval: Duration,
    last_backup: SystemTime,
}

impl SecureKeyStore {
    /// Create new secure key store
    pub fn new<P: AsRef<Path>>(storage_path: P) -> Result<Self> {
        Self::with_config(storage_path, KeyDerivationConfig::test())
    }
    
    /// Create key store with custom configuration
    pub fn with_config<P: AsRef<Path>>(
        storage_path: P,
        kdf_config: KeyDerivationConfig,
    ) -> Result<Self> {
        kdf_config.validate()?;
        
        Ok(Self {
            storage_path: storage_path.as_ref().to_path_buf(),
            keys: HashMap::new(),
            password_hash: None,
            password_salt: Vec::new(),
            kdf_config,
            auto_save: true,
            backup_interval: Duration::from_secs(24 * 3600), // 24 hours
            last_backup: SystemTime::UNIX_EPOCH,
        })
    }
    
    /// Initialize key store with master password
    pub fn initialize(&mut self, password: &str) -> Result<()> {
        if self.password_hash.is_some() {
            return Err(BlockchainError::CryptographyError(
                "Key store already initialized".to_string()));
        }
        
        // Generate random salt for master password
        let password_salt = generate_random_bytes(self.kdf_config.salt_length)?;
        // Derive a fixed-length seed from the password
        let seed = blake3_hash(password.as_bytes());
        let password_hash = derive_key(&seed, &String::from_utf8_lossy(&password_salt), b"keystore-auth")?;
        self.password_hash = Some(password_hash);
        self.password_salt = password_salt;
        
        log::info!("🔐 Secure key store initialized");
        
        if self.auto_save {
            self.save_to_disk()?;
        }
        
        Ok(())
    }
    
    /// Load key store from disk with decryption
    pub fn load_from_disk(&mut self, password: &str) -> Result<()> {
        if !self.storage_path.exists() {
            return Err(BlockchainError::StorageError(
                "Key store file does not exist".to_string()));
        }
        
        // Read encrypted data
        let encrypted_data = fs::read(&self.storage_path)
            .map_err(|e| BlockchainError::StorageError(format!("Failed to read key store: {e}")))?;
        
        if encrypted_data.len() < 64 {
            return Err(BlockchainError::StorageError(
                "Invalid key store file format".to_string()));
        }
        
        // Extract components: encryption_salt (32) | nonce (12) | password_salt (kdf_config.salt_length) | ciphertext
        let enc_salt_end = 32;
        let nonce_end = enc_salt_end + 12;
        let psalt_end = nonce_end + self.kdf_config.salt_length;

        if encrypted_data.len() < psalt_end + 16 { // must have at least tag bytes
            return Err(BlockchainError::StorageError("Invalid key store file format".to_string()));
        }
        let salt = &encrypted_data[0..enc_salt_end];
        let nonce = &encrypted_data[enc_salt_end..nonce_end];
        let file_password_salt = &encrypted_data[nonce_end..psalt_end];
        let encrypted_content = &encrypted_data[psalt_end..];
        // Temporarily set self.password_salt for key derivation
        self.password_salt = file_password_salt.to_vec();
        
        // Derive the temporary hash using password salt read from file header
        let seed = blake3_hash(password.as_bytes());
        let temp_hash = derive_key(&seed, &String::from_utf8_lossy(&self.password_salt), b"keystore-auth")?;
        
        let derived_password = format!("keystore_{}", hex::encode(temp_hash));
        
        // Derive key from password
        let key = self.derive_key_from_password(&derived_password, salt)?;
        
        // Decrypt data
        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|e| BlockchainError::CryptographyError(format!("Cipher creation failed: {e}")))?;
        
        let nonce_slice = Nonce::from_slice(nonce);
        let decrypted_data = cipher.decrypt(nonce_slice, encrypted_content)
            .map_err(|_| BlockchainError::CryptographyError("Decryption failed - invalid password or corrupted data".to_string()))?;
        
        // Deserialize key store data
        let store_data: StorageFormat = bincode::deserialize(&decrypted_data)
            .map_err(|e| BlockchainError::StorageError(format!("Failed to deserialize key store: {e}")))?;
        
        // Load data
        self.keys = store_data.keys;
        self.password_hash = Some(store_data.password_hash);
        // use value read earlier; but if store_data has salt field populated (new version) ensure consistency
        if !store_data.password_salt.is_empty() {
            self.password_salt = store_data.password_salt;
        }
        self.kdf_config = store_data.kdf_config;
        
        // Migrate legacy keystores without salt
        if self.password_salt.is_empty() {
            let new_salt = generate_random_bytes(self.kdf_config.salt_length)?;
            let seed = blake3_hash(password.as_bytes());
            let new_hash = derive_key(&seed, &String::from_utf8_lossy(&new_salt), b"keystore-auth")?;
            self.password_hash = Some(new_hash);
            self.password_salt = new_salt.clone();
            log::warn!("🔄 Migrated keystore to use random master password salt");
            if self.auto_save {
                self.save_to_disk()?;
            }
        }
        
        log::info!("🔓 Secure key store loaded with {} keys", self.keys.len());
        Ok(())
    }
    
    /// Save key store to disk with encryption
    pub fn save_to_disk(&self) -> Result<()> {
        let password_hash = self.password_hash
            .ok_or_else(|| BlockchainError::CryptographyError("Key store not initialized".to_string()))?;
        
        // Create storage data
        let store_data = StorageFormat {
            version: 2,
            keys: self.keys.clone(),
            password_hash,
            password_salt: self.password_salt.clone(),
            kdf_config: self.kdf_config.clone(),
            created_at: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
        };
        
        // Serialize data
        let serialized_data = bincode::serialize(&store_data)
            .map_err(|e| BlockchainError::StorageError(format!("Failed to serialize key store: {e}")))?;
        
        // Generate encryption parameters
        let salt = generate_random_bytes(32)?;
        let nonce = generate_random_bytes(12)?;
        
        // For persistence, we need to use a consistent encryption method
        // Since we don't have the original password here, we'll use a derived key from the password hash
        // This is a simplified approach - in production, you'd want to store the password securely
        let derived_password = format!("keystore_{}", hex::encode(password_hash));
        let key = self.derive_key_from_password(&derived_password, &salt)?;
        
        // Encrypt data
        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|e| BlockchainError::CryptographyError(format!("Cipher creation failed: {e}")))?;
        
        let nonce_slice = Nonce::from_slice(&nonce);
        let encrypted_data = cipher.encrypt(nonce_slice, serialized_data.as_ref())
            .map_err(|e| BlockchainError::CryptographyError(format!("Encryption failed: {e}")))?;
        
        // Combine components: encryption_salt(32) + nonce(12) + password_salt(kdf_config.salt_length) + encrypted_data
        let mut file_data = Vec::new();
        file_data.extend_from_slice(&salt);
        file_data.extend_from_slice(&nonce);
        file_data.extend_from_slice(&self.password_salt);
        file_data.extend_from_slice(&encrypted_data);
        
        // Write to temporary file first, then move (atomic operation)
        let temp_path = self.storage_path.with_extension("tmp");
        fs::write(&temp_path, &file_data)
            .map_err(|e| BlockchainError::StorageError(format!("Failed to write temp file: {e}")))?;
        
        fs::rename(&temp_path, &self.storage_path)
            .map_err(|e| BlockchainError::StorageError(format!("Failed to move temp file: {e}")))?;
        
        log::debug!("💾 Key store saved to disk with {} keys", self.keys.len());
        Ok(())
    }
    
    /// Store a Dilithium3 keypair with encryption
    pub fn store_keypair(&mut self, id: &str, keypair: &Dilithium3Keypair, password: &str) -> Result<()> {
        self.verify_password(password)?;
        
        // Serialize keypair
        let keypair_data = bincode::serialize(keypair)
            .map_err(|e| BlockchainError::CryptographyError(format!("Failed to serialize keypair: {e}")))?;
        
        // Generate encryption parameters
        // Use `?` to unwrap the `Result<Vec<u8>, BlockchainError>` returned by
        // `generate_random_bytes`, ensuring `salt` and `nonce` are plain
        // `Vec<u8>` values.
        let salt = generate_random_bytes(self.kdf_config.salt_length)?;
        let nonce = generate_random_bytes(12)?;
        
        // Derive key from password
        let key = self.derive_key_from_password(password, &salt)?;
        
        // Encrypt keypair data
        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|e| BlockchainError::CryptographyError(format!("Cipher creation failed: {e}")))?;
        
        let nonce_slice = Nonce::from_slice(&nonce);
        let encrypted_data = cipher.encrypt(nonce_slice, keypair_data.as_ref())
            .map_err(|e| BlockchainError::CryptographyError(format!("Encryption failed: {e}")))?;
        
        // Split encrypted data and auth tag (last 16 bytes)
        if encrypted_data.len() < 16 {
            return Err(BlockchainError::CryptographyError("Invalid encrypted data length".to_string()));
        }
        
        let (encrypted_key, auth_tag) = encrypted_data.split_at(encrypted_data.len() - 16);
        
        // Create key entry
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        let entry = EncryptedKeyEntry {
            id: id.to_string(),
            encrypted_data: encrypted_key.to_vec(),
            nonce,
            auth_tag: auth_tag.to_vec(),
            salt,
            kdf_params: self.kdf_config.clone(),
            created_at: now,
            last_accessed: now,
            expires_at: 0, // Never expires by default
            version: 1,
            metadata: HashMap::new(),
        };
        
        self.keys.insert(id.to_string(), entry);
        
        if self.auto_save {
            self.save_to_disk()?;
        }
        
        log::info!("🔑 Keypair '{id}' stored securely");
        Ok(())
    }
    
    /// Retrieve and decrypt a keypair
    pub fn get_keypair(&mut self, id: &str, password: &str) -> Result<Dilithium3Keypair> {
        self.verify_password(password)?;
        
        // First, get the salt and check expiration without mutable borrow
        let salt = {
            let entry = self.keys.get(id)
                .ok_or_else(|| BlockchainError::StorageError(format!("Key '{id}' not found")))?;
            
            // Check if key has expired
            if entry.is_expired() {
                return Err(BlockchainError::CryptographyError(format!("Key '{id}' has expired")));
            }
            
            entry.salt.clone()
        };
        
        // Now get mutable access for updating access time
        if let Some(entry) = self.keys.get_mut(id) {
            entry.touch();
        }
        
        // Get the entry data for decryption
        let entry = self.keys.get(id)
            .ok_or_else(|| BlockchainError::StorageError(format!("Key '{id}' not found")))?;
        
        // Derive key from password
        let key = self.derive_key_from_password(password, &salt)?;
        
        // Reconstruct full encrypted data (encrypted_data + auth_tag)
        let mut full_encrypted_data = entry.encrypted_data.clone();
        full_encrypted_data.extend_from_slice(&entry.auth_tag);
        
        // Decrypt keypair data
        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|e| BlockchainError::CryptographyError(format!("Cipher creation failed: {e}")))?;
        
        let nonce = Nonce::from_slice(&entry.nonce);
        let decrypted_data = cipher.decrypt(nonce, full_encrypted_data.as_ref())
            .map_err(|_| BlockchainError::CryptographyError("Failed to decrypt keypair".to_string()))?;
        
        // Deserialize keypair
        let keypair: Dilithium3Keypair = bincode::deserialize(&decrypted_data)
            .map_err(|e| BlockchainError::CryptographyError(format!("Failed to deserialize keypair: {e}")))?;
        
        if self.auto_save {
            self.save_to_disk()?;
        }
        
        log::debug!("🔓 Keypair '{id}' retrieved successfully");
        Ok(keypair)
    }
    
    /// List all stored key IDs
    pub fn list_keys(&self) -> Vec<String> {
        self.keys.keys().cloned().collect()
    }
    
    /// Remove a key from storage
    pub fn remove_key(&mut self, id: &str, password: &str) -> Result<()> {
        self.verify_password(password)?;
        
        self.keys.remove(id)
            .ok_or_else(|| BlockchainError::StorageError(format!("Key '{id}' not found")))?;
        
        if self.auto_save {
            self.save_to_disk()?;
        }
        
        log::info!("🗑️ Key '{id}' removed from secure storage");
        Ok(())
    }
    
    /// Set key expiration time
    pub fn set_key_expiry(&mut self, id: &str, expiry: SystemTime) -> Result<()> {
        let entry = self.keys.get_mut(id)
            .ok_or_else(|| BlockchainError::StorageError(format!("Key '{id}' not found")))?;
        
        entry.expires_at = expiry.duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        if self.auto_save {
            self.save_to_disk()?;
        }
        
        Ok(())
    }
    
    /// Clean up expired keys
    pub fn cleanup_expired_keys(&mut self) -> Result<usize> {
        let expired_keys: Vec<String> = self.keys.iter()
            .filter(|(_, entry)| entry.is_expired())
            .map(|(id, _)| id.clone())
            .collect();
        
        let count = expired_keys.len();
        for id in expired_keys {
            self.keys.remove(&id);
            log::debug!("🧹 Removed expired key: {id}");
        }
        
        if count > 0 && self.auto_save {
            self.save_to_disk()?;
        }
        
        Ok(count)
    }
    
    /// Get key store statistics
    pub fn get_stats(&self) -> KeyStoreStats {
        let total_keys = self.keys.len();
        let expired_keys = self.keys.values()
            .filter(|entry| entry.is_expired())
            .count();
        let active_keys = total_keys - expired_keys;
        
        let (oldest_age, newest_age) = self.keys.values()
            .map(|entry| entry.age_seconds())
            .fold((u64::MIN, u64::MAX), |(oldest, newest), age| {
                (oldest.max(age), newest.min(age))
            });
        
        let total_size_bytes = bincode::serialize(&self.keys)
            .map(|data| data.len() as u64)
            .unwrap_or(0);
        
        KeyStoreStats {
            total_keys,
            active_keys,
            expired_keys,
            oldest_key_age_seconds: if total_keys > 0 { oldest_age } else { 0 },
            newest_key_age_seconds: if total_keys > 0 { newest_age } else { 0 },
            total_size_bytes,
            last_backup_timestamp: self.last_backup.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs(),
            integrity_check_passed: self.verify_integrity().unwrap_or(false),
        }
    }
    
    /// Create encrypted backup of the key store
    pub fn create_backup<P: AsRef<Path>>(&self, backup_path: P, password: &str) -> Result<()> {
        self.verify_password(password)?;
        
        // Save current state to backup location
        let _current_path = self.storage_path.clone();
        let backup_store = SecureKeyStore {
            storage_path: backup_path.as_ref().to_path_buf(),
            keys: self.keys.clone(),
            password_hash: self.password_hash,
            password_salt: self.password_salt.clone(),
            kdf_config: self.kdf_config.clone(),
            auto_save: false,
            backup_interval: self.backup_interval,
            last_backup: self.last_backup,
        };
        
        backup_store.save_to_disk()?;
        
        log::info!("💾 Key store backup created: {:?}", backup_path.as_ref());
        Ok(())
    }
    
    /// Verify key store integrity
    pub fn verify_integrity(&self) -> Result<bool> {
        // Check if all keys have valid structure
        for (id, entry) in &self.keys {
            if entry.id != *id {
                return Ok(false);
            }
            if entry.encrypted_data.is_empty() {
                return Ok(false);
            }
            if entry.nonce.len() != 12 {
                return Ok(false);
            }
            if entry.auth_tag.len() != 16 {
                return Ok(false);
            }
            if entry.salt.len() != entry.kdf_params.salt_length {
                return Ok(false);
            }
        }
        
        Ok(true)
    }
    
    // Private helper methods
    
    /// Derive encryption key from password using Scrypt
    fn derive_key_from_password(&self, password: &str, salt: &[u8]) -> Result<Vec<u8>> {
        let params = ScryptParams::new(
            (self.kdf_config.cost as f64).log2() as u8,
            self.kdf_config.block_size,
            self.kdf_config.parallelization,
            self.kdf_config.key_length,
        ).map_err(|e| BlockchainError::CryptographyError(format!("Invalid Scrypt parameters: {e}")))?;
        
        let mut key = vec![0u8; self.kdf_config.key_length];
        scrypt(password.as_bytes(), salt, &params, &mut key)
            .map_err(|e| BlockchainError::CryptographyError(format!("Key derivation failed: {e}")))?;
        
        Ok(key)
    }
    
    /// Verify master password
    fn verify_password(&self, password: &str) -> Result<()> {
        let stored_hash = self.password_hash
            .ok_or_else(|| BlockchainError::CryptographyError("Key store not initialized".to_string()))?;
        
        // Derive a fixed-length seed from the password using stored salt
        let seed = blake3_hash(password.as_bytes());
        let test_hash = derive_key(&seed, &String::from_utf8_lossy(&self.password_salt), b"keystore-auth")?;
        
        // Note: This is simplified - proper implementation would use constant-time comparison
        if !crate::crypto::constant_time_eq(&test_hash, &stored_hash) {
            return Err(BlockchainError::CryptographyError("Invalid password".to_string()));
        }
        
        Ok(())
    }
}

/// Internal storage format for serialization
#[derive(Serialize, Deserialize)]
struct StorageFormat {
    version: u32,
    keys: HashMap<String, EncryptedKeyEntry>,
    password_hash: Hash,
    #[serde(default)]
    password_salt: Vec<u8>,
    kdf_config: KeyDerivationConfig,
    created_at: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    
    #[test]
    fn test_key_store_creation() {
        let temp_dir = TempDir::new().unwrap();
        let store_path = temp_dir.path().join("test_keystore.bin");
        
        let mut store = SecureKeyStore::new(&store_path).unwrap();
        assert!(store.initialize("test_password").is_ok());
    }
    
    #[test]
    fn test_keypair_storage_and_retrieval() {
        let temp_dir = TempDir::new().unwrap();
        let store_path = temp_dir.path().join("test_keystore.bin");
        
        let mut store = SecureKeyStore::new(&store_path).unwrap();
        store.initialize("test_password").unwrap();
        
        // Create test keypair
        let keypair = Dilithium3Keypair::new().unwrap();
        let original_pubkey = keypair.public_key.clone();
        
        // Store keypair
        store.store_keypair("test_key", &keypair, "test_password").unwrap();
        
        // Retrieve keypair
        let retrieved = store.get_keypair("test_key", "test_password").unwrap();
        assert_eq!(retrieved.public_key, original_pubkey);
    }
    
    #[test]
    fn test_invalid_password() {
        let temp_dir = TempDir::new().unwrap();
        let store_path = temp_dir.path().join("test_keystore.bin");
        
        let mut store = SecureKeyStore::new(&store_path).unwrap();
        store.initialize("correct_password").unwrap();
        
        let keypair = Dilithium3Keypair::new().unwrap();
        store.store_keypair("test_key", &keypair, "correct_password").unwrap();
        
        // Try with wrong password
        assert!(store.get_keypair("test_key", "wrong_password").is_err());
    }
    
    #[test]
    fn test_key_expiry() {
        let temp_dir = TempDir::new().unwrap();
        let store_path = temp_dir.path().join("test_keystore.bin");
        
        let mut store = SecureKeyStore::new(&store_path).unwrap();
        store.initialize("test_password").unwrap();
        
        let keypair = Dilithium3Keypair::new().unwrap();
        store.store_keypair("test_key", &keypair, "test_password").unwrap();
        
        // Set expiry to past time
        let past_time = SystemTime::UNIX_EPOCH + Duration::from_secs(1);
        store.set_key_expiry("test_key", past_time).unwrap();
        
        // Should fail to retrieve expired key
        assert!(store.get_keypair("test_key", "test_password").is_err());
    }
    
    #[test]
    fn test_key_cleanup() {
        let temp_dir = TempDir::new().unwrap();
        let store_path = temp_dir.path().join("test_keystore.bin");
        
        let mut store = SecureKeyStore::new(&store_path).unwrap();
        store.initialize("test_password").unwrap();
        
        // Add multiple keys
        for i in 0..5 {
            let keypair = Dilithium3Keypair::new().unwrap();
            store.store_keypair(&format!("key_{i}"), &keypair, "test_password").unwrap();
        }
        
        // Expire some keys
        let past_time = SystemTime::UNIX_EPOCH + Duration::from_secs(1);
        store.set_key_expiry("key_0", past_time).unwrap();
        store.set_key_expiry("key_1", past_time).unwrap();
        
        // Clean up expired keys
        let removed_count = store.cleanup_expired_keys().unwrap();
        assert_eq!(removed_count, 2);
        assert_eq!(store.list_keys().len(), 3);
    }
    
    #[test]
    fn test_persistence() {
        let temp_dir = TempDir::new().unwrap();
        let store_path = temp_dir.path().join("test_keystore.bin");
        
        let original_pubkey = {
            let mut store = SecureKeyStore::new(&store_path).unwrap();
            store.initialize("test_password").unwrap();
            
            let keypair = Dilithium3Keypair::new().unwrap();
            let pubkey = keypair.public_key.clone();
            store.store_keypair("persistent_key", &keypair, "test_password").unwrap();
            pubkey
        };
        
        // Load store from disk
        let mut new_store = SecureKeyStore::new(&store_path).unwrap();
        new_store.load_from_disk("test_password").unwrap();
        
        let retrieved = new_store.get_keypair("persistent_key", "test_password").unwrap();
        assert_eq!(retrieved.public_key, original_pubkey);
    }
} 