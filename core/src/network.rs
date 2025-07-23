use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::prelude::*;
use libp2p::{
    core::upgrade,
    floodsub::{self, Behaviour as Floodsub, Event as FloodsubEvent, Topic},
    identity,
    swarm::{Swarm, SwarmEvent},
    tcp, noise, yamux,
    Multiaddr, PeerId, Transport,
};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, RwLock};

use crate::block::Block;
use crate::transaction::Transaction;
use crate::{Result, BlockchainError};

// AI Agent Note: This is a simplified network implementation for compilation.
// The complex P2P features (Kademlia, mDNS, advanced peer management) have been
// temporarily simplified to get the codebase compiling. Future versions should
// restore the full production-ready networking with proper peer discovery,
// DHT routing, and sophisticated peer management.

const TOPIC_BLOCKS: &str = "numi/blocks/1.0.0";
const TOPIC_TRANSACTIONS: &str = "numi/transactions/1.0.0";
const TOPIC_PEER_INFO: &str = "numi/peer-info/1.0.0";

/// Bootstrap nodes for initial network discovery
const BOOTSTRAP_NODES: &[&str] = &[
    "/ip4/127.0.0.1/tcp/8333",  // Local node for testing
];

/// Network message types for blockchain communication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NetworkMessage {
    /// New block announcement
    NewBlock(Block),
    /// New transaction announcement  
    NewTransaction(Transaction),
    /// Request specific block by hash
    BlockRequest(Vec<u8>),
    /// Request block headers starting from hash
    HeadersRequest { start_hash: Vec<u8>, count: u32 },
    /// Peer information broadcast
    PeerInfo { chain_height: u64, peer_id: String },
    /// Ping for connection health
    Ping { timestamp: u64 },
    /// Pong response to ping
    Pong { timestamp: u64 },
}

/// Peer information and reputation tracking
#[derive(Debug, Clone)]
pub struct PeerInfo {
    pub last_seen: Instant,
    pub reputation: i32,
    pub chain_height: u64,
    pub connection_count: u32,
    pub is_banned: bool,
    pub ban_until: Option<Instant>,
}

impl PeerInfo {
    pub fn new() -> Self {
        Self {
            last_seen: Instant::now(),
            reputation: 0,
            chain_height: 0,
            connection_count: 0,
            is_banned: false,
            ban_until: None,
        }
    }
}

/// Simplified network behavior - using Floodsub directly for now
/// TODO: Implement proper NetworkBehaviour when libp2p API stabilizes
pub type SimpleNetworkBehaviour = Floodsub;

/// Thread-safe network manager wrapper for RPC compatibility
#[derive(Clone)]
pub struct NetworkManagerHandle {
    message_sender: mpsc::UnboundedSender<NetworkMessage>,
    peers: Arc<RwLock<HashMap<PeerId, PeerInfo>>>,
    banned_peers: Arc<RwLock<HashSet<PeerId>>>,
    _local_peer_id: PeerId,
    chain_height: Arc<RwLock<u64>>,
    is_syncing: Arc<RwLock<bool>>,
}

/// Production-ready P2P network manager (simplified version)
pub struct NetworkManager {
    swarm: Swarm<SimpleNetworkBehaviour>,
    peers: Arc<RwLock<HashMap<PeerId, PeerInfo>>>,
    banned_peers: Arc<RwLock<HashSet<PeerId>>>,
    message_sender: mpsc::UnboundedSender<NetworkMessage>,
    message_receiver: mpsc::UnboundedReceiver<NetworkMessage>,
    local_peer_id: PeerId,
    chain_height: Arc<RwLock<u64>>,
    is_syncing: Arc<RwLock<bool>>,
}

impl NetworkManagerHandle {
    /// Get the number of connected peers
    pub async fn get_peer_count(&self) -> usize {
        self.peers.read().await.len()
    }

    /// Check if the node is currently syncing
    pub async fn is_syncing(&self) -> bool {
        *self.is_syncing.read().await
    }

    /// Get current chain height
    pub async fn get_chain_height(&self) -> u64 {
        *self.chain_height.read().await
    }

    /// Broadcast a block to the network
    pub async fn broadcast_block(&self, block: Block) -> Result<()> {
        let message = NetworkMessage::NewBlock(block);
        self.message_sender.send(message)
            .map_err(|e| BlockchainError::NetworkError(format!("Failed to send block: {}", e)))?;
        Ok(())
    }

    /// Broadcast a transaction to the network
    pub async fn broadcast_transaction(&self, transaction: Transaction) -> Result<()> {
        let message = NetworkMessage::NewTransaction(transaction);
        self.message_sender.send(message)
            .map_err(|e| BlockchainError::NetworkError(format!("Failed to send transaction: {}", e)))?;
        Ok(())
    }

    /// Update peer reputation
    pub async fn update_peer_reputation(&self, peer_id: PeerId, delta: i32) {
        let mut peers = self.peers.write().await;
        if let Some(peer) = peers.get_mut(&peer_id) {
            peer.reputation += delta;
            peer.last_seen = Instant::now();
            
            // Ban peer if reputation drops too low
            if peer.reputation < -100 {
                peer.is_banned = true;
                peer.ban_until = Some(Instant::now() + Duration::from_secs(3600)); // 1 hour ban
                log::warn!("🚫 Peer {} banned due to low reputation: {}", peer_id, peer.reputation);
            }
        }
    }

    /// Check if a peer is banned
    pub async fn is_peer_banned(&self, peer_id: &PeerId) -> bool {
        self.banned_peers.read().await.contains(peer_id)
    }
}

impl NetworkManager {
    pub fn new() -> Result<Self> {
        // Generate Ed25519 key pair for peer identity
        let local_key = identity::Keypair::generate_ed25519();
        let local_peer_id = PeerId::from(local_key.public());

        log::info!("🔑 Local peer ID: {}", local_peer_id);

        // Create transport with noise encryption and yamux multiplexing
        let transport = tcp::tokio::Transport::default()
            .upgrade(upgrade::Version::V1Lazy)
            .authenticate(noise::Config::new(&local_key)
                .map_err(|e| BlockchainError::NetworkError(format!("Failed to create noise config: {}", e)))?)
            .multiplex(yamux::Config::default())
            .boxed();

        // Initialize flood-sub for gossip protocol with updated Topic API
        let mut behaviour = Floodsub::new(local_peer_id);
        
        // Create topics using the new API
        let blocks_topic = Topic::new(TOPIC_BLOCKS);
        let transactions_topic = Topic::new(TOPIC_TRANSACTIONS);
        let peer_info_topic = Topic::new(TOPIC_PEER_INFO);
        
        behaviour.subscribe(blocks_topic);
        behaviour.subscribe(transactions_topic);
        behaviour.subscribe(peer_info_topic);

        // Create swarm with config
        let config = libp2p::swarm::Config::with_tokio_executor();
        let swarm = Swarm::new(transport, behaviour, local_peer_id, config);

        let (message_sender, message_receiver) = mpsc::unbounded_channel();

        Ok(Self {
            swarm,
            peers: Arc::new(RwLock::new(HashMap::new())),
            banned_peers: Arc::new(RwLock::new(HashSet::new())),
            message_sender,
            message_receiver,
            local_peer_id,
            chain_height: Arc::new(RwLock::new(0)),
            is_syncing: Arc::new(RwLock::new(false)),
        })
    }

    /// Create a thread-safe handle for RPC server
    pub fn create_handle(&self) -> NetworkManagerHandle {
        NetworkManagerHandle {
            message_sender: self.message_sender.clone(),
            peers: self.peers.clone(),
            banned_peers: self.banned_peers.clone(),
            _local_peer_id: self.local_peer_id,
            chain_height: self.chain_height.clone(),
            is_syncing: self.is_syncing.clone(),
        }
    }

    /// Start the network manager and bind to listening address
    pub async fn start(&mut self, listen_addr: &str) -> Result<()> {
        let addr: Multiaddr = listen_addr.parse()
            .map_err(|e| BlockchainError::NetworkError(format!("Invalid listen address: {}", e)))?;

        self.swarm.listen_on(addr.clone())
            .map_err(|e| BlockchainError::NetworkError(format!("Failed to listen: {}", e)))?;

        log::info!("🌐 Network listening on: {}", addr);
        
        // Connect to bootstrap nodes
        self.bootstrap().await?;
        
        Ok(())
    }

    /// Connect to bootstrap nodes
    async fn bootstrap(&mut self) -> Result<()> {
        for &bootstrap_addr in BOOTSTRAP_NODES {
            if let Ok(addr) = bootstrap_addr.parse::<Multiaddr>() {
                match self.swarm.dial(addr.clone()) {
                    Ok(_) => log::info!("📞 Dialing bootstrap node: {}", addr),
                    Err(e) => log::warn!("❌ Failed to dial bootstrap node {}: {}", addr, e),
                }
            }
        }
        Ok(())
    }

    /// Main event processing loop
    pub async fn run_event_loop(&mut self) {
        let mut maintenance_interval = tokio::time::interval(Duration::from_secs(30));

        loop {
            tokio::select! {
                // Handle swarm events
                event = self.swarm.select_next_some() => {
                    if let Err(e) = self.handle_swarm_event(event).await {
                        log::error!("Error handling swarm event: {}", e);
                    }
                }
                
                // Handle outgoing messages
                message = self.message_receiver.recv() => {
                    if let Some(msg) = message {
                        if let Err(e) = self.handle_outgoing_message(msg).await {
                            log::error!("Error handling outgoing message: {}", e);
                        }
                    }
                }
                
                // Periodic maintenance
                _ = maintenance_interval.tick() => {
                    self.perform_maintenance().await;
                }
            }
        }
    }

    /// Handle events from libp2p swarm
    async fn handle_swarm_event(&mut self, event: SwarmEvent<FloodsubEvent>) -> Result<()> {
        match event {
            SwarmEvent::Behaviour(FloodsubEvent::Message(msg)) => {
                self.handle_floodsub_message(msg).await?;
            }
            SwarmEvent::NewListenAddr { address, .. } => {
                log::info!("🌐 New listen address: {}", address);
            }
            SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                self.on_peer_connected(peer_id).await;
            }
            SwarmEvent::ConnectionClosed { peer_id, .. } => {
                self.on_peer_disconnected(peer_id).await;
            }
            _ => {}
        }
        Ok(())
    }

    /// Handle incoming floodsub messages
    async fn handle_floodsub_message(&mut self, message: floodsub::FloodsubMessage) -> Result<()> {
        // Extract topic string - use debug formatting
        let topic_str = if let Some(topic) = message.topics.first() {
            format!("{:?}", topic)
        } else {
            String::new()
        };
        let data = message.data;

        match topic_str.as_str() {
            TOPIC_BLOCKS => {
                if let Ok(network_message) = bincode::deserialize::<NetworkMessage>(&data) {
                    if let NetworkMessage::NewBlock(block) = network_message {
                        log::info!("📦 Received new block: {}", hex::encode(&block.calculate_hash()));
                        // TODO: Process new block
                    }
                }
            }
            TOPIC_TRANSACTIONS => {
                if let Ok(network_message) = bincode::deserialize::<NetworkMessage>(&data) {
                    if let NetworkMessage::NewTransaction(tx) = network_message {
                        log::info!("💸 Received new transaction: {}", hex::encode(&tx.id));
                        // TODO: Process new transaction
                    }
                }
            }
            TOPIC_PEER_INFO => {
                if let Ok(network_message) = bincode::deserialize::<NetworkMessage>(&data) {
                    if let NetworkMessage::PeerInfo { chain_height, peer_id } = network_message {
                        log::debug!("👥 Peer info: {} at height {}", peer_id, chain_height);
                        // TODO: Update peer information
                    }
                }
            }
            _ => {
                log::debug!("📨 Unknown message topic: {}", topic_str);
            }
        }
        Ok(())
    }

    /// Handle outgoing messages
    async fn handle_outgoing_message(&mut self, message: NetworkMessage) -> Result<()> {
        let (topic, data) = match &message {
            NetworkMessage::NewBlock(_) => (TOPIC_BLOCKS, bincode::serialize(&message)?),
            NetworkMessage::NewTransaction(_) => (TOPIC_TRANSACTIONS, bincode::serialize(&message)?),
            NetworkMessage::PeerInfo { .. } => (TOPIC_PEER_INFO, bincode::serialize(&message)?),
            _ => return Ok(()), // Skip other message types for now
        };

        self.swarm
            .behaviour_mut()
            .publish(Topic::new(topic), data);
        
        Ok(())
    }

    /// Broadcast a block to the network
    pub async fn broadcast_block(&self, block: Block) -> Result<()> {
        let message = NetworkMessage::NewBlock(block);
        self.message_sender.send(message)
            .map_err(|e| BlockchainError::NetworkError(format!("Failed to send block: {}", e)))?;
        Ok(())
    }

    /// Broadcast a transaction to the network
    pub async fn broadcast_transaction(&self, transaction: Transaction) -> Result<()> {
        let message = NetworkMessage::NewTransaction(transaction);
        self.message_sender.send(message)
            .map_err(|e| BlockchainError::NetworkError(format!("Failed to send transaction: {}", e)))?;
        Ok(())
    }

    /// Handle peer connection
    async fn on_peer_connected(&self, peer_id: PeerId) {
        log::info!("🔗 Peer connected: {}", peer_id);
        let mut peers = self.peers.write().await;
        peers.entry(peer_id).or_insert_with(PeerInfo::new);
    }

    /// Handle peer disconnection
    async fn on_peer_disconnected(&self, peer_id: PeerId) {
        log::info!("🔌 Peer disconnected: {}", peer_id);
        // Keep peer info for potential reconnection
    }

    /// Update peer reputation
    pub async fn update_peer_reputation(&self, peer_id: PeerId, delta: i32) {
        let mut peers = self.peers.write().await;
        if let Some(peer) = peers.get_mut(&peer_id) {
            peer.reputation += delta;
            peer.last_seen = Instant::now();
            
            // Ban peer if reputation drops too low
            if peer.reputation < -100 {
                peer.is_banned = true;
                peer.ban_until = Some(Instant::now() + Duration::from_secs(3600)); // 1 hour ban
                log::warn!("🚫 Peer {} banned due to low reputation: {}", peer_id, peer.reputation);
            }
        }
    }

    /// Check if a peer is banned
    pub async fn is_peer_banned(&self, peer_id: &PeerId) -> bool {
        self.banned_peers.read().await.contains(peer_id)
    }

    /// Perform periodic maintenance tasks
    async fn perform_maintenance(&mut self) {
        let now = Instant::now();
        let mut peers = self.peers.write().await;
        let mut banned_peers = self.banned_peers.write().await;

        // Remove old peer entries
        peers.retain(|_, peer| {
            if peer.is_banned {
                if let Some(ban_until) = peer.ban_until {
                    if now > ban_until {
                        peer.is_banned = false;
                        peer.ban_until = None;
                        peer.reputation = 0; // Reset reputation after ban expires
                        log::info!("✅ Peer ban expired, reputation reset");
                    }
                }
            }
            
            // Remove peers not seen for more than 1 hour
            now.duration_since(peer.last_seen) < Duration::from_secs(3600)
        });

        // Update banned peers set
        banned_peers.clear();
        for (peer_id, peer) in peers.iter() {
            if peer.is_banned {
                banned_peers.insert(*peer_id);
            }
        }

        log::debug!("🧹 Maintenance: {} active peers, {} banned peers", 
                   peers.len(), banned_peers.len());
    }

    /// Get the number of connected peers
    pub async fn get_peer_count(&self) -> usize {
        self.peers.read().await.len()
    }

    /// Get local peer ID
    pub fn get_local_peer_id(&self) -> PeerId {
        self.local_peer_id
    }

    /// Check if the node is currently syncing
    pub async fn is_syncing(&self) -> bool {
        *self.is_syncing.read().await
    }

    /// Set syncing status
    pub async fn set_syncing(&mut self, syncing: bool) {
        *self.is_syncing.write().await = syncing;
    }

    /// Get current chain height
    pub async fn get_chain_height(&self) -> u64 {
        *self.chain_height.read().await
    }

    /// Set current chain height
    pub async fn set_chain_height(&mut self, height: u64) {
        *self.chain_height.write().await = height;
    }
} 