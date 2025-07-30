// src/crypto.rs

use std::fmt;
use std::fs::{OpenOptions, metadata, read_to_string};
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::io::Write;

use argon2::{Argon2, Params, Algorithm, Version};
use base64ct::Base64;
use base64ct::Encoding;
use blake3::Hasher;
use chrono::Utc;
use pqcrypto_traits::sign::DetachedSignature as PqDetachedSignature;
use pqcrypto_traits::sign::{PublicKey, SecretKey};
use pqcrypto_dilithium::dilithium3::{
    keypair as dilithium3_keypair, PublicKey as PqPublicKey3, SecretKey as PqSecretKey3,
    detached_sign as dilithium3_sign, verify_detached_signature as dilithium3_verify,
};
use pqcrypto_traits::kem::{Ciphertext as KemCiphertext, PublicKey as KemPublicKey, SecretKey as KemSecretKey, SharedSecret as KemSharedSecret};
use pqcrypto_kyber::kyber768;
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;
use zeroize::ZeroizeOnDrop;

use crate::error::BlockchainError;
use crate::Result;
use crate::config::ConsensusConfig;

/// 256-bit hash
pub type Hash = [u8; 32];

const MAX_RANDOM_BYTES: usize = 1_000_000;
pub const MAX_SIGNABLE_MESSAGE_SIZE: usize = 4 * 1024 * 1024; // Reduced to 4MB

/// Dilithium3 sizes
pub const DILITHIUM3_SIGNATURE_SIZE: usize = pqcrypto_dilithium::dilithium3::signature_bytes();
pub const DILITHIUM3_PUBKEY_SIZE: usize    = pqcrypto_dilithium::dilithium3::public_key_bytes();
pub const DILITHIUM3_SECKEY_SIZE: usize    = pqcrypto_dilithium::dilithium3::secret_key_bytes();

/// PEM-export/import record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PemKeyPair {
    pub private_key: String,
    pub public_key:  String,
}

/// Post-quantum Dilithium3 keypair
#[derive(Debug, Clone, Serialize, Deserialize, ZeroizeOnDrop)]
pub struct Dilithium3Keypair {
    #[zeroize(skip)]
    pub public_key: Vec<u8>,
    #[zeroize]
    pub secret_key: Vec<u8>,
    #[zeroize(skip)]
    pub fingerprint: Hash,
    pub created_at:  u64,
}

impl Dilithium3Keypair {
    /// New keypair (with entropy check)
    pub fn new() -> Result<Self> {
        Self::with_entropy_check(true)
    }

    pub fn with_entropy_check(validate_entropy: bool) -> Result<Self> {
        if validate_entropy {
            Self::check_entropy()?;
        }
        let (pk, sk) = dilithium3_keypair();
        let public_key = pk.as_bytes().to_vec();
        let secret_key = sk.as_bytes().to_vec();
        if public_key.len() != DILITHIUM3_PUBKEY_SIZE {
            return Err(BlockchainError::CryptographyError(format!(
                "Expected public key size {}, got {}",
                DILITHIUM3_PUBKEY_SIZE,
                public_key.len()
            )));
        }
        if secret_key.len() != DILITHIUM3_SECKEY_SIZE {
            return Err(BlockchainError::CryptographyError(format!(
                "Expected secret key size {}, got {}",
                DILITHIUM3_SECKEY_SIZE,
                secret_key.len()
            )));
        }
        let fingerprint = blake3_hash(&public_key);
        let created_at  = Utc::now().timestamp() as u64;
        Ok(Self { public_key, secret_key, fingerprint, created_at })
    }

    /// Load from JSON or PEM
    pub fn load_from_file<P: AsRef<std::path::Path>>(path: P) -> Result<Self> {
        // Enforce strict file permissions (owner read-only)
        let perms = metadata(path.as_ref()).map_err(|e| BlockchainError::CryptographyError(e.to_string()))?.permissions();
        #[cfg(unix)]
        if perms.mode() & 0o177 != 0 {
            return Err(BlockchainError::CryptographyError("Insecure key file permissions: group/other access detected. Use chmod 600.".into()));
        }
        #[cfg(not(unix))]
        {
            // Basic check for non-Unix systems (less precise but better than nothing)
            if perms.readonly() == false {
                 log::warn!("Key file is not read-only. For security, restrict write access to this file.");
            }
        }

        let content = read_to_string(path.as_ref()).map_err(|e| BlockchainError::CryptographyError(e.to_string()))?;
        if let Ok(kp) = serde_json::from_str::<Self>(&content) {
            kp.validate_integrity()?;
            return Ok(kp);
        }
        Self::from_pem(&content)
    }

    /// Save as JSON (0600 on Unix)
    pub fn save_to_file<P: AsRef<std::path::Path>>(&self, path: P) -> Result<()> {
        let json = serde_json::to_string_pretty(self).map_err(|e| BlockchainError::CryptographyError(e.to_string()))?;
        #[cfg(unix)]
        {
            let mut file = OpenOptions::new()
                .write(true).create(true).truncate(true)
                .mode(0o600)
                .open(path.as_ref())
                .map_err(|e| BlockchainError::CryptographyError(e.to_string()))?;
            file.write_all(json.as_bytes()).map_err(|e| BlockchainError::CryptographyError(e.to_string()))?;
        }
        #[cfg(not(unix))]
        {
            write(path.as_ref(), json).map_err(|e| BlockchainError::CryptographyError(e.to_string()))?;
        }
        Ok(())
    }

    /// Export to PEM
    pub fn to_pem(&self) -> PemKeyPair {
        PemKeyPair {
            private_key: Base64::encode_string(&self.secret_key),
            public_key:  Base64::encode_string(&self.public_key),
        }
    }

    /// Import from PEM
    pub fn from_pem(pem: &str) -> Result<Self> {
        let priv_b64 = extract_pem_block(pem, "PRIVATE KEY")?;
        let pub_b64  = extract_pem_block(pem, "PUBLIC KEY")?;
        let mut sk = vec![0u8; DILITHIUM3_SECKEY_SIZE];
        Base64::decode(priv_b64.as_bytes(), &mut sk).map_err(|e| BlockchainError::CryptographyError(e.to_string()))?;
        let mut pk = vec![0u8; DILITHIUM3_PUBKEY_SIZE];
        Base64::decode(pub_b64.as_bytes(), &mut pk).map_err(|e| BlockchainError::CryptographyError(e.to_string()))?;
        Self::from_bytes(pk, sk)
    }

    /// From raw bytes (round-trip validation)
    pub fn from_bytes(public_key: Vec<u8>, secret_key: Vec<u8>) -> Result<Self> {
        if public_key.len() != DILITHIUM3_PUBKEY_SIZE || secret_key.len() != DILITHIUM3_SECKEY_SIZE {
            return Err(BlockchainError::CryptographyError("Invalid key sizes".into()));
        }
        let pk = PqPublicKey3::from_bytes(&public_key).map_err(|_| BlockchainError::CryptographyError("Bad public key".into()))?;
        let sk = PqSecretKey3::from_bytes(&secret_key).map_err(|_| BlockchainError::CryptographyError("Bad secret key".into()))?;
        let test_sig = dilithium3_sign(b"__check__", &sk);
        if dilithium3_verify(&test_sig, b"__check__", &pk).is_err() {
            return Err(BlockchainError::CryptographyError("Keypair mismatch".into()));
        }
        let fingerprint = blake3_hash(&public_key);
        let created_at  = Utc::now().timestamp() as u64;
        Ok(Self { public_key, secret_key, fingerprint, created_at })
    }

    /// Sign a message
    pub fn sign(&self, msg: &[u8]) -> Result<Dilithium3Signature> {
        if msg.len() > MAX_SIGNABLE_MESSAGE_SIZE {
            return Err(BlockchainError::InvalidArgument("Message too large".into()));
        }
        if self.secret_key.iter().all(|&b| b == 0) {
            return Err(BlockchainError::CryptographyError("No secret key available".into()));
        }
        let sk = PqSecretKey3::from_bytes(&self.secret_key).map_err(|_| BlockchainError::CryptographyError("Invalid secret key".into()))?;
        let sig = dilithium3_sign(msg, &sk);
        Ok(Dilithium3Signature {
            signature:    sig.as_bytes().to_vec(),
            public_key:   self.public_key.clone(),
            message_hash: blake3_hash(msg),
            created_at:   Utc::now().timestamp() as u64,
        })
    }

    /// Verify a signature against an explicit public key
    pub fn verify(msg: &[u8], sig: &Dilithium3Signature, public_key: &[u8]) -> Result<bool> {
        if msg.len() > MAX_SIGNABLE_MESSAGE_SIZE {
            return Err(BlockchainError::InvalidArgument("Message too large to verify".into()));
        }

        let msg_hash = blake3_hash(msg);
        if !constant_time_eq(&msg_hash, &sig.message_hash) {
            return Ok(false);
        }

        let pk = match PqPublicKey3::from_bytes(public_key) {
            Ok(pk) => pk,
            Err(_) => return Ok(false), // Invalid public key format
        };

        let ds = match PqDetachedSignature::from_bytes(&sig.signature) {
            Ok(ds) => ds,
            Err(_) => return Ok(false), // Invalid signature format
        };

        Ok(dilithium3_verify(&ds, msg, &pk).is_ok())
    }

    /// Fingerprint integrity
    pub fn validate_integrity(&self) -> Result<()> {
        let fp = blake3_hash(&self.public_key);
        if !constant_time_eq(&fp, &self.fingerprint) {
            return Err(BlockchainError::CryptographyError("Fingerprint mismatch".into()));
        }
        Ok(())
    }

    fn check_entropy() -> Result<()> {
        let mut buf = [0u8; 32];
        OsRng.fill_bytes(&mut buf);
        let ones: u32 = buf.iter().map(|b| b.count_ones()).sum();
        if !(96..=160).contains(&ones) {
            return Err(BlockchainError::CryptographyError("Insufficient entropy".into()));
        }
        Ok(())
    }
}

/// Return the raw public key bytes
impl Dilithium3Keypair {
    pub fn public_key_bytes(&self) -> &[u8] {
        &self.public_key
    }
}
/// Return the raw secret key bytes
impl Dilithium3Keypair {
    pub fn secret_key_bytes(&self) -> &[u8] {
        &self.secret_key
    }
}

/// PQ signature wrapper
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Dilithium3Signature {
    pub signature:    Vec<u8>,
    pub public_key:   Vec<u8>,
    pub message_hash: Hash,
    pub created_at:   u64,
}

impl Dilithium3Signature {
    pub fn is_valid_format(&self) -> bool {
        self.signature.len() == DILITHIUM3_SIGNATURE_SIZE
            && self.public_key.len() == DILITHIUM3_PUBKEY_SIZE
            && self.created_at > 0
    }
    pub fn is_expired(&self, max_age: u64) -> bool {
        Utc::now().timestamp() as u64 - self.created_at >= max_age
    }
    pub fn size(&self) -> usize {
        self.signature.len() + self.public_key.len() + 32 + 8
    }
}

/// Basic BLAKE3 hash (empty → zero)
pub fn blake3_hash(data: &[u8]) -> Hash {
    if data.is_empty() {
        return [0; 32];
    }
    let mut h = Hasher::new();
    h.update(data);
    let mut out = [0; 32];
    out.copy_from_slice(&h.finalize().as_bytes()[..32]);
    out
}

/// Hex‐encode
pub fn blake3_hash_hex(data: &[u8]) -> String {
    hex::encode(blake3_hash(data))
}

/// Domain‐separated block hash
pub fn blake3_hash_block(data: &[u8]) -> Hash {
    if data.is_empty() {
        return [0; 32];
    }
    let mut h = Hasher::new_derive_key("numi-block");
    h.update(data);
    let mut out = [0; 32];
    out.copy_from_slice(&h.finalize().as_bytes()[..32]);
    out
}

/// Domain‐separated transaction hash
pub fn blake3_hash_tx(data: &[u8]) -> Hash {
    if data.is_empty() {
        return [0; 32];
    }
    let mut h = Hasher::new_derive_key("transaction_id");
    h.update(data);
    let mut out = [0; 32];
    out.copy_from_slice(&h.finalize().as_bytes()[..32]);
    out
}

/// Derive key with BLAKE3‐KDF
pub fn derive_key(seed: &[u8], salt: &str, info: &[u8]) -> Result<Hash> {
    if seed.len() < 16 || salt.is_empty() {
        return Err(BlockchainError::CryptographyError("Invalid derive inputs".into()));
    }
    let mut h = Hasher::new_derive_key(salt);
    h.update(seed);
    h.update(info);
    let mut out = [0; 32];
    out.copy_from_slice(&h.finalize().as_bytes()[..32]);
    Ok(out)
}

/// Secure random
pub fn generate_random_bytes(len: usize) -> Result<Vec<u8>> {
    if len > MAX_RANDOM_BYTES {
        return Err(BlockchainError::CryptographyError("Too many random bytes".into()));
    }
    let mut buf = vec![0; len];
    OsRng.fill_bytes(&mut buf);
    Ok(buf)
}

/// 256-bit salt
pub fn generate_salt() -> Result<[u8; 32]> {
    let buf = generate_random_bytes(32)?;
    let mut s = [0; 32];
    s.copy_from_slice(&buf);
    Ok(s)
}

fn extract_pem_block(pem: &str, tag: &str) -> Result<String> {
    let begin = format!("-----BEGIN {}-----", tag);
    let end = format!("-----END {}-----", tag);
    let start = pem.find(&begin)
        .ok_or_else(|| BlockchainError::CryptographyError(format!("Missing {}", begin)))?;
    let finish = pem.find(&end)
        .ok_or_else(|| BlockchainError::CryptographyError(format!("Missing {}", end)))?;
    Ok(pem[start + begin.len()..finish]
        .lines()
        .filter(|l| !l.trim().is_empty())
        .collect())
}

/// Constant-time eq
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.ct_eq(b).into()
}

/// Argon2id PoW parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Argon2Config {
    pub memory_cost:   u32,
    pub time_cost:     u32,
    pub parallelism:   u32,
    pub output_length: usize,
    pub salt_length:   usize,
}

impl Default for Argon2Config {
    fn default() -> Self {
        Self {
            memory_cost:   64 * 1024, // 64 MiB
            time_cost:     3,
            parallelism:   1,
            output_length: 32,
            salt_length:   16,
        }
    }
}

impl Argon2Config {
    pub fn production() -> Self {
        Self {
            memory_cost: 262_144,   // 256 MiB
            time_cost:   5,
            ..Self::default()
        }
    }
    pub fn development() -> Self {
        Self {
            memory_cost: 4_096,     // 4 MiB
            time_cost:   1,
            ..Self::default()
        }
    }
    pub fn validate(&self) -> Result<()> {
        if self.memory_cost < 8 || self.memory_cost > (1 << 24) {
            return Err(BlockchainError::CryptographyError("Invalid memory_cost".into()));
        }
        if self.time_cost == 0 || self.time_cost > 100 {
            return Err(BlockchainError::CryptographyError("Invalid time_cost".into()));
        }
        if self.parallelism == 0 || self.parallelism > 16 {
            return Err(BlockchainError::CryptographyError("Invalid parallelism".into()));
        }
        if self.output_length < 16 || self.output_length > 64 {
            return Err(BlockchainError::CryptographyError("Invalid output_length".into()));
        }
        if self.salt_length < 8 || self.salt_length > 64 {
            return Err(BlockchainError::CryptographyError("Invalid salt_length".into()));
        }
        Ok(())
    }
}

/// Argon2d‐based PoW hash
/// Note: Argon2d is used for its ASIC resistance in PoW.
pub fn argon2d_pow(data: &[u8], salt: &[u8], cfg: &Argon2Config) -> Result<Vec<u8>> {
    if data.is_empty() || salt.len() < cfg.salt_length {
        return Err(BlockchainError::CryptographyError("Invalid PoW inputs".into()));
    }
    cfg.validate()?;
    let salt_bytes = &salt[..cfg.salt_length];
    let params = Params::new(cfg.memory_cost, cfg.time_cost, cfg.parallelism, Some(cfg.output_length))
        .map_err(|e| BlockchainError::CryptographyError(e.to_string()))?;
    let argon2 = Argon2::new(Algorithm::Argon2d, Version::V0x13, params);
    let mut out = vec![0; cfg.output_length];
    argon2.hash_password_into(data, salt_bytes, &mut out)
        .map_err(|e| BlockchainError::CryptographyError(e.to_string()))?;
    Ok(out)
}

/// Verify PoW (adaptive default config)
pub fn verify_pow(header: &[u8], _nonce: u64, target: &[u8], consensus: &ConsensusConfig) -> Result<bool> {
    if header.is_empty() || target.len() != 32 {
        return Err(BlockchainError::CryptographyError("Invalid PoW args".into()));
    }

    // salt = first 16 bytes of blake3(header)
    let salt = &blake3_hash(header)[..16];

    let pow = argon2d_pow(header, salt, &consensus.argon2_config)?;
    let h   = blake3_hash_block(&pow);
    let mut tgt = [0u8; 32];
    tgt.copy_from_slice(target);
    Ok(crate::blockchain::meets_target(&h, &tgt))
}

/// Build a 256-bit target from difficulty bits
pub fn generate_difficulty_target(mut diff: u32) -> [u8; 32] {
    diff = diff.min(255);
    let mut t = [0xFFu8; 32];
    let zb    = (diff / 8) as usize;
    let zt    = (diff % 8) as usize;
    for b in t.iter_mut().take(zb) {
        *b = 0;
    }
    if zb < 32 && zt > 0 {
        t[zb] = 0xFF >> zt;
    }
    t
}

/// Recover difficulty from target
pub fn target_to_difficulty(target: &[u8]) -> u32 {
    if target.len() != 32 {
        return 0;
    }
    let mut d = 0u32;
    for &b in target {
        if b == 0 {
            d += 8;
        } else {
            d += b.leading_zeros();
            break;
        }
        if d >= 248 {
            break;
        }
    }
    d
}

/// PQ Kyber KEM
#[derive(Clone)]
pub struct KyberKeypair {
    pub public: Vec<u8>,
    secret:     Vec<u8>,
}

impl KyberKeypair {
    pub fn new() -> Result<Self> {
        let (pk, sk) = kyber768::keypair();
        Ok(Self {
            public: pk.as_bytes().to_vec(),
            secret: sk.as_bytes().to_vec(),
        })
    }
    pub fn encapsulate(peer: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
        let pk = kyber768::PublicKey::from_bytes(peer)
            .map_err(|_| BlockchainError::CryptographyError("Invalid Kyber public key".into()))?;
        let (ct, ss) = kyber768::encapsulate(&pk);
        Ok((ct.as_bytes().to_vec(), ss.as_bytes().to_vec()))
    }
    pub fn decapsulate(&self, ct_bytes: &[u8]) -> Result<Vec<u8>> {
        let sk = kyber768::SecretKey::from_bytes(&self.secret)
            .map_err(|_| BlockchainError::CryptographyError("Invalid Kyber secret key".into()))?;
        let ct = kyber768::Ciphertext::from_bytes(ct_bytes)
            .map_err(|_| BlockchainError::CryptographyError("Invalid Kyber ciphertext".into()))?;
        let ss = kyber768::decapsulate(&ct, &sk);
        Ok(ss.as_bytes().to_vec())
    }
}

impl fmt::Display for Dilithium3Keypair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let preview = hex::encode(&self.public_key[..16.min(self.public_key.len())]);
        write!(f, "Dilithium3Keypair(pub: {}..., age: {}s)", preview, self.created_at)
    }
}
 
impl fmt::Display for Dilithium3Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let preview = hex::encode(&self.signature[..8.min(self.signature.len())]);
        write!(f, "Dilithium3Signature(sig: {}..., size: {} bytes)", preview, self.size())
    }
}

/// Derive address from public key using BLAKE3 + RIPEMD160 + Base58Check
pub fn derive_address_from_public_key(pk: &[u8]) -> Result<String> {
    use ripemd::{Ripemd160, Digest};
    
    let h1 = blake3_hash(pk);
    let mut h2 = Ripemd160::new();
    h2.update(h1);
    let h3 = h2.finalize();

    let mut payload = vec![0u8; 21];
    payload[0] = 0x00;
    payload[1..].copy_from_slice(&h3);

    let checksum = &blake3_hash(&blake3_hash(&payload))[..4];
    let mut full = vec![0u8; 25];
    full[..21].copy_from_slice(&payload);
    full[21..].copy_from_slice(checksum);
    
    Ok(bs58::encode(full).into_string())
}
