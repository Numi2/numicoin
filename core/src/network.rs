use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use crate::block::Block;
use crate::transaction::Transaction;
use crate::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NetworkMessage {
    NewBlock(Block),
    NewTransaction(Transaction),
    BlockRequest(u64),
    BlockResponse(Block),
    ChainRequest,
    ChainResponse(Vec<Block>),
    Ping,
    Pong,
}

pub struct NetworkNode {
    peer_id: String,
    peers: Arc<RwLock<HashMap<String, String>>>,
    message_sender: mpsc::Sender<NetworkMessage>,
    message_receiver: mpsc::Receiver<NetworkMessage>,
}

impl NetworkNode {
    pub fn new() -> Result<Self> {
        let peer_id = format!("peer-{}", uuid::Uuid::new_v4());
        println!("🔑 Local peer id: {}", peer_id);

        let (message_sender, message_receiver) = mpsc::channel(1000);

        Ok(Self {
            peer_id,
            peers: Arc::new(RwLock::new(HashMap::new())),
            message_sender,
            message_receiver,
        })
    }

    pub async fn start(&mut self, listen_addr: &str) -> Result<()> {
        println!("🌐 Starting network node on {}", listen_addr);
        
        // Start message handling loop
        while let Some(message) = self.message_receiver.recv().await {
            self.handle_message(message).await?;
        }
        
        Ok(())
    }

    async fn handle_message(&mut self, message: NetworkMessage) -> Result<()> {
        match message {
            NetworkMessage::NewBlock(block) => {
                println!("📦 Received new block: {}", block.header.height);
            }
            NetworkMessage::NewTransaction(transaction) => {
                println!("💸 Received new transaction: {}", transaction.get_hash_hex());
            }
            NetworkMessage::BlockRequest(height) => {
                println!("📥 Received block request for height: {}", height);
            }
            NetworkMessage::BlockResponse(block) => {
                println!("📤 Received block response: {}", block.header.height);
            }
            NetworkMessage::ChainRequest => {
                println!("🔗 Received chain request");
            }
            NetworkMessage::ChainResponse(blocks) => {
                println!("🔗 Received chain response with {} blocks", blocks.len());
            }
            NetworkMessage::Ping => {
                println!("🏓 Received ping");
                // Send pong back
                let pong = NetworkMessage::Pong;
                if let Err(e) = self.message_sender.send(pong).await {
                    eprintln!("Failed to send pong: {}", e);
                }
            }
            NetworkMessage::Pong => {
                println!("🏓 Received pong");
            }
        }
        Ok(())
    }

    pub async fn broadcast_block(&mut self, block: Block) -> Result<()> {
        println!("📤 Broadcasted block: {}", block.header.height);
        let message = NetworkMessage::NewBlock(block);
        self.message_sender.send(message).await
            .map_err(|e| crate::error::BlockchainError::NetworkError(format!("Failed to broadcast block: {}", e)))?;
        Ok(())
    }

    pub async fn broadcast_transaction(&mut self, transaction: Transaction) -> Result<()> {
        println!("📤 Broadcasted transaction: {}", transaction.get_hash_hex());
        let message = NetworkMessage::NewTransaction(transaction);
        self.message_sender.send(message).await
            .map_err(|e| crate::error::BlockchainError::NetworkError(format!("Failed to broadcast transaction: {}", e)))?;
        Ok(())
    }

    pub async fn connect_to_peer(&mut self, addr: &str) -> Result<()> {
        let mut peers = self.peers.write().unwrap();
        peers.insert(addr.to_string(), "connected".to_string());
        println!("🔗 Connected to peer: {}", addr);
        Ok(())
    }

    pub fn get_peer_count(&self) -> usize {
        self.peers.read().unwrap().len()
    }

    pub fn get_peers(&self) -> Vec<String> {
        self.peers.read().unwrap().keys().cloned().collect()
    }

    pub async fn request_block(&mut self, height: u64) -> Result<()> {
        let message = NetworkMessage::BlockRequest(height);
        self.message_sender.send(message).await
            .map_err(|e| crate::error::BlockchainError::NetworkError(format!("Failed to request block: {}", e)))?;
        println!("📥 Requested block at height: {}", height);
        Ok(())
    }

    pub async fn request_chain(&mut self) -> Result<()> {
        let message = NetworkMessage::ChainRequest;
        self.message_sender.send(message).await
            .map_err(|e| crate::error::BlockchainError::NetworkError(format!("Failed to request chain: {}", e)))?;
        println!("📥 Requested full chain");
        Ok(())
    }

    pub async fn ping_peers(&mut self) -> Result<()> {
        let message = NetworkMessage::Ping;
        self.message_sender.send(message).await
            .map_err(|e| crate::error::BlockchainError::NetworkError(format!("Failed to ping peers: {}", e)))?;
        println!("🏓 Pinging peers");
        Ok(())
    }
}

pub struct NetworkManager {
    node: Option<NetworkNode>,
}

impl NetworkManager {
    pub fn new() -> Self {
        Self { node: None }
    }

    pub async fn start(&mut self, listen_addr: &str) -> Result<()> {
        let node = NetworkNode::new()?;
        println!("🌐 Network manager started on {}", listen_addr);
        self.node = Some(node);
        Ok(())
    }

    pub async fn broadcast_block(&mut self, block: Block) -> Result<()> {
        if let Some(ref mut node) = self.node {
            node.broadcast_block(block).await?;
        }
        Ok(())
    }

    pub async fn broadcast_transaction(&mut self, transaction: Transaction) -> Result<()> {
        if let Some(ref mut node) = self.node {
            node.broadcast_transaction(transaction).await?;
        }
        Ok(())
    }

    pub async fn connect_to_peer(&mut self, addr: &str) -> Result<()> {
        if let Some(ref mut node) = self.node {
            node.connect_to_peer(addr).await?;
        }
        Ok(())
    }

    pub fn get_peer_count(&self) -> usize {
        self.node.as_ref().map(|n| n.get_peer_count()).unwrap_or(0)
    }

    pub fn get_peers(&self) -> Vec<String> {
        self.node.as_ref().map(|n| n.get_peers()).unwrap_or_default()
    }

    pub async fn ping_peers(&mut self) -> Result<()> {
        if let Some(ref mut node) = self.node {
            node.ping_peers().await?;
        }
        Ok(())
    }
}

// Enhanced P2P Network Layer (Future Implementation)
// This is a placeholder for a more sophisticated libp2p implementation
// that could be added later when the API stabilizes
pub struct P2PNetworkLayer {
    enabled: bool,
}

impl P2PNetworkLayer {
    pub fn new() -> Self {
        Self { enabled: false }
    }

    pub async fn initialize(&mut self) -> Result<()> {
        // Future: Initialize real libp2p networking here
        println!("🔮 P2P Network Layer: Ready for future libp2p integration");
        self.enabled = true;
        Ok(())
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_network_node_creation() {
        let node = NetworkNode::new();
        assert!(node.is_ok());
    }

    #[tokio::test]
    async fn test_network_manager_creation() {
        let manager = NetworkManager::new();
        assert_eq!(manager.get_peer_count(), 0);
    }

    #[test]
    fn test_network_message_serialization() {
        let message = NetworkMessage::Ping;
        let serialized = serde_json::to_vec(&message);
        assert!(serialized.is_ok());
        
        let deserialized: NetworkMessage = serde_json::from_slice(&serialized.unwrap()).unwrap();
        matches!(deserialized, NetworkMessage::Ping);
    }

    #[tokio::test]
    async fn test_peer_connection() {
        let mut node = NetworkNode::new().unwrap();
        let result = node.connect_to_peer("/ip4/127.0.0.1/tcp/8000").await;
        assert!(result.is_ok());
        assert_eq!(node.get_peer_count(), 1);
    }

    #[test]
    fn test_p2p_layer_initialization() {
        let mut layer = P2PNetworkLayer::new();
        assert!(!layer.is_enabled());
    }
} 