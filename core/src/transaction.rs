use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use crate::crypto::{Dilithium3Keypair, Dilithium3Signature, blake3_hash, blake3_hash_hex};
use crate::error::BlockchainError;
use crate::Result;

pub type TransactionId = [u8; 32];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransactionType {
    Transfer {
        to: Vec<u8>, // Public key of recipient
        amount: u64,
    },
    Stake {
        amount: u64,
    },
    Unstake {
        amount: u64,
    },
    MiningReward {
        block_height: u64,
        amount: u64,
    },
    Governance {
        proposal_id: u64,
        vote: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub id: TransactionId,
    pub from: Vec<u8>, // Public key of sender
    pub transaction_type: TransactionType,
    pub nonce: u64,
    pub timestamp: DateTime<Utc>,
    pub signature: Option<Dilithium3Signature>,
}

impl Transaction {
    pub fn new(
        from: Vec<u8>,
        transaction_type: TransactionType,
        nonce: u64,
    ) -> Self {
        let mut tx = Self {
            id: [0u8; 32],
            from,
            transaction_type,
            nonce,
            timestamp: Utc::now(),
            signature: None,
        };
        
        tx.id = tx.calculate_hash();
        tx
    }
    
    pub fn sign(&mut self, keypair: &Dilithium3Keypair) -> Result<()> {
        let message = self.serialize_for_signing()?;
        self.signature = Some(keypair.sign(&message)?);
        Ok(())
    }
    
    pub fn verify_signature(&self) -> Result<bool> {
        if let Some(ref signature) = self.signature {
            let message = self.serialize_for_signing()?;
            Dilithium3Keypair::verify(&message, signature)
        } else {
            Ok(false)
        }
    }
    
    pub fn calculate_hash(&self) -> TransactionId {
        let data = self.serialize_for_signing().unwrap_or_default();
        blake3_hash(&data)
    }
    
    pub fn get_hash_hex(&self) -> String {
        blake3_hash_hex(&self.id)
    }
    
    fn serialize_for_signing(&self) -> Result<Vec<u8>> {
        // Create a copy without signature for signing
        let tx_for_signing = TransactionForSigning {
            from: self.from.clone(),
            transaction_type: self.transaction_type.clone(),
            nonce: self.nonce,
            timestamp: self.timestamp,
        };
        
        bincode::serialize(&tx_for_signing)
            .map_err(|e| BlockchainError::SerializationError(format!("Failed to serialize transaction: {e}")))
    }
    
    pub fn validate(&self, current_balance: u64, current_nonce: u64) -> Result<()> {
        // Verify signature
        if !self.verify_signature()? {
            return Err(BlockchainError::InvalidSignature("Transaction signature verification failed".to_string()));
        }
        
        // Verify nonce
        if self.nonce != current_nonce {
            return Err(BlockchainError::InvalidNonce {
                expected: current_nonce,
                found: self.nonce,
            });
        }
        
        // Verify sufficient balance
        let required_amount = match &self.transaction_type {
            TransactionType::Transfer { amount, .. } => *amount,
            TransactionType::Stake { amount } => *amount,
            TransactionType::Unstake { amount: _ } => 0, // Unstaking doesn't require balance
            TransactionType::MiningReward { amount: _, block_height: _ } => 0, // Rewards don't require balance
            TransactionType::Governance { .. } => 0, // Voting doesn't require balance
        };
        
        if required_amount > current_balance {
            return Err(BlockchainError::InsufficientBalance(
                format!("Required: {required_amount}, Available: {current_balance}")
            ));
        }
        
        Ok(())
    }
    
    pub fn get_amount(&self) -> u64 {
        match &self.transaction_type {
            TransactionType::Transfer { amount, .. } => *amount,
            TransactionType::Stake { amount } => *amount,
            TransactionType::Unstake { amount } => *amount,
            TransactionType::MiningReward { amount, .. } => *amount,
            TransactionType::Governance { .. } => 0,
        }
    }
    
    pub fn is_reward(&self) -> bool {
        matches!(self.transaction_type, TransactionType::MiningReward { .. })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TransactionForSigning {
    from: Vec<u8>,
    transaction_type: TransactionType,
    nonce: u64,
    timestamp: DateTime<Utc>,
}

impl TransactionType {
    pub fn is_transfer(&self) -> bool {
        matches!(self, TransactionType::Transfer { .. })
    }
    
    pub fn is_stake(&self) -> bool {
        matches!(self, TransactionType::Stake { .. })
    }
    
    pub fn is_unstake(&self) -> bool {
        matches!(self, TransactionType::Unstake { .. })
    }
    
    pub fn is_reward(&self) -> bool {
        matches!(self, TransactionType::MiningReward { .. })
    }
    
    pub fn is_governance(&self) -> bool {
        matches!(self, TransactionType::Governance { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_transaction_creation() {
        let keypair = Dilithium3Keypair::new().unwrap();
        let tx = Transaction::new(
            keypair.public_key.clone(),
            TransactionType::Transfer {
                to: vec![1, 2, 3, 4],
                amount: 100,
            },
            1,
        );
        
        assert_eq!(tx.nonce, 1);
        assert_eq!(tx.get_amount(), 100);
    }
    
    #[test]
    fn test_transaction_signing() {
        let keypair = Dilithium3Keypair::new().unwrap();
        let mut tx = Transaction::new(
            keypair.public_key.clone(),
            TransactionType::Transfer {
                to: vec![1, 2, 3, 4],
                amount: 100,
            },
            1,
        );
        
        tx.sign(&keypair).unwrap();
        assert!(tx.verify_signature().unwrap());
    }
    
    #[test]
    fn test_transaction_validation() {
        let keypair = Dilithium3Keypair::new().unwrap();
        let mut tx = Transaction::new(
            keypair.public_key.clone(),
            TransactionType::Transfer {
                to: vec![1, 2, 3, 4],
                amount: 100,
            },
            1,
        );
        
        tx.sign(&keypair).unwrap();
        assert!(tx.validate(200, 1).is_ok());
        assert!(tx.validate(50, 1).is_err()); // Insufficient balance
        assert!(tx.validate(200, 2).is_err()); // Wrong nonce
    }
} 