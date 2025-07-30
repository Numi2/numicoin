use std::path::Path;
use sled;
use serde::{Serialize, Deserialize};

use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm, Nonce
};
use rand::RngCore;
use blake3;
use bincode;

use crate::block::Block;
use crate::transaction::Transaction;
use crate::blockchain::{ChainState, AccountState, SecurityCheckpoint};
use crate::error::BlockchainError;
use crate::Result;

/// Optional encryption key for sensitive data (AES-256)
#[derive(Debug, Clone)]
pub struct EncryptionKey {
    key: [u8; 32],
}

impl EncryptionKey {
    /// Derive key from password + salt via Argon2
    pub fn from_password(password: &str, salt: &[u8; 32]) -> Self {
        use argon2::{Argon2, PasswordHasher};
        let salt_str = argon2::password_hash::SaltString::encode_b64(salt).unwrap();
        let argon2 = Argon2::default();
        let hash = argon2.hash_password(password.as_bytes(), &salt_str).unwrap().hash.unwrap();
        let mut key = [0u8; 32];
        key.copy_from_slice(&hash.as_bytes()[..32]);
        EncryptionKey { key }
    }

    /// Generate a random key
    pub fn random() -> Self {
        let mut key = [0u8; 32];
        OsRng.fill_bytes(&mut key);
        EncryptionKey { key }
    }

    /// Encrypt, returning IV || ciphertext
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>> {
        let cipher = Aes256Gcm::new_from_slice(&self.key)
            .map_err(|e| BlockchainError::StorageError(format!("Invalid key: {e}")))?;
        // 96-bit random nonce
        let mut iv = [0u8; 12];
        OsRng.fill_bytes(&mut iv);
        let nonce = Nonce::from_slice(&iv);
        let mut ct = cipher.encrypt(nonce, plaintext)
            .map_err(|e| BlockchainError::StorageError(format!("Encrypt failed: {e}")))?;
        // Prepend IV
        let mut out = iv.to_vec();
        out.append(&mut ct);
        Ok(out)
    }

    /// Decrypt IV || ciphertext
    pub fn decrypt(&self, data: &[u8]) -> Result<Vec<u8>> {
        if data.len() < 12 {
            return Err(BlockchainError::StorageError("Ciphertext too short".into()));
        }
        let (iv, ct) = data.split_at(12);
        let cipher = Aes256Gcm::new_from_slice(&self.key)
            .map_err(|e| BlockchainError::StorageError(format!("Invalid key: {e}")))?;
        let nonce = Nonce::from_slice(iv);
        cipher.decrypt(nonce, ct)
            .map_err(|e| BlockchainError::StorageError(format!("Decrypt failed: {e}")))
    }
}

/// Database versioning
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DbVersion { pub major: u8, pub minor: u8 }
impl DbVersion {
    pub const CURRENT: Self = DbVersion { major: 1, minor: 0 };
    pub fn is_compatible(&self) -> bool {
        self.major == Self::CURRENT.major
    }
}

/// Backup metadata
#[derive(Debug, Serialize, Deserialize)]
pub struct BackupMetadata {
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub db_version: DbVersion,
    pub source_path: String,
    pub backup_size: u64,
}

/// Frame = [ version(2) | flag(1) | length(8) | payload[..] | checksum(32) ]
fn serialize_with_encryption<T: Serialize>(data: &T, key: Option<&EncryptionKey>) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    // version
    buf.extend_from_slice(&DbVersion::CURRENT.major.to_be_bytes());
    buf.extend_from_slice(&DbVersion::CURRENT.minor.to_be_bytes());
    // raw bincode
    let raw = bincode::serialize(data)
        .map_err(|e| BlockchainError::SerializationError(format!("{e}")))?;
    let (flag, payload) = if let Some(k) = key {
        let ct = k.encrypt(&raw)?;
        (1u8, ct)
    } else {
        (0u8, raw)
    };
    // flag + length
    buf.push(flag);
    buf.extend_from_slice(&(payload.len() as u64).to_be_bytes());
    // payload
    buf.extend_from_slice(&payload);
    // checksum of payload
    let chk = blake3::hash(&payload);
    buf.extend_from_slice(chk.as_bytes());
    Ok(buf)
}

fn deserialize_with_encryption<T: for<'de> Deserialize<'de>>(buf: &[u8], key: Option<&EncryptionKey>) -> Result<T> {
    if buf.len() < 2 +1 +8 +32 { return Err(BlockchainError::SerializationError("Too short".into())); }
    let major = buf[0]; let minor = buf[1];
    let version = DbVersion { major, minor };
    if !version.is_compatible() {
        return Err(BlockchainError::SerializationError(format!(
            "Incompatible {}.{} (need {}.{})",
            major, minor,
            DbVersion::CURRENT.major, DbVersion::CURRENT.minor
        )));
    }
    let flag = buf[2];
    let len = u64::from_be_bytes(buf[3..11].try_into().unwrap()) as usize;
    let payload_start = 11;
    let payload_end = payload_start + len;
    if buf.len() < payload_end + 32 {
        return Err(BlockchainError::SerializationError("Length mismatch".into()));
    }
    let payload = &buf[payload_start..payload_end];
    // verify checksum
    let expected = &buf[payload_end..payload_end+32];
    let actual = blake3::hash(payload);
    if &actual.as_bytes()[..] != expected {
        return Err(BlockchainError::SerializationError("Checksum failed".into()));
    }
    let data = if flag == 1 {
        let k = key.ok_or_else(|| BlockchainError::SerializationError("Encrypted but no key".into()))?;
        k.decrypt(payload)?
    } else {
        payload.to_vec()
    };
    bincode::deserialize(&data)
        .map_err(|e| BlockchainError::SerializationError(format!("{e}")))
}

pub struct BlockchainStorage {
    blocks: sled::Tree,
    transactions: sled::Tree,
    accounts: sled::Tree,
    state: sled::Tree,
    checkpoints: sled::Tree,
    metadata: sled::Tree, // For version and other metadata
    encryption_key: Option<EncryptionKey>, // Optional encryption for sensitive data
    base_path: std::path::PathBuf, // root directory of the database – used for auxiliary files
}

/// Transaction for atomic storage operations
pub struct StorageTransaction<'a> {
    storage: &'a BlockchainStorage,
    blocks_batch: sled::Batch,
    transactions_batch: sled::Batch,
    accounts_batch: sled::Batch,
    state_batch: sled::Batch,
    checkpoints_batch: sled::Batch,
}

impl<'a> StorageTransaction<'a> {
    pub fn new(storage: &'a BlockchainStorage) -> Self {
        Self {
            storage,
            blocks_batch: sled::Batch::default(),
            transactions_batch: sled::Batch::default(),
            accounts_batch: sled::Batch::default(),
            state_batch: sled::Batch::default(),
            checkpoints_batch: sled::Batch::default(),
        }
    }
    
    /// Add block to transaction batch
    pub fn save_block(&mut self, block: &Block) -> Result<()> {
        let key = self.storage.block_key(block.header.height);
        let value = serialize_with_encryption(block, self.storage.encryption_key.as_ref())?;
        self.blocks_batch.insert(key, value);
        Ok(())
    }
    
    /// Add transaction to batch
    pub fn save_transaction(&mut self, tx_id: &[u8; 32], transaction: &Transaction) -> Result<()> {
        let value = serialize_with_encryption(transaction, self.storage.encryption_key.as_ref())?;
        self.transactions_batch.insert(tx_id.as_slice(), value);
        Ok(())
    }
    
    /// Add account to batch
    pub fn save_account(&mut self, public_key: &[u8], account: &AccountState) -> Result<()> {
        let value = serialize_with_encryption(account, self.storage.encryption_key.as_ref())?;
        self.accounts_batch.insert(public_key, value);
        Ok(())
    }
    
    /// Add chain state to batch
    pub fn save_chain_state(&mut self, state: &ChainState) -> Result<()> {
        let value = serialize_with_encryption(state, self.storage.encryption_key.as_ref())?;
        self.state_batch.insert(b"current", value);
        Ok(())
    }
    
    /// Add checkpoint to batch
    pub fn save_checkpoint(&mut self, checkpoint: &SecurityCheckpoint) -> Result<()> {
        let key = self.storage.checkpoint_key(checkpoint.block_height);
        let value = serialize_with_encryption(checkpoint, self.storage.encryption_key.as_ref())?;
        self.checkpoints_batch.insert(key, value);
        Ok(())
    }
    
    /// Commit all changes atomically
    pub fn commit(self) -> Result<()> {
        // Apply batches to all trees
        self.storage.blocks.apply_batch(self.blocks_batch)
            .map_err(|e| BlockchainError::StorageError(format!("Failed to commit blocks: {e}")))?;
        self.storage.transactions.apply_batch(self.transactions_batch)
            .map_err(|e| BlockchainError::StorageError(format!("Failed to commit transactions: {e}")))?;
        self.storage.accounts.apply_batch(self.accounts_batch)
            .map_err(|e| BlockchainError::StorageError(format!("Failed to commit accounts: {e}")))?;
        self.storage.state.apply_batch(self.state_batch)
            .map_err(|e| BlockchainError::StorageError(format!("Failed to commit state: {e}")))?;
        self.storage.checkpoints.apply_batch(self.checkpoints_batch)
            .map_err(|e| BlockchainError::StorageError(format!("Failed to commit checkpoints: {e}")))?;
        Ok(())
    }
}

impl BlockchainStorage {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        Self::new_with_encryption(path, None)
    }
    
    pub fn new_with_encryption<P: AsRef<Path>>(path: P, encryption_key: Option<EncryptionKey>) -> Result<Self> {
        let db = sled::open(path.as_ref())
            .map_err(|e| BlockchainError::StorageError(format!("Failed to open database: {e}")))?;
        
        let blocks = db.open_tree("blocks")
            .map_err(|e| BlockchainError::StorageError(format!("Failed to open blocks tree: {e}")))?;
        
        let transactions = db.open_tree("transactions")
            .map_err(|e| BlockchainError::StorageError(format!("Failed to open transactions tree: {e}")))?;
        
        let accounts = db.open_tree("accounts")
            .map_err(|e| BlockchainError::StorageError(format!("Failed to open accounts tree: {e}")))?;
        
        let state = db.open_tree("chain_state")
            .map_err(|e| BlockchainError::StorageError(format!("Failed to open state tree: {e}")))?;
        
        let checkpoints = db.open_tree("checkpoints")
            .map_err(|e| BlockchainError::StorageError(format!("Failed to open checkpoints tree: {e}")))?;
        
        let metadata = db.open_tree("metadata")
            .map_err(|e| BlockchainError::StorageError(format!("Failed to open metadata tree: {e}")))?;
        
        let storage = Self {
            blocks,
            transactions,
            accounts,
            state,
            checkpoints,
            metadata,
            encryption_key,
            base_path: path.as_ref().to_path_buf(),
        };
        
        // Initialize database version if not exists
        storage.initialize_version()?;
        
        Ok(storage)
    }
    
    /// Initialize database version metadata
    fn initialize_version(&self) -> Result<()> {
        if self.metadata.get(b"version")
            .map_err(|e| BlockchainError::StorageError(format!("Failed to get version: {e}")))?
            .is_none() {
            let version_bytes = serialize_with_encryption(&DbVersion::CURRENT, self.encryption_key.as_ref())?;
            self.metadata.insert(b"version", version_bytes)
                .map_err(|e| BlockchainError::StorageError(format!("Failed to save version: {e}")))?;
        }
        Ok(())
    }
    
    /// Get database version
    pub fn get_version(&self) -> Result<DbVersion> {
        match self.metadata.get(b"version")
            .map_err(|e| BlockchainError::StorageError(format!("Failed to get version: {e}")))? {
            Some(data) => {
                let version: DbVersion = deserialize_with_encryption(&data, self.encryption_key.as_ref())?;
                Ok(version)
            }
            None => Ok(DbVersion::CURRENT), // Default for new databases
        }
    }
    
    /// Create a new storage transaction
    pub fn transaction(&self) -> StorageTransaction {
        StorageTransaction::new(self)
    }

    /// Path for simple block file persistence (core-data/blocks)
    pub fn blocks_dir(&self) -> std::path::PathBuf {
        self.base_path.join("blocks")
    }

    /* ------------------------------------------------------------------
       Internal key helpers – sled keys are little byte strings.  We store
       block height / checkpoint height as fixed-width big-endian so ordering
       matches natural height order.  Missing earlier caused E0599.
       ----------------------------------------------------------------*/

    fn block_key(&self, height: u64) -> Vec<u8> {
        height.to_be_bytes().to_vec()
    }

    fn checkpoint_key(&self, height: u64) -> Vec<u8> {
        // Re-use same encoding for checkpoints.
        height.to_be_bytes().to_vec()
    }
}