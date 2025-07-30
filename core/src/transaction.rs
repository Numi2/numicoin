// src/transaction.rs
//
// Numicoin transaction model (PoW chain, no smart-contracts yet).
//

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    crypto::{blake3_hash, Dilithium3Keypair, Dilithium3Signature},
    error::BlockchainError,
    Result,
};

pub type TransactionId = [u8; 32];

/* ---------------------------------------------------------------------
   Fee & size limits
---------------------------------------------------------------------*/
pub const MAX_TX_BYTES: usize = 64 * 1024; // 64 KB hard cap
pub const BASE_FEE:     u64   = 1;         // 1 nano-NUMI
pub const FEE_PER_KIB:  u64   = 1;         // 1 nano per KiB (rounded up)
pub const MAX_FEE:      u64   = 100;       // guard-rail

/* ---------------------------------------------------------------------
   Types
---------------------------------------------------------------------*/
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransactionType {
    Transfer { to: Vec<u8>, amount: u64, memo: Option<String> },
    MiningReward { block_height: u64, amount: u64 },
}

impl TransactionType {
    fn is_reward(&self) -> bool {
        matches!(self, TransactionType::MiningReward { .. })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub id:            TransactionId,
    pub from:          Vec<u8>,
    pub kind:          TransactionType,
    pub nonce:         u64,
    pub fee:           u64,
    pub timestamp:     DateTime<Utc>,
    pub valid_until:   DateTime<Utc>,
    pub signature:     Option<Dilithium3Signature>,
}

#[derive(Serialize)]
struct SigningView<'a> {
    from:        &'a [u8],
    kind:        &'a TransactionType,
    nonce:       u64,
    fee:         u64,
    timestamp:   DateTime<Utc>,
    valid_until: DateTime<Utc>,
}

/* ---------------------------------------------------------------------
   Constructors
---------------------------------------------------------------------*/
impl Transaction {
    pub fn new(from: Vec<u8>, kind: TransactionType, nonce: u64) -> Self {
        let now = Utc::now();
        let mut tx = Self {
            id: [0; 32],
            from,
            kind,
            nonce,
            fee: BASE_FEE,
            timestamp: now,
            valid_until: now + chrono::Duration::hours(1),
            signature: None,
        };
        
        // Set appropriate fee based on transaction type
        if tx.kind.is_reward() {
            tx.fee = 0; // Mining rewards must have 0 fee
        } else {
            tx.fee = tx.min_fee().total;
        }
        
        tx.id = tx.hash();
        tx
    }

    pub fn sign(&mut self, kp: &Dilithium3Keypair) -> Result<()> {
        self.validate_structure()?;
        let msg = self.signing_bytes()?;
        self.signature = Some(kp.sign(&msg)?);
        self.id = self.hash();
        Ok(())
    }

    pub fn verify_signature(&self) -> Result<bool> {
        if let Some(sig) = &self.signature {
            crate::crypto::Dilithium3Keypair::verify(&self.signing_bytes()?, sig, &self.from)
        } else {
            Ok(false)
        }
    }

    /* ---------------- fee helpers ---------------- */
    fn min_fee(&self) -> FeeInfo {
        let size = self.signing_bytes().map(|b| b.len()).unwrap_or(0);
        FeeInfo::for_size(size)
    }

    /* ---------------- hash & serialization -------- */
    fn signing_bytes(&self) -> Result<Vec<u8>> {
        let view = SigningView {
            from: &self.from,
            kind: &self.kind,
            nonce: self.nonce,
            fee: self.fee,
            timestamp: self.timestamp,
            valid_until: self.valid_until,
        };
        bincode::serialize(&view)
            .map_err(|e| BlockchainError::SerializationError(e.to_string()))
    }

    pub fn hash(&self) -> TransactionId {
        let bytes = self.signing_bytes().unwrap_or_default();
        blake3_hash(&bytes)
    }
}

/* ---------------------------------------------------------------------
   Validation & helpers
---------------------------------------------------------------------*/
impl Transaction {
    pub fn validate_structure(&self) -> Result<()> {
        // pubkey sanity
        if self.from.is_empty() || self.from.len() > 10_000 {
            return Err(BlockchainError::InvalidTransaction("bad sender pk".into()));
        }

        // size limit
        let sz = self.signing_bytes()?.len();
        if sz > MAX_TX_BYTES {
            return Err(BlockchainError::InvalidTransaction("tx too large".into()));
        }

        // fee sanity (except for rewards)
        if !self.kind.is_reward() {
            let min = self.min_fee();
            if self.fee < min.total {
                return Err(BlockchainError::InvalidTransaction("fee too low".into()));
            }
            if self.fee > MAX_FEE {
                return Err(BlockchainError::InvalidTransaction("fee too high".into()));
            }
        } else if self.fee != 0 {
            return Err(BlockchainError::InvalidTransaction("reward fee must be 0".into()));
        }

        // timestamp / expiry
        let now = Utc::now();
        if self.timestamp > now + chrono::Duration::minutes(5) {
            return Err(BlockchainError::InvalidTransaction("future ts".into()));
        }
        if now > self.valid_until {
            return Err(BlockchainError::InvalidTransaction("expired".into()));
        }

        // kind-specific checks
        match &self.kind {
            TransactionType::Transfer { to, amount, memo } => {
                if to.is_empty() { return Err(BlockchainError::InvalidTransaction("empty recipient".into())); }
                if *amount == 0 { return Err(BlockchainError::InvalidTransaction("zero amount".into())); }
                if let Some(m) = memo {
                    if m.len() > 256 || !m.is_ascii() {
                        return Err(BlockchainError::InvalidTransaction("bad memo".into()));
                    }
                }
            }
            TransactionType::MiningReward { amount, .. } => {
                if *amount == 0 {
                    return Err(BlockchainError::InvalidTransaction("zero reward".into()));
                }
            }
        }
        Ok(())
    }

    pub fn amount(&self) -> u64 {
        match &self.kind {
            TransactionType::Transfer { amount, .. } => *amount,
            TransactionType::MiningReward { amount, .. } => *amount,
        }
    }

    pub fn required_balance(&self) -> u64 {
        self.amount().saturating_add(self.fee)
    }

    pub fn priority(&self) -> u64 {
        if self.kind.is_reward() { u64::MAX } else { self.fee }
    }
}

/* ---------------------------------------------------------------------
   FeeInfo helper
---------------------------------------------------------------------*/
#[derive(Debug)]
struct FeeInfo {
    total: u64,
}
impl FeeInfo {
    fn for_size(bytes: usize) -> Self {
        let size_fee = ((bytes as u64 + 1023) / 1024) * FEE_PER_KIB;   // ceil KiB
        let total = BASE_FEE.saturating_add(size_fee).clamp(BASE_FEE, MAX_FEE);
        Self { total }
    }
}

/* ---------------------------------------------------------------------
   Tests
---------------------------------------------------------------------*/
#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::Dilithium3Keypair;

    #[test]
    fn fee_calc() {
        let f = FeeInfo::for_size(1500);
        assert_eq!(f.total, BASE_FEE + 2); // 1.5 KiB ⇒ 2 KiB (integer division), so total = 1 + 2 = 3
    }

    #[test]
    fn sign_and_verify() {
        let kp = Dilithium3Keypair::new().unwrap();
        let mut tx = Transaction::new(kp.public_key.clone(), TransactionType::Transfer { to: vec![1], amount: 10, memo: None }, 1);
        tx.sign(&kp).unwrap();
        assert!(tx.verify_signature().unwrap());
    }
}