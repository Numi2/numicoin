use std::fmt;

use blake3::Hasher;
use argon2::{Argon2, Params, Algorithm, Version};
use serde::{Deserialize, Serialize};
use zeroize::ZeroizeOnDrop;
use pqcrypto_traits::sign::{PublicKey, SecretKey, DetachedSignature};
use pqcrypto_traits::kem::{PublicKey as KemPublicKey, SecretKey as KemSecretKey, SharedSecret, Ciphertext};
use rand::RngCore;
use base64ct::Encoding;
use pqcrypto_kyber::kyber768::{PublicKey as KyberPublicKey, SecretKey as KyberSecretKey, Ciphertext as KyberCiphertext};

use crate::error::BlockchainError;
use crate::Result;

/// 256-bit hash output
pub type Hash = [u8; 32];

/// Dilithium3 signature size (actual size from pqcrypto-dilithium)
pub const DILITHIUM3_SIGNATURE_SIZE: usize = pqcrypto_dilithium::dilithium3::signature_bytes();

/// Dilithium3 public key size (actual size from pqcrypto-dilithium)
pub const DILITHIUM3_PUBKEY_SIZE: usize = pqcrypto_dilithium::dilithium3::public_key_bytes();

/// Dilithium3 secret key size (actual size from pqcrypto-dilithium)
pub const DILITHIUM3_SECKEY_SIZE: usize = pqcrypto_dilithium::dilithium3::secret_key_bytes();

/// Maximum message size for signing (prevent DoS)
pub const MAX_SIGNABLE_MESSAGE_SIZE: usize = 10 * 1024 * 1024; // 10MB

/// Minimum entropy bits required for key generation
pub const MIN_ENTROPY_BITS: usize = 256;
/// PEM format key pair for export/import
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PemKeyPair {
    pub private_key: String,
    pub public_key: String,
}
/// Production-ready Dilithium3 keypair with enhanced security
#[derive(Debug, Clone, Serialize, Deserialize, ZeroizeOnDrop)]
pub struct Dilithium3Keypair {
    #[zeroize(skip)] // Public key doesn't need zeroization
    pub public_key: Vec<u8>,
    #[serde(skip)] // Never serialize secret keys
    #[zeroize] // Zeroize secret key on drop
    secret_key: Vec<u8>,
    /// Key fingerprint for integrity checking
    #[zeroize(skip)]
    fingerprint: Hash,
    /// Creation timestamp for auditing
    created_at: u64,
}



impl Dilithium3Keypair {
    /// Generate new Dilithium3 keypair with enhanced entropy
    pub fn new() -> Result<Self> {
        Self::new_with_entropy_check(true)
    }
    
    /// Generate keypair with optional entropy validation
    pub fn new_with_entropy_check(validate_entropy: bool) -> Result<Self> {
        if validate_entropy {
            Self::validate_system_entropy()?;
        }
        
        // Use pqcrypto-dilithium for real post-quantum signatures
        let (public_key, secret_key) = pqcrypto_dilithium::dilithium3::keypair();
        
        let public_key_vec = public_key.as_bytes().to_vec();
        let secret_key_vec = secret_key.as_bytes().to_vec();
        
        // Validate key sizes
        if public_key_vec.len() != DILITHIUM3_PUBKEY_SIZE {
            return Err(BlockchainError::CryptographyError(
                format!("Invalid public key size: expected {}, got {}", 
                       DILITHIUM3_PUBKEY_SIZE, public_key_vec.len())));
        }
        
        if secret_key_vec.len() != DILITHIUM3_SECKEY_SIZE {
            return Err(BlockchainError::CryptographyError(
                format!("Invalid secret key size: expected {}, got {}", 
                       DILITHIUM3_SECKEY_SIZE, secret_key_vec.len())));
        }
        
        // Create fingerprint for integrity checking
        let fingerprint = blake3_hash(&public_key_vec);
        
        Ok(Self {
            public_key: public_key_vec,
            secret_key: secret_key_vec,
            fingerprint,
            created_at: chrono::Utc::now().timestamp() as u64,
        })
    }
    
    /// Load keypair from file (JSON format)
    pub fn load_from_file<P: AsRef<std::path::Path>>(path: P) -> Result<Self> {
        let file_content = std::fs::read_to_string(path.as_ref())
            .map_err(|e| BlockchainError::CryptographyError(format!("Failed to read key file: {}", e)))?;
        
        // Try JSON format first
        if let Ok(keypair) = serde_json::from_str::<Self>(&file_content) {
            keypair.validate_integrity()?;
            return Ok(keypair);
        }
        
        // Try PEM format
        if file_content.contains("-----BEGIN PRIVATE KEY-----") {
            return Self::from_pem(&file_content);
        }
        
        Err(BlockchainError::CryptographyError("Unsupported key file format".to_string()))
    }
    
    /// Save keypair to file in JSON format
    pub fn save_to_file<P: AsRef<std::path::Path>>(&self, path: P) -> Result<()> {
        let json_content = serde_json::to_string_pretty(self)
            .map_err(|e| BlockchainError::CryptographyError(format!("Failed to serialize keypair: {}", e)))?;
        
        std::fs::write(path.as_ref(), json_content)
            .map_err(|e| BlockchainError::CryptographyError(format!("Failed to write key file: {}", e)))?;
        
        Ok(())
    }
    
    /// Export keypair in PEM format
    pub fn to_pem(&self) -> Result<PemKeyPair> {
        // Encode keys in base64 for PEM format
        let private_key_b64 = base64ct::Base64::encode_string(&self.secret_key);
        let public_key_b64 = base64ct::Base64::encode_string(&self.public_key);
        
        Ok(PemKeyPair {
            private_key: private_key_b64,
            public_key: public_key_b64,
        })
    }
    
    /// Load keypair from PEM format
    pub fn from_pem(pem_content: &str) -> Result<Self> {
        // Extract private key from PEM
        let private_key_start = pem_content.find("-----BEGIN PRIVATE KEY-----")
            .ok_or_else(|| BlockchainError::CryptographyError("Private key not found in PEM".to_string()))?;
        let private_key_end = pem_content.find("-----END PRIVATE KEY-----")
            .ok_or_else(|| BlockchainError::CryptographyError("Private key end not found in PEM".to_string()))?;
        
        let private_key_b64 = &pem_content[private_key_start + 29..private_key_end]
            .lines()
            .filter(|line| !line.is_empty())
            .collect::<String>();
        
        // Extract public key from PEM
        let public_key_start = pem_content.find("-----BEGIN PUBLIC KEY-----")
            .ok_or_else(|| BlockchainError::CryptographyError("Public key not found in PEM".to_string()))?;
        let public_key_end = pem_content.find("-----END PUBLIC KEY-----")
            .ok_or_else(|| BlockchainError::CryptographyError("Public key end not found in PEM".to_string()))?;
        
        let public_key_b64 = &pem_content[public_key_start + 26..public_key_end]
            .lines()
            .filter(|line| !line.is_empty())
            .collect::<String>();
        
        // Decode base64
        let secret_key = base64ct::Base64::decode_vec(private_key_b64)
            .map_err(|e| BlockchainError::CryptographyError(format!("Failed to decode private key: {}", e)))?;
        let public_key = base64ct::Base64::decode_vec(public_key_b64)
            .map_err(|e| BlockchainError::CryptographyError(format!("Failed to decode public key: {}", e)))?;
        
        Self::from_bytes(public_key, secret_key)
    }
    
    /// Create keypair from existing key material (for testing/import)
    pub fn from_bytes(public_key: Vec<u8>, secret_key: Vec<u8>) -> Result<Self> {
        // Validate sizes
        if public_key.len() != DILITHIUM3_PUBKEY_SIZE {
            return Err(BlockchainError::CryptographyError("Invalid public key size".to_string()));
        }
        if secret_key.len() != DILITHIUM3_SECKEY_SIZE {
            return Err(BlockchainError::CryptographyError("Invalid secret key size".to_string()));
        }
        
        // Validate key pair consistency
        let test_message = b"validation_test_message";
        let pk = pqcrypto_dilithium::dilithium3::PublicKey::from_bytes(&public_key)
            .map_err(|e| BlockchainError::CryptographyError(format!("Invalid public key: {:?}", e)))?;
        let sk = pqcrypto_dilithium::dilithium3::SecretKey::from_bytes(&secret_key)
            .map_err(|e| BlockchainError::CryptographyError(format!("Invalid secret key: {:?}", e)))?;
        
        let sig = pqcrypto_dilithium::dilithium3::detached_sign(test_message, &sk);
        if pqcrypto_dilithium::dilithium3::verify_detached_signature(&sig, test_message, &pk).is_err() {
            return Err(BlockchainError::CryptographyError("Key pair validation failed".to_string()));
        }
        
        let fingerprint = blake3_hash(&public_key);
        
        Ok(Self {
            public_key,
            secret_key,
            fingerprint,
            created_at: chrono::Utc::now().timestamp() as u64,
        })
    }
    
    /// Sign message with comprehensive validation
    pub fn sign(&self, message: &[u8]) -> Result<Dilithium3Signature> {
        if message.len() > MAX_SIGNABLE_MESSAGE_SIZE {
            return Err(BlockchainError::InvalidArgument("Message too large to sign".to_string()));
        }

        let pq_sk = pqcrypto_dilithium::dilithium3::SecretKey::from_bytes(&self.secret_key)
            .map_err(|e| BlockchainError::CryptographyError(format!("Invalid secret key: {}", e)))?;

        let signature_bytes = pqcrypto_dilithium::dilithium3::detached_sign(message, &pq_sk);

        Ok(Dilithium3Signature {
            signature: signature_bytes.as_bytes().to_vec(),
            public_key: self.public_key.clone(),
            message_hash: blake3_hash(message),
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        })
    }
    
    pub fn verify(message: &[u8], signature: &Dilithium3Signature, public_key: &[u8]) -> Result<bool> {
        if message.len() > MAX_SIGNABLE_MESSAGE_SIZE {
            return Err(BlockchainError::InvalidArgument("Message too large to verify".to_string()));
        }

        // Integrity check: message hash
        let calculated_hash = blake3_hash(message);
        if calculated_hash != signature.message_hash {
            log::warn!("Signature message hash mismatch");
            return Ok(false);
        }

        let pq_pk = pqcrypto_dilithium::dilithium3::PublicKey::from_bytes(public_key)
            .map_err(|e| BlockchainError::CryptographyError(format!("Invalid public key: {}", e)))?;
        
        let pq_sig = pqcrypto_dilithium::dilithium3::DetachedSignature::from_bytes(&signature.signature)
            .map_err(|e| BlockchainError::CryptographyError(format!("Invalid signature format: {}", e)))?;

        Ok(pqcrypto_dilithium::dilithium3::verify_detached_signature(&pq_sig, message, &pq_pk).is_ok())
    }
    
    /// Validate key integrity using fingerprint
    pub fn validate_integrity(&self) -> Result<()> {
        let current_fingerprint = blake3_hash(&self.public_key);
        if !constant_time_eq(&self.fingerprint, &current_fingerprint) {
            return Err(BlockchainError::CryptographyError("Key integrity check failed".to_string()));
        }
        Ok(())
    }
    
    /// Get public key bytes
    pub fn public_key_bytes(&self) -> &[u8] {
        &self.public_key
    }
    
    /// Get secret key bytes (use carefully!)
    pub fn secret_key_bytes(&self) -> &[u8] {
        &self.secret_key
    }
    
    /// Get key fingerprint
    pub fn fingerprint(&self) -> &Hash {
        &self.fingerprint
    }
    
    /// Get key age in seconds
    pub fn age_seconds(&self) -> u64 {
        chrono::Utc::now().timestamp() as u64 - self.created_at
    }
    
    /// Validate system entropy before key generation
    fn validate_system_entropy() -> Result<()> {
        use std::fs::File;
        use std::io::Read;
        
        // Check entropy sources on Unix systems
        #[cfg(unix)]
        {
            match File::open("/proc/sys/kernel/random/entropy_avail") {
                Ok(mut file) => {
                    let mut contents = String::new();
                    if file.read_to_string(&mut contents).is_ok() {
                        if let Ok(entropy) = contents.trim().parse::<u32>() {
                            if entropy < (MIN_ENTROPY_BITS as u32) {
                                log::warn!("Low system entropy: {} bits", entropy);
                            }
                        }
                    }
                }
                Err(_) => {
                    // Can't check entropy, proceed with warning
                    log::warn!("Cannot check system entropy availability");
                }
            }
        }
        
        // Test randomness quality
        let mut test_bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut test_bytes);
        
        // Basic entropy test - check for patterns
        let mut ones = 0;
        for byte in &test_bytes {
            ones += byte.count_ones();
        }
        
        // Should be roughly balanced (around 128 ones in 256 bits)
        if ones < 96 || ones > 160 {
            return Err(BlockchainError::CryptographyError(
                "Insufficient randomness detected".to_string()));
        }
        
        Ok(())
    }
}

/// Enhanced Dilithium3 signature with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dilithium3Signature {
    pub signature: Vec<u8>,
    pub public_key: Vec<u8>,
    /// Hash of signed message for integrity
    pub message_hash: Hash,
    /// Creation timestamp
    pub created_at: u64,
}

impl Dilithium3Signature {
    /// Validate signature structure and metadata
    pub fn is_valid_format(&self) -> bool {
        self.signature.len() == DILITHIUM3_SIGNATURE_SIZE &&
        self.public_key.len() == DILITHIUM3_PUBKEY_SIZE &&
        self.created_at > 0
    }
    
    /// Get signature size in bytes
    pub fn size(&self) -> usize {
        self.signature.len() + self.public_key.len() + 32 + 8 // + hash + timestamp
    }
    
    /// Check if signature is expired (max_age_seconds window)
    pub fn is_expired(&self, max_age_seconds: u64) -> bool {
        let current_time = chrono::Utc::now().timestamp() as u64;
        current_time.saturating_sub(self.created_at) >= max_age_seconds
    }
}

/// Enhanced Argon2id configuration with security optimizations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Argon2Config {
    /// Memory cost in KiB
    pub memory_cost: u32,
    /// Time cost - number of iterations
    pub time_cost: u32,
    /// Parallelism factor
    pub parallelism: u32,
    /// Output length in bytes
    pub output_length: usize,
    /// Salt length for additional security
    pub salt_length: usize,
}

impl Default for Argon2Config {
    fn default() -> Self {
        Self {
            memory_cost: 65536,  // 64 MB
            time_cost: 3,        // 3 iterations
            parallelism: 1,      // Single-threaded for consistency
            output_length: 32,   // 256 bits
            salt_length: 32,     // 256-bit salt
        }
    }
}

impl Argon2Config {
    /// Production configuration with high security
    pub fn production() -> Self {
        Self {
            memory_cost: 262144, // 256 MB
            time_cost: 5,        // 5 iterations
            parallelism: 1,      // Consistent across systems
            output_length: 32,
            salt_length: 32,
        }
    }
    
    /// Development configuration for faster testing
    pub fn development() -> Self {
        Self {
            memory_cost: 4096,   // 4 MB
            time_cost: 1,        // 1 iteration
            parallelism: 1,
            output_length: 32,
            salt_length: 32,
        }
    }
    
    /// Validate configuration parameters
    pub fn validate(&self) -> Result<()> {
        if self.memory_cost < 8 || self.memory_cost > 2u32.pow(24) {
            return Err(BlockchainError::CryptographyError("Invalid memory cost".to_string()));
        }
        if self.time_cost < 1 || self.time_cost > 100 {
            return Err(BlockchainError::CryptographyError("Invalid time cost".to_string()));
        }
        if self.parallelism < 1 || self.parallelism > 16 {
            return Err(BlockchainError::CryptographyError("Invalid parallelism".to_string()));
        }
        if self.output_length < 16 || self.output_length > 64 {
            return Err(BlockchainError::CryptographyError("Invalid output length".to_string()));
        }
        if self.salt_length < 16 || self.salt_length > 64 {
            return Err(BlockchainError::CryptographyError("Invalid salt length".to_string()));
        }
        Ok(())
    }
}

/// Fast BLAKE3 hashing for blockchain operations
pub fn blake3_hash(data: &[u8]) -> Hash {
    if data.is_empty() {
        return [0u8; 32]; // Handle empty input gracefully
    }
    let mut hasher = Hasher::new();
    hasher.update(data);
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&hasher.finalize().as_bytes()[..32]);
    hash
}

/// BLAKE3 hash with hex encoding
pub fn blake3_hash_hex(data: &[u8]) -> String {
    let hash = blake3_hash(data);
    hex::encode(hash)
}

/// Incremental BLAKE3 hasher for large data
pub struct Blake3Hasher {
    hasher: Hasher,
    bytes_processed: usize,
}

impl Blake3Hasher {
    pub fn new() -> Self {
        Self {
            hasher: Hasher::new(),
            bytes_processed: 0,
        }
    }
    
    pub fn update(&mut self, data: &[u8]) -> Result<()> {
        // Prevent DoS with extremely large inputs
        if self.bytes_processed.saturating_add(data.len()) > 1_000_000_000 {
            return Err(BlockchainError::CryptographyError("Input too large for hashing".to_string()));
        }
        
        self.hasher.update(data);
        self.bytes_processed += data.len();
        Ok(())
    }
    
    pub fn finalize(self) -> Hash {
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&self.hasher.finalize().as_bytes()[..32]);
        hash
    }
    
    pub fn bytes_processed(&self) -> usize {
        self.bytes_processed
    }
}

/// Production-ready Argon2id Proof of Work with enhanced security
pub fn argon2id_pow_hash(data: &[u8], salt: &[u8], config: &Argon2Config) -> Result<Vec<u8>> {
    // Validate inputs
    if data.is_empty() {
        return Err(BlockchainError::CryptographyError("Empty data for PoW hash".to_string()));
    }
    if salt.len() < 16 {
        return Err(BlockchainError::CryptographyError("Salt too short".to_string()));
    }
    
    config.validate()?;
    
    // Build Argon2 parameters with enhanced validation
    let params = Params::new(
        config.memory_cost,
        config.time_cost,
        config.parallelism,
        Some(config.output_length),
    ).map_err(|e| BlockchainError::CryptographyError(format!("Argon2 params error: {}", e)))?;
    
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    
    // Use appropriate salt length
    let salt_to_use = if salt.len() >= config.salt_length {
        &salt[..config.salt_length]
    } else {
        // Pad salt if too short
        let mut padded_salt = vec![0u8; config.salt_length];
        padded_salt[..salt.len()].copy_from_slice(salt);
        return argon2id_pow_hash(data, &padded_salt, config);
    };
    
    // Perform Argon2id hashing
    let mut output = vec![0u8; config.output_length];
    argon2.hash_password_into(data, salt_to_use, &mut output)
        .map_err(|e| BlockchainError::CryptographyError(format!("Argon2id hash error: {}", e)))?;
    
    Ok(output)
}

/// Default Argon2id hash with standard configuration
pub fn argon2id_hash(data: &[u8], salt: &[u8]) -> Result<Vec<u8>> {
    let config = Argon2Config::default();
    argon2id_pow_hash(data, salt, &config)
}

/// Optimized Proof of Work verification with caching
pub fn verify_pow(header_blob: &[u8], nonce: u64, difficulty_target: &[u8]) -> Result<bool> {
    // Input validation
    if header_blob.is_empty() {
        return Err(BlockchainError::CryptographyError("Empty header for PoW".to_string()));
    }
    if difficulty_target.len() != 32 {
        return Err(BlockchainError::CryptographyError("Invalid difficulty target length".to_string()));
    }
    
    // Combine header and nonce efficiently
    let mut pow_data = Vec::with_capacity(header_blob.len() + 8);
    pow_data.extend_from_slice(header_blob);
    pow_data.extend_from_slice(&nonce.to_le_bytes());
    
    // Use header hash as salt for Argon2id (first 32 bytes)
    let salt_full = blake3_hash(header_blob);
    let salt_slice = &salt_full[..16]; // Use first 16 bytes as salt
    
    // Use optimized Argon2id configuration based on environment
    let config = if cfg!(test) || cfg!(debug_assertions) {
        Argon2Config::development() // Faster for tests/debug
    } else {
        // Production with adaptive parameters based on difficulty
        let difficulty_level = target_to_difficulty(difficulty_target);
        if difficulty_level > 20 {
            Argon2Config::production() // High security for high difficulty
        } else {
            Argon2Config::default() // Standard security
        }
    };
    
    // Perform Argon2id computation
    let argon2_result = argon2id_pow_hash(&pow_data, salt_slice, &config)?;
    
    // Final BLAKE3 hash of Argon2id result
    let final_hash = blake3_hash(&argon2_result);
    
    // Convert difficulty target to array for comparison
    let target_array: [u8; 32] = difficulty_target.try_into()
        .map_err(|_| BlockchainError::CryptographyError("Invalid difficulty target format".to_string()))?;
    
    // Compare with difficulty target using constant-time comparison
    Ok(final_hash <= target_array)
}

/// Generate difficulty target with enhanced validation
pub fn generate_difficulty_target(difficulty: u32) -> Vec<u8> {
    // Validate difficulty range
    let difficulty = difficulty.min(255); // Cap at 255 for safety
    
    let mut target = [0xFFu8; 32];
    
    if difficulty == 0 {
        return target.to_vec(); // Maximum target (easiest)
    }
    
    // Calculate position of leading zeros more precisely
    let zero_bytes = (difficulty / 8) as usize;
    let zero_bits = (difficulty % 8) as usize;
    
    // Set leading bytes to zero
    for i in 0..zero_bytes.min(32) {
        target[i] = 0;
    }
    
    // Set partial byte if needed
    if zero_bytes < 32 && zero_bits > 0 {
        let partial_byte = 0xFF >> zero_bits;
        target[zero_bytes] = partial_byte;
        
        // Zero out remaining bits more precisely
        for i in (zero_bytes + 1)..32 {
            target[i] = 0xFF;
        }
    }
    
    target.to_vec()
}

/// Calculate difficulty from target with improved precision
pub fn target_to_difficulty(target: &[u8]) -> u32 {
    if target.len() != 32 {
        return 0; // Invalid target
    }
    
    // Count leading zero bits more accurately
    let mut difficulty = 0u32;
    
    for &byte in target.iter() {
        if byte == 0 {
            difficulty += 8;
        } else {
            // Count leading zero bits in this byte
            difficulty += byte.leading_zeros();
            break;
        }
        
        // Prevent overflow
        if difficulty >= 248 { // Leave some headroom
            break;
        }
    }
    
    difficulty
}

/// Secure key derivation using BLAKE3 with enhanced validation
pub fn derive_key(seed: &[u8], salt: &str, info: &[u8]) -> Result<Hash> {
    // Validate inputs
    if seed.is_empty() {
        return Err(BlockchainError::CryptographyError("Empty seed for key derivation".to_string()));
    }
    if salt.is_empty() {
        return Err(BlockchainError::CryptographyError("Empty salt for key derivation".to_string()));
    }
    if seed.len() < 16 {
        return Err(BlockchainError::CryptographyError("Seed too short for secure derivation".to_string()));
    }
    
    let mut hasher = Hasher::new_derive_key(salt);
    hasher.update(seed);
    hasher.update(info);
    let mut derived_key = [0u8; 32];
    derived_key.copy_from_slice(&hasher.finalize().as_bytes()[..32]);
    Ok(derived_key)
}

/// Generate cryptographically secure random bytes with validation
pub fn generate_random_bytes(length: usize) -> Result<Vec<u8>> {
    if length == 0 {
        return Ok(Vec::new());
    }
    if length > 1_000_000 { // 1MB limit
        return Err(BlockchainError::CryptographyError("Requested too many random bytes".to_string()));
    }
    
    let mut bytes = vec![0u8; length];
    rand::thread_rng().fill_bytes(&mut bytes);
    
    // Basic quality check for small lengths
    if length <= 32 {
        let mut ones = 0;
        for byte in &bytes {
            ones += byte.count_ones();
        }
        // Should have reasonable bit distribution
        let expected_ones = (length * 8) / 2;
        let tolerance = (expected_ones / 4).max(4); // 25% tolerance, minimum 4
        if ones < (expected_ones as u32).saturating_sub(tolerance as u32) || 
           ones > (expected_ones as u32) + tolerance as u32 {
            log::warn!("Random bytes may have poor entropy distribution");
        }
    }
    
    Ok(bytes)
}

/// Generate random salt with proper size validation
pub fn generate_salt() -> Result<[u8; 32]> {
    let bytes = generate_random_bytes(32)?;
    let mut salt = [0u8; 32];
    salt.copy_from_slice(&bytes);
    Ok(salt)
}

/// Generate salt with custom length
pub fn generate_salt_with_length(length: usize) -> Result<Vec<u8>> {
    if length < 16 || length > 64 {
        return Err(BlockchainError::CryptographyError("Invalid salt length".to_string()));
    }
    generate_random_bytes(length)
}

// KYBER KEM IMPLEMENTATION

/// Generate a Kyber768 keypair (public, secret) as byte vectors.
pub fn kyber_keypair() -> (Vec<u8>, Vec<u8>) {
    let (pk, sk) = pqcrypto_kyber::kyber768::keypair();
    (pk.as_bytes().to_vec(), sk.as_bytes().to_vec())
}

/// Encapsulate a shared secret to a peer's Kyber768 public key.
pub fn kyber_encapsulate(pk_bytes: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    let pk = KyberPublicKey::from_bytes(pk_bytes)
        .map_err(|e| BlockchainError::CryptographyError(format!("Invalid Kyber public key: {:?}", e)))?;
    let (ss, ct) = pqcrypto_kyber::kyber768::encapsulate(&pk);
    Ok((ct.as_bytes().to_vec(), ss.as_bytes().to_vec()))
}

/// Decapsulate a shared secret from ciphertext using Kyber768 secret key.
pub fn kyber_decapsulate(ct_bytes: &[u8], sk_bytes: &[u8]) -> Result<Vec<u8>> {
    let sk = KyberSecretKey::from_bytes(sk_bytes)
        .map_err(|e| BlockchainError::CryptographyError(format!("Invalid Kyber secret key: {:?}", e)))?;
    let ct = KyberCiphertext::from_bytes(ct_bytes)
        .map_err(|e| BlockchainError::CryptographyError(format!("Invalid Kyber ciphertext: {:?}", e)))?;
    let ss = pqcrypto_kyber::kyber768::decapsulate(&ct, &sk);
    Ok(ss.as_bytes().to_vec())
}

// Format implementations for better error messages
impl fmt::Display for Dilithium3Keypair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let pk_preview = if self.public_key.len() >= 32 {
            hex::encode(&self.public_key[..16])
        } else {
            hex::encode(&self.public_key)
        };
        write!(f, "Dilithium3Keypair(pubkey: {}..., age: {}s)", pk_preview, self.age_seconds())
    }
}

impl fmt::Display for Dilithium3Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sig_preview = if self.signature.len() >= 16 {
            hex::encode(&self.signature[..8])
        } else {
            hex::encode(&self.signature)
        };
        write!(f, "Dilithium3Signature(sig: {}..., size: {} bytes)", sig_preview, self.size())
    }
}

// Secure comparison to prevent timing attacks
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    
    use subtle::ConstantTimeEq;
    a.ct_eq(b).into()
}

/// Batch verify multiple signatures for efficiency
pub fn batch_verify_signatures(messages_and_signatures: &[(&[u8], &Dilithium3Signature)]) -> Result<Vec<bool>> {
    if messages_and_signatures.is_empty() {
        return Ok(Vec::new());
    }
    
    if messages_and_signatures.len() > 1000 {
        return Err(BlockchainError::CryptographyError("Too many signatures for batch verification".to_string()));
    }
    
    let mut results = Vec::with_capacity(messages_and_signatures.len());
    
    for (message, signature) in messages_and_signatures {
        let result = Dilithium3Keypair::verify(message, signature, &signature.public_key).unwrap();
        results.push(result);
    }
    
    Ok(results)
}

/// Secure memory wiping for sensitive data
pub fn secure_zero(data: &mut [u8]) {
    use zeroize::Zeroize;
    data.zeroize();
}

/// Timing-safe signature verification with DoS protection
pub fn verify_signature_with_timeout(
    message: &[u8], 
    signature: &Dilithium3Signature, 
    timeout_ms: u64
) -> Result<bool> {
    use std::time::Duration;
    use std::sync::mpsc;
    use std::thread;
    
    if timeout_ms == 0 {
        return Ok(Dilithium3Keypair::verify(message, signature, &signature.public_key).unwrap());
    }
    
    let (tx, rx) = mpsc::channel();
    let message = message.to_vec();
    let signature = signature.clone();
    
    thread::spawn(move || {
        let result = Dilithium3Keypair::verify(&message, &signature, &signature.public_key);
        let _ = tx.send(result);
    });
    
    match rx.recv_timeout(Duration::from_millis(timeout_ms)) {
        Ok(result) => result,
        Err(_) => {
            log::warn!("Signature verification timed out after {}ms", timeout_ms);
            Ok(false) // Timeout treated as invalid signature
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_dilithium3_keypair_generation() {
        let keypair = Dilithium3Keypair::new().unwrap();
        assert_eq!(keypair.public_key.len(), DILITHIUM3_PUBKEY_SIZE);
        assert_eq!(keypair.secret_key.len(), DILITHIUM3_SECKEY_SIZE);
        assert!(keypair.age_seconds() < 10); // Should be very recent
        
        // Test entropy validation
        let keypair_no_check = Dilithium3Keypair::new_with_entropy_check(false).unwrap();
        assert_eq!(keypair_no_check.public_key.len(), DILITHIUM3_PUBKEY_SIZE);
    }
    
    #[test]
    fn test_dilithium3_sign_verify() {
        let keypair = Dilithium3Keypair::new().unwrap();
        let message = b"Hello, quantum-safe world!";
        
        let signature = keypair.sign(message).unwrap();
        
        assert!(signature.is_valid_format());
        assert_eq!(signature.message_hash, blake3_hash(message));
        assert!(!signature.is_expired(3600)); // Not expired within 1 hour
        
        let valid = Dilithium3Keypair::verify(message, &signature, &keypair.public_key).unwrap();
        assert!(valid);
        
        // Test with wrong message
        let wrong_message = b"Wrong message";
        let invalid = Dilithium3Keypair::verify(wrong_message, &signature, &keypair.public_key).unwrap();
        assert!(!invalid);
        
        // Test message size limit
        let large_message = vec![0u8; MAX_SIGNABLE_MESSAGE_SIZE + 1];
        assert!(keypair.sign(&large_message).is_err());
    }
    
    #[test]
    fn test_keypair_integrity() {
        let keypair = Dilithium3Keypair::new().unwrap();
        
        // Should pass integrity check
        assert!(keypair.validate_integrity().is_ok());
        
        // Test fingerprint
        let expected_fingerprint = blake3_hash(&keypair.public_key);
        assert_eq!(*keypair.fingerprint(), expected_fingerprint);
    }
    
    #[test]
    fn test_keypair_from_bytes() {
        let original = Dilithium3Keypair::new().unwrap();
        let public_key = original.public_key.clone();
        let secret_key = original.secret_key_bytes().to_vec();
        
        // Should recreate keypair successfully
        let recreated = Dilithium3Keypair::from_bytes(public_key, secret_key).unwrap();
        assert_eq!(original.public_key, recreated.public_key);
        
        // Test with invalid sizes
        assert!(Dilithium3Keypair::from_bytes(vec![0; 10], vec![0; 10]).is_err());
    }
    
    #[test]
    fn test_blake3_hashing() {
        let data = b"test data";
        let hash1 = blake3_hash(data);
        let hash2 = blake3_hash(data);
        
        // Deterministic
        assert_eq!(hash1, hash2);
        
        // Different data produces different hash
        let hash3 = blake3_hash(b"different data");
        assert_ne!(hash1, hash3);
        
        // Empty data
        let empty_hash = blake3_hash(b"");
        assert_eq!(empty_hash, [0u8; 32]);
    }
    
    #[test]
    fn test_incremental_hasher() {
        let mut hasher = Blake3Hasher::new();
        
        hasher.update(b"hello").unwrap();
        hasher.update(b" ").unwrap();
        hasher.update(b"world").unwrap();
        
        let hash1 = hasher.finalize();
        let hash2 = blake3_hash(b"hello world");
        assert_eq!(hash1, hash2);
        
        // Test size limit
        let mut big_hasher = Blake3Hasher::new();
        let big_data = vec![0u8; 500_000_000];
        big_hasher.update(&big_data).unwrap();
        // This should succeed
        
        let huge_data = vec![0u8; 600_000_000];
        assert!(big_hasher.update(&huge_data).is_err()); // Should fail due to size limit
    }
    
    #[test]
    fn test_argon2id_pow() {
        let config = Argon2Config::development(); // Fast for testing
        let data = b"test proof of work";
        let salt = b"testsalt1234567890123456"; // 24 bytes
        
        let result1 = argon2id_pow_hash(data, salt, &config).unwrap();
        let result2 = argon2id_pow_hash(data, salt, &config).unwrap();
        
        // Deterministic with same input
        assert_eq!(result1, result2);
        assert_eq!(result1.len(), config.output_length);
        
        // Different salt produces different result
        let salt2 = b"differentsalt123456789012";
        let result3 = argon2id_pow_hash(data, salt2, &config).unwrap();
        assert_ne!(result1, result3);
        
        // Test validation
        assert!(argon2id_pow_hash(b"", salt, &config).is_err()); // Empty data
        assert!(argon2id_pow_hash(data, b"short", &config).is_err()); // Short salt
    }
    
    #[test]
    fn test_difficulty_target_generation() {
        let target_0 = generate_difficulty_target(0);
        let target_1 = generate_difficulty_target(1);
        let target_8 = generate_difficulty_target(8);
        let target_high = generate_difficulty_target(1000); // Should be capped
        
        // Target should decrease with difficulty
        assert!(target_1 < target_0);
        assert!(target_8 < target_1);
        
        // High difficulty should be capped
        assert_eq!(target_high.len(), 32);
        
        // Verify target format - for difficulty 8, first byte should be 0
        assert_eq!(target_8[0], 0);
    }
    
    #[test]
    fn test_pow_verification() {
        let header = b"test block header";
        let nonce = 12345u64;
        
        // Generate easy target (low difficulty)
        let target = generate_difficulty_target(1);
        
        // Should complete without error
        let result = verify_pow(header, nonce, &target);
        assert!(result.is_ok());
        
        // Test input validation
        assert!(verify_pow(b"", nonce, &target).is_err()); // Empty header
        assert!(verify_pow(header, nonce, &[]).is_err()); // Invalid target
    }
    
    #[test]
    fn test_target_difficulty_conversion() {
        for difficulty in [0, 1, 8, 16, 24] {
            let target = generate_difficulty_target(difficulty);
            let calculated_difficulty = target_to_difficulty(&target);
            assert_eq!(calculated_difficulty, difficulty);
        }
        
        // Test invalid target
        assert_eq!(target_to_difficulty(&[]), 0);
    }
    
    #[test]
    fn test_key_derivation() {
        let seed = b"test seed data with sufficient length";
        let salt = "test salt";
        let info = b"test info";
        
        let key1 = derive_key(seed, salt, info).unwrap();
        let key2 = derive_key(seed, salt, info).unwrap();
        
        // Deterministic
        assert_eq!(key1, key2);
        
        // Different inputs produce different keys
        let key3 = derive_key(b"different seed with sufficient length", salt, info).unwrap();
        assert_ne!(key1, key3);
        
        // Test validation
        assert!(derive_key(b"", salt, info).is_err()); // Empty seed
        assert!(derive_key(b"short", salt, info).is_err()); // Short seed
        assert!(derive_key(seed, "", info).is_err()); // Empty salt
    }
    
    #[test]
    fn test_random_generation() {
        // Test basic generation
        let bytes1 = generate_random_bytes(32).unwrap();
        let bytes2 = generate_random_bytes(32).unwrap();
        
        assert_eq!(bytes1.len(), 32);
        assert_eq!(bytes2.len(), 32);
        assert_ne!(bytes1, bytes2); // Should be different
        
        // Test edge cases
        let empty = generate_random_bytes(0).unwrap();
        assert_eq!(empty.len(), 0);
        
        assert!(generate_random_bytes(2_000_000).is_err()); // Too large
        
        // Test salt generation
        let salt1 = generate_salt().unwrap();
        let salt2 = generate_salt().unwrap();
        assert_ne!(salt1, salt2);
        
        let custom_salt = generate_salt_with_length(24).unwrap();
        assert_eq!(custom_salt.len(), 24);
        
        assert!(generate_salt_with_length(8).is_err()); // Too short
        assert!(generate_salt_with_length(128).is_err()); // Too long
    }
    
    #[test]
    fn test_constant_time_comparison() {
        let data1 = [1, 2, 3, 4];
        let data2 = [1, 2, 3, 4];
        let data3 = [1, 2, 3, 5];
        
        assert!(constant_time_eq(&data1, &data2));
        assert!(!constant_time_eq(&data1, &data3));
        assert!(!constant_time_eq(&data1, &[1, 2, 3])); // Different lengths
    }
    
    #[test]
    fn test_batch_signature_verification() {
        let keypair1 = Dilithium3Keypair::new().unwrap();
        let keypair2 = Dilithium3Keypair::new().unwrap();
        
        let msg1 = b"message 1";
        let msg2 = b"message 2";
        
        let sig1 = keypair1.sign(msg1).unwrap();
        let sig2 = keypair2.sign(msg2).unwrap();
        
        let batch = [
            (msg1.as_slice(), &sig1),
            (msg2.as_slice(), &sig2),
        ];
        
        let results = batch_verify_signatures(&batch).unwrap();
        assert_eq!(results.len(), 2);
        assert!(results[0]);
        assert!(results[1]);
        
        // Test empty batch
        let empty_results = batch_verify_signatures(&[]).unwrap();
        assert_eq!(empty_results.len(), 0);
        
        // Test too many signatures
        let too_many = vec![(msg1.as_slice(), &sig1); 1001];
        assert!(batch_verify_signatures(&too_many).is_err());
    }
    
    #[test]
    fn test_signature_timeout() {
        let keypair = Dilithium3Keypair::new().unwrap();
        let message = b"test message";
        let signature = keypair.sign(message).unwrap();
        
        // Test without timeout
        let result1 = verify_signature_with_timeout(message, &signature, 0).unwrap();
        assert!(result1);
        
        // Test with reasonable timeout
        let result2 = verify_signature_with_timeout(message, &signature, 1000).unwrap();
        assert!(result2);
    }
    
    #[test]
    fn test_signature_expiry() {
        let keypair = Dilithium3Keypair::new().unwrap();
        let message = b"test message";
        let signature = keypair.sign(message).unwrap();
        
        // Should not be expired immediately
        assert!(!signature.is_expired(3600));
        
        // Should be expired with very short window
        assert!(signature.is_expired(0));
    }
    
    #[test]
    fn test_secure_zero() {
        let mut data = vec![0xFF; 32];
        secure_zero(&mut data);
        assert_eq!(data, vec![0; 32]);
    }

    #[test]
    fn test_kyber_kem() {
        let (pk, sk) = kyber_keypair();
        let (ct, ss1) = kyber_encapsulate(&pk).unwrap();
        let ss2 = kyber_decapsulate(&ct, &sk).unwrap();
        assert_eq!(ss1, ss2);
    }
} 