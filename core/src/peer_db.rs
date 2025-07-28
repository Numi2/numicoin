use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::error::BlockchainError;
use crate::Result;
use libp2p::PeerId;

/// A thread-safe, in-memory database for storing peer information.
#[derive(Clone)]
pub struct PeerDB {
    peers: Arc<RwLock<HashMap<PeerId, PeerInfo>>>,
}

/// Information about a peer, including their public key for signature verification.
#[derive(Clone, Debug)]
pub struct PeerInfo {
    pub dilithium_pk: Vec<u8>,
    pub kyber_pk: Vec<u8>,
    pub last_nonce: u64,
}

impl Default for PeerDB {
    fn default() -> Self {
        Self::new()
    }
}

impl PeerDB {
    /// Creates a new, empty `PeerDB`.
    pub fn new() -> Self {
        Self {
            peers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Adds a new peer to the database.
    pub async fn add_peer(&self, peer_id: PeerId, dilithium_pk: Vec<u8>, kyber_pk: Vec<u8>) {
        let peers = self.peers.clone();
        tokio::spawn(async move {
            let mut guard = peers.write().await;
            guard.insert(peer_id, PeerInfo { dilithium_pk, kyber_pk, last_nonce: 0 });
        }).await.ok();
    }

    /// Retrieves the information for a given peer.
    pub async fn get_peer(&self, peer_id: &PeerId) -> Option<PeerInfo> {
        self.peers.read().await.get(peer_id).cloned()
    }

    /// Updates the last-seen nonce for a peer to prevent replay attacks.
    pub async fn update_peer_nonce(&self, peer_id: &PeerId, nonce: u64) -> Result<()> {
        let mut peers = self.peers.write().await;
        if let Some(peer_info) = peers.get_mut(peer_id) {
            if nonce > peer_info.last_nonce {
                peer_info.last_nonce = nonce;
                Ok(())
            } else {
                Err(BlockchainError::InvalidNonce {
                    expected: peer_info.last_nonce + 1,
                    found: nonce,
                })
            }
        } else {
            Err(BlockchainError::PeerNotFound)
        }
    }
} 