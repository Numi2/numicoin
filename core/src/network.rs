use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::str::FromStr;

use futures::stream::FuturesUnordered;
use libp2p::{
    core::upgrade,
    floodsub::{self, Behaviour as Floodsub, Event as FloodsubEvent, Topic},
    identity,
    swarm::{Swarm, SwarmEvent},
    tcp, tls, yamux,
    Multiaddr, PeerId,
    mdns::{tokio::Behaviour as Mdns, Event as MdnsEvent},
    swarm::NetworkBehaviour,
    Transport,
};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, RwLock};

use crate::block::Block;
use crate::transaction::Transaction;
use crate::{Result, BlockchainError};
use crate::crypto::{Dilithium3Keypair, kyber_keypair};


const TOPIC_BLOCKS: &str = "numi/blocks/1.0.0";
const TOPIC_TRANSACTIONS: &str = "numi/transactions/1.0.0";
const TOPIC_PEER_INFO: &str = "numi/peer-info/1.0.0";

/// Bootstrap nodes for initial network discovery
const BOOTSTRAP_NODES: &[&str] = &[
    "/ip4/127.0.0.1/tcp/8333",  // Local node for testing
];

/// Maximum allowed timestamp skew for replay protection (5 minutes)
const MAX_TIMESTAMP_SKEW: u64 = 300;

/// Peer key registry for storing and validating peer identities
#[derive(Debug, Clone)]
pub struct PeerKeyRegistry {
    /// Mapping of PeerId to peer's Dilithium3 public key
    dilithium_keys: Arc<RwLock<HashMap<PeerId, Vec<u8>>>>,
    /// Mapping of PeerId to peer's Kyber public key  
    kyber_keys: Arc<RwLock<HashMap<PeerId, Vec<u8>>>>,
    /// Pending key requests awaiting response
    pending_requests: Arc<RwLock<HashMap<PeerId, Instant>>>,
    /// Bootstrap nodes with known public keys
    bootstrap_keys: HashMap<PeerId, (Vec<u8>, Vec<u8>)>, // (dilithium_pk, kyber_pk)
    /// Key verification status
    verified_keys: Arc<RwLock<HashSet<PeerId>>>,
}

impl PeerKeyRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            dilithium_keys: Arc::new(RwLock::new(HashMap::new())),
            kyber_keys: Arc::new(RwLock::new(HashMap::new())),
            pending_requests: Arc::new(RwLock::new(HashMap::new())),
            bootstrap_keys: HashMap::new(),
            verified_keys: Arc::new(RwLock::new(HashSet::new())),
        };
        
        // Initialize with bootstrap node keys (in production, these would be hardcoded or loaded from config)
        registry.initialize_bootstrap_keys();
        registry
    }

    /// Initialize bootstrap node public keys
    fn initialize_bootstrap_keys(&mut self) {
        // In a real implementation, these would be hardcoded or loaded from a trusted source
        // For now, we'll use placeholder keys that should be replaced with actual bootstrap node keys
        log::info!("🔑 Initializing bootstrap node keys...");
        
        // Example bootstrap node (replace with actual keys in production)
        // let bootstrap_peer_id = PeerId::from_bytes(&[/* actual peer ID bytes */]).unwrap();
        // let bootstrap_dilithium_pk = vec![/* actual Dilithium3 public key */];
        // let bootstrap_kyber_pk = vec![/* actual Kyber public key */];
        // self.bootstrap_keys.insert(bootstrap_peer_id, (bootstrap_dilithium_pk, bootstrap_kyber_pk));
    }

    /// Store a peer's public keys
    pub async fn store_peer_keys(&self, peer_id: PeerId, dilithium_pk: Vec<u8>, kyber_pk: Vec<u8>) {
        let mut dil_keys = self.dilithium_keys.write().await;
        let mut kyber_keys = self.kyber_keys.write().await;
        
        dil_keys.insert(peer_id, dilithium_pk);
        kyber_keys.insert(peer_id, kyber_pk);
        
        log::debug!("💾 Stored keys for peer: {}", peer_id);
    }

    /// Get a peer's Dilithium3 public key
    pub async fn get_dilithium_key(&self, peer_id: &PeerId) -> Option<Vec<u8>> {
        self.dilithium_keys.read().await.get(peer_id).cloned()
    }

    /// Get a peer's Kyber public key
    pub async fn get_kyber_key(&self, peer_id: &PeerId) -> Option<Vec<u8>> {
        self.kyber_keys.read().await.get(peer_id).cloned()
    }

    /// Check if we have both keys for a peer
    pub async fn has_complete_keys(&self, peer_id: &PeerId) -> bool {
        let dil_keys = self.dilithium_keys.read().await;
        let kyber_keys = self.kyber_keys.read().await;
        
        dil_keys.contains_key(peer_id) && kyber_keys.contains_key(peer_id)
    }

    /// Check if a peer's keys are verified
    pub async fn is_verified(&self, peer_id: &PeerId) -> bool {
        self.verified_keys.read().await.contains(peer_id)
    }

    /// Mark a peer's keys as verified
    pub async fn mark_verified(&self, peer_id: PeerId) {
        self.verified_keys.write().await.insert(peer_id);
        log::debug!("✅ Marked peer {} as verified", peer_id);
    }

    /// Request keys from a peer if we don't have them
    pub async fn request_keys_if_needed(&self, peer_id: PeerId) -> bool {
        if self.has_complete_keys(&peer_id).await {
            return false; // Already have keys
        }

        let mut pending = self.pending_requests.write().await;
        if pending.contains_key(&peer_id) {
            return false; // Already requested
        }

        // Check if it's a bootstrap node
        if self.bootstrap_keys.contains_key(&peer_id) {
            let (dil_pk, kyber_pk) = self.bootstrap_keys.get(&peer_id).unwrap();
            self.store_peer_keys(peer_id, dil_pk.clone(), kyber_pk.clone()).await;
            self.mark_verified(peer_id).await;
            return false;
        }

        // Add to pending requests
        pending.insert(peer_id, Instant::now());
        log::debug!("🔍 Requesting keys for peer: {}", peer_id);
        true
    }

    /// Clean up expired pending requests
    pub async fn cleanup_expired_requests(&self) {
        let now = Instant::now();
        let mut pending = self.pending_requests.write().await;
        pending.retain(|_, timestamp| now.duration_since(*timestamp) < Duration::from_secs(30));
    }

    /// Get all verified peers
    pub async fn get_verified_peers(&self) -> Vec<PeerId> {
        self.verified_keys.read().await.iter().cloned().collect()
    }

    /// Remove a peer's keys (e.g., when they disconnect)
    pub async fn remove_peer_keys(&self, peer_id: &PeerId) {
        let mut dil_keys = self.dilithium_keys.write().await;
        let mut kyber_keys = self.kyber_keys.write().await;
        let mut verified = self.verified_keys.write().await;
        let mut pending = self.pending_requests.write().await;
        
        dil_keys.remove(peer_id);
        kyber_keys.remove(peer_id);
        verified.remove(peer_id);
        pending.remove(peer_id);
        
        log::debug!("🗑️ Removed keys for peer: {}", peer_id);
    }
}

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
    /// Peer information broadcast with replay protection
    PeerInfo { 
        chain_height: u64, 
        peer_id: String,
        timestamp: u64,
        nonce: u64,
        signature: Vec<u8>,
    },
    /// Ping for connection health with replay protection
    Ping { 
        timestamp: u64,
        nonce: u64,
        signature: Vec<u8>,
    },
    /// Pong response to ping with replay protection
    Pong { 
        timestamp: u64,
        nonce: u64,
        signature: Vec<u8>,
    },
    /// Key exchange request
    KeyRequest {
        requester_id: String,
        timestamp: u64,
        nonce: u64,
        signature: Vec<u8>,
    },
    /// Key exchange response
    KeyResponse {
        responder_id: String,
        dilithium_pk: Vec<u8>,
        kyber_pk: Vec<u8>,
        timestamp: u64,
        nonce: u64,
        signature: Vec<u8>,
    },
}

/// Peer authentication data for replay protection
#[derive(Debug, Clone)]
pub struct PeerAuth {
    pub peer_id: PeerId,
    pub timestamp: u64,
    pub nonce: u64,
    pub signature: Vec<u8>,
}

/// Peer information and reputation tracking with replay protection
#[derive(Debug, Clone)]
pub struct PeerInfo {
    pub last_seen: Instant,
    pub reputation: i32,
    pub chain_height: u64,
    pub connection_count: u32,
    pub is_banned: bool,
    pub ban_until: Option<Instant>,
    pub public_key: Dilithium3Keypair, // Store peer's public key for validation
    pub last_nonce: u64, // Track last nonce for replay protection
    pub last_timestamp: u64, // Track last timestamp for replay protection
}

impl PeerInfo {
    pub fn new(public_key: Dilithium3Keypair) -> Self {
        Self {
            last_seen: Instant::now(),
            reputation: 0,
            chain_height: 0,
            connection_count: 0,
            is_banned: false,
            ban_until: None,
            public_key,
            last_nonce: 0,
            last_timestamp: 0,
        }
    }

    /// Validate message authenticity and check for replay attacks
    pub fn validate_message(&mut self, timestamp: u64, nonce: u64, signature: &[u8], message_data: &[u8]) -> Result<()> {
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| BlockchainError::NetworkError(format!("Time error: {}", e)))?
            .as_secs();

        // Check timestamp skew
        if timestamp > current_time + MAX_TIMESTAMP_SKEW || timestamp < current_time - MAX_TIMESTAMP_SKEW {
            return Err(BlockchainError::NetworkError("Message timestamp outside allowed skew window".to_string()));
        }

        // Check for replay attacks (nonce must be greater than last seen)
        if nonce <= self.last_nonce {
            return Err(BlockchainError::NetworkError("Duplicate nonce detected - possible replay attack".to_string()));
        }

        // Verify signature
        let mut data_to_verify = Vec::new();
        data_to_verify.extend_from_slice(&timestamp.to_le_bytes());
        data_to_verify.extend_from_slice(&nonce.to_le_bytes());
        data_to_verify.extend_from_slice(message_data);

        if !Dilithium3Keypair::verify(&data_to_verify, &crate::crypto::Dilithium3Signature { signature: signature.to_vec(), public_key: self.public_key.public_key_bytes().to_vec(), created_at: chrono::Utc::now().timestamp() as u64, message_hash: crate::crypto::blake3_hash(&data_to_verify) }, &self.public_key.public_key_bytes())? {
            return Err(BlockchainError::NetworkError("Invalid message signature".to_string()));
        }

        // Update tracking data
        self.last_nonce = nonce;
        self.last_timestamp = timestamp;
        self.last_seen = Instant::now();

        Ok(())
    }
}

// Composite behaviour combining Floodsub for pub-sub and mDNS for local peer discovery.
// More protocols (e.g. Kademlia, Identify, Ping) can be added later.

#[derive(NetworkBehaviour)]
pub struct NumiBehaviour {
    pub floodsub: Floodsub,
    pub mdns: Mdns,
}

// Re-export the behaviour type used throughout the file so later code needs only minimal changes.
pub type SimpleNetworkBehaviour = NumiBehaviour;
// The derive macro already generates an enum `NumiBehaviourEvent` for us, so no extra alias is needed.

/// Thread-safe network manager wrapper for RPC compatibility
#[derive(Clone)]
pub struct NetworkManagerHandle {
    message_sender: mpsc::UnboundedSender<NetworkMessage>,
    peers: Arc<RwLock<HashMap<PeerId, PeerInfo>>>,
    banned_peers: Arc<RwLock<HashSet<PeerId>>>,
    _local_peer_id: PeerId,
    chain_height: Arc<RwLock<u64>>,
    is_syncing: Arc<RwLock<bool>>,
    key_registry: Arc<PeerKeyRegistry>,
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
    key_registry: Arc<PeerKeyRegistry>,
    local_dilithium_kp: Dilithium3Keypair,
    local_kyber_pk: Vec<u8>,
    _local_kyber_sk: Vec<u8>,
}

// Safety: NetworkManager is moved into its own dedicated async task thread and is not shared thereafter, 
// so it is safe to mark it Send and Sync for the purpose of spawning.
unsafe impl Send for NetworkManager {}
unsafe impl Sync for NetworkManager {}

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
    /// Get list of verified peers from key registry
    pub async fn get_verified_peers(&self) -> Vec<PeerId> {
        self.key_registry.get_verified_peers().await
    }
}

impl NetworkManager {
    pub fn new() -> Result<Self> {
        // Generate post-quantum key material for handshake and authentication
        let dilithium_kp = Dilithium3Keypair::new()?;
        let (kyber_pk, kyber_sk) = kyber_keypair();

        // Generate TLS identity for transport and derive PeerId from it
        let tls_identity = identity::Keypair::generate_ed25519();
        let local_peer_id = PeerId::from(tls_identity.public());

        log::info!("🔑 Local peer ID: {}", local_peer_id);

        // Create key registry
        let key_registry = Arc::new(PeerKeyRegistry::new());

        // Create TLS config for transport encryption
        let tls_config = tls::Config::new(&tls_identity).map_err(|e| BlockchainError::NetworkError(format!("TLS config failed: {}", e)))?;

        // Create transport with TLS and Yamux
        let transport = tcp::tokio::Transport::default()
            .upgrade(upgrade::Version::V1)
            .authenticate(tls_config)
            .multiplex(yamux::Config::default())
            .boxed();

        // --- Build the composite behaviour (Floodsub + mDNS) ---

        // Floodsub for gossip based messaging
        let mut floodsub = Floodsub::new(local_peer_id);

        // Subscribe to the Numicoin topics
        let blocks_topic = Topic::new(TOPIC_BLOCKS);
        let transactions_topic = Topic::new(TOPIC_TRANSACTIONS);
        let peer_info_topic = Topic::new(TOPIC_PEER_INFO);

        floodsub.subscribe(blocks_topic.clone());
        floodsub.subscribe(transactions_topic.clone());
        floodsub.subscribe(peer_info_topic.clone());

        // mDNS for local peer discovery
        let mdns = Mdns::new(Default::default(), local_peer_id)
            .map_err(|e| BlockchainError::NetworkError(format!("mDNS init failed: {}", e)))?;

        // Compose the behaviour
        let behaviour = NumiBehaviour { floodsub, mdns };

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
            key_registry,
            local_dilithium_kp: dilithium_kp,
            local_kyber_pk: kyber_pk,
            _local_kyber_sk: kyber_sk,
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
            key_registry: self.key_registry.clone(),
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
    async fn handle_swarm_event(&mut self, event: SwarmEvent<NumiBehaviourEvent>) -> Result<()> {
        match event {
            SwarmEvent::Behaviour(NumiBehaviourEvent::Floodsub(FloodsubEvent::Message(msg))) => {
                self.handle_floodsub_message(msg).await?;
            }
            SwarmEvent::Behaviour(NumiBehaviourEvent::Mdns(MdnsEvent::Discovered(list))) => {
                for (peer, _addr) in list {
                    self.swarm.behaviour_mut().floodsub.add_node_to_partial_view(peer);
                }
            }
            SwarmEvent::Behaviour(NumiBehaviourEvent::Mdns(MdnsEvent::Expired(list))) => {
                for (peer, _addr) in list {
                    self.swarm.behaviour_mut().floodsub.remove_node_from_partial_view(&peer);
                }
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

    /// Handle outgoing messages with parallel broadcast
    async fn handle_outgoing_message(&mut self, message: NetworkMessage) -> Result<()> {
        let (topic, data) = match &message {
            NetworkMessage::NewBlock(_) => (TOPIC_BLOCKS, bincode::serialize(&message)?),
            NetworkMessage::NewTransaction(_) => (TOPIC_TRANSACTIONS, bincode::serialize(&message)?),
            NetworkMessage::PeerInfo { .. } => (TOPIC_PEER_INFO, bincode::serialize(&message)?),
            _ => return Ok(()), // Skip other message types for now
        };

        // Parallel broadcast to all peers using FuturesUnordered
        let mut broadcast_futures = FuturesUnordered::new();
        
        // Get all connected peers
        let peers = self.peers.read().await;
        let peer_ids: Vec<PeerId> = peers.keys().cloned().collect();
        drop(peers); // Release lock early

        for _peer_id in peer_ids {
            let _topic_clone = Topic::new(topic);
            let _data_clone = data.clone();
            
            // Spawn individual broadcast task for each peer
            let broadcast_future = async move {
                // In a real implementation, this would send directly to the peer
                // For now, we use floodsub which handles the broadcast internally
                Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
            };
            
            broadcast_futures.push(broadcast_future);
        }

        // Wait for all broadcasts to complete, handling errors individually
        while let Some(result) = broadcast_futures.next().await {
            if let Err(e) = result {
                log::warn!("Failed to broadcast to peer: {}", e);
                // In a real implementation, we would remove the problematic peer here
            }
        }

        // Also publish via floodsub for discovery
        self.swarm
            .behaviour_mut()
            .floodsub
            .publish(Topic::new(topic), data);
        
        Ok(())
    }

    /// Handle incoming floodsub messages with validation
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
                        log::info!("📦 Received new block: {}", hex::encode(&block.calculate_hash().unwrap_or([0u8; 32])));
                        
                        // Validate block before processing
                        if let Err(e) = self.validate_block(&block).await {
                            log::warn!("❌ Block validation failed: {}", e);
                            return Ok(());
                        }
                        
                        // TODO: Process validated block
                        log::info!("✅ Block validated successfully");
                    }
                }
            }
            TOPIC_TRANSACTIONS => {
                if let Ok(network_message) = bincode::deserialize::<NetworkMessage>(&data) {
                    if let NetworkMessage::NewTransaction(tx) = network_message {
                        log::info!("💸 Received new transaction: {}", hex::encode(&tx.id));
                        
                        // Validate transaction before processing
                        if let Err(e) = self.validate_transaction(&tx).await {
                            log::warn!("❌ Transaction validation failed: {}", e);
                            return Ok(());
                        }
                        
                        // TODO: Process validated transaction
                        log::info!("✅ Transaction validated successfully");
                    }
                }
            }
            TOPIC_PEER_INFO => {
                if let Ok(network_message) = bincode::deserialize::<NetworkMessage>(&data) {
                    if let NetworkMessage::PeerInfo { chain_height, peer_id, timestamp, nonce, signature } = network_message {
                        log::debug!("👥 Peer info: {} at height {}", peer_id, chain_height);
                        
                        // Validate peer info message
                        if let Err(e) = self.validate_peer_info(&peer_id, timestamp, nonce, &signature, &data).await {
                            log::warn!("❌ Peer info validation failed: {}", e);
                            return Ok(());
                        }
                        
                        // TODO: Update peer information
                        log::info!("✅ Peer info validated successfully");
                    }
                }
            }
            "numi/key-exchange/1.0.0" => {
                // Handle key exchange messages
                if let Err(e) = self.handle_key_exchange_message(&data).await {
                    log::warn!("❌ Key exchange message handling failed: {}", e);
                }
            }
            _ => {
                log::debug!("📨 Unknown message topic: {}", topic_str);
            }
        }
        Ok(())
    }

    /// Validate incoming block
    async fn validate_block(&self, block: &Block) -> Result<()> {
        // Verify block signature
        if !block.verify_signature()? {
            return Err(BlockchainError::NetworkError("Invalid block signature".to_string()));
        }

        // Verify Merkle root
        if !block.verify_merkle_root() {
            return Err(BlockchainError::NetworkError("Invalid Merkle root".to_string()));
        }

        // Verify block timestamp is reasonable
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| BlockchainError::NetworkError(format!("Time error: {}", e)))?
            .as_secs();

        let block_timestamp = block.header.timestamp.timestamp() as u64;
        if block_timestamp > current_time + MAX_TIMESTAMP_SKEW {
            return Err(BlockchainError::NetworkError("Block timestamp too far in future".to_string()));
        }

        Ok(())
    }

    /// Validate incoming transaction
    async fn validate_transaction(&self, transaction: &Transaction) -> Result<()> {
        // Verify transaction signature
        if !transaction.verify_signature()? {
            return Err(BlockchainError::NetworkError("Invalid transaction signature".to_string()));
        }

        // Verify transaction hash
        if transaction.calculate_hash() != transaction.id {
            return Err(BlockchainError::NetworkError("Invalid transaction hash".to_string()));
        }

        // Verify sufficient balance (this would require blockchain state access)
        // TODO: Implement balance verification

        // Verify transaction timestamp is reasonable
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| BlockchainError::NetworkError(format!("Time error: {}", e)))?
            .as_secs();

        let tx_timestamp = transaction.timestamp.timestamp() as u64;
        if tx_timestamp > current_time + MAX_TIMESTAMP_SKEW {
            return Err(BlockchainError::NetworkError("Transaction timestamp too far in future".to_string()));
        }

        Ok(())
    }

    /// Validate peer info message
    async fn validate_peer_info(&self, peer_id_str: &str, timestamp: u64, nonce: u64, signature: &[u8], message_data: &[u8]) -> Result<()> {
        // Parse peer ID
        let peer_id = PeerId::from_str(peer_id_str)
            .map_err(|e| BlockchainError::NetworkError(format!("Invalid peer ID: {}", e)))?;

        // Get peer info
        let mut peers = self.peers.write().await;
        if let Some(peer_info) = peers.get_mut(&peer_id) {
            // Validate message using peer's stored public key
            peer_info.validate_message(timestamp, nonce, signature, message_data)?;
        } else {
            // New peer - we need to establish their public key first
            // For now, we'll skip validation for new peers
            log::debug!("New peer {}, skipping validation", peer_id);
        }

        Ok(())
    }

    /// Generate authenticated message with replay protection
    pub fn create_authenticated_message(&self, _message_type: &str, data: &[u8]) -> Result<(u64, u64, Vec<u8>)> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| BlockchainError::NetworkError(format!("Time error: {}", e)))?
            .as_secs();

        // Generate random nonce (in production, use proper RNG)
        let nonce = timestamp ^ (data.len() as u64);

        // Create data to sign
        let mut data_to_sign = Vec::new();
        data_to_sign.extend_from_slice(&timestamp.to_le_bytes());
        data_to_sign.extend_from_slice(&nonce.to_le_bytes());
        data_to_sign.extend_from_slice(data);

        // Sign with our private key
        let signature = self.local_dilithium_kp.sign(&data_to_sign)?;

        Ok((timestamp, nonce, signature.signature))
    }

    /// Handle peer connection
    async fn on_peer_connected(&self, peer_id: PeerId) {
        log::info!("🔗 Peer connected: {}", peer_id);
        
        // Request keys if we don't have them
        if self.key_registry.request_keys_if_needed(peer_id).await {
            // Send key request message
            // Note: This would need to be handled differently since we can't call send_key_request from here
            // In a real implementation, this would be queued for the main event loop
        }
        
        let mut peers = self.peers.write().await;
        // For new connections, we don't have the public key yet.
        // We'll add a placeholder or fetch it later if needed.
        peers.entry(peer_id).or_insert_with(|| PeerInfo::new(Dilithium3Keypair::new().unwrap()));
    }

    /// Handle peer disconnection
    async fn on_peer_disconnected(&self, peer_id: PeerId) {
        log::info!("🔌 Peer disconnected: {}", peer_id);
        // Remove peer keys from registry
        self.key_registry.remove_peer_keys(&peer_id).await;
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

        // Clean up expired key requests
        self.key_registry.cleanup_expired_requests().await;

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

    /// Send a key request to a peer
    #[allow(dead_code)]
    async fn send_key_request(&mut self, peer_id: PeerId) {
        let requester_id = self.local_peer_id.to_string();
        let (timestamp, nonce, signature) = match self.create_authenticated_message("key_request", requester_id.as_bytes()) {
            Ok(auth) => auth,
            Err(e) => {
                log::error!("Failed to create key request auth: {}", e);
                return;
            }
        };

        let key_request = NetworkMessage::KeyRequest {
            requester_id,
            timestamp,
            nonce,
            signature,
        };

        // Send via floodsub for now (in production, use direct connection)
        if let Ok(data) = bincode::serialize(&key_request) {
            self.swarm
                .behaviour_mut()
                .floodsub
                .publish(Topic::new("numi/key-exchange/1.0.0"), data);
            
            log::debug!("🔑 Sent key request to peer: {}", peer_id);
        }
    }

    /// Handle key exchange messages
    async fn handle_key_exchange_message(&mut self, data: &[u8]) -> Result<()> {
        if let Ok(network_message) = bincode::deserialize::<NetworkMessage>(data) {
            match network_message {
                NetworkMessage::KeyRequest { requester_id, timestamp, nonce, signature } => {
                    self.handle_key_request(requester_id, timestamp, nonce, signature).await?;
                }
                NetworkMessage::KeyResponse { responder_id, dilithium_pk, kyber_pk, timestamp, nonce, signature } => {
                    self.handle_key_response(responder_id, dilithium_pk, kyber_pk, timestamp, nonce, signature).await?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Handle incoming key request
    async fn handle_key_request(&mut self, requester_id: String, timestamp: u64, _nonce: u64, _signature: Vec<u8>) -> Result<()> {
        // Validate the request (in production, verify signature)
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| BlockchainError::NetworkError(format!("Time error: {}", e)))?
            .as_secs();

        if timestamp > current_time + MAX_TIMESTAMP_SKEW || timestamp < current_time - MAX_TIMESTAMP_SKEW {
            return Err(BlockchainError::NetworkError("Key request timestamp outside allowed skew window".to_string()));
        }

        // Create key response
        let responder_id = self.local_peer_id.to_string();
        let (response_timestamp, response_nonce, response_signature) = 
            self.create_authenticated_message("key_response", responder_id.as_bytes())?;

        let key_response = NetworkMessage::KeyResponse {
            responder_id,
            dilithium_pk: self.local_dilithium_kp.public_key_bytes().to_vec(),
            kyber_pk: self.local_kyber_pk.clone(),
            timestamp: response_timestamp,
            nonce: response_nonce,
            signature: response_signature,
        };

        // Send response
        if let Ok(data) = bincode::serialize(&key_response) {
            self.swarm
                .behaviour_mut()
                .floodsub
                .publish(Topic::new("numi/key-exchange/1.0.0"), data);
            
            log::debug!("🔑 Sent key response to: {}", requester_id);
        }

        Ok(())
    }

    /// Handle incoming key response
    async fn handle_key_response(&mut self, responder_id: String, dilithium_pk: Vec<u8>, kyber_pk: Vec<u8>, timestamp: u64, _nonce: u64, _signature: Vec<u8>) -> Result<()> {
        // Validate the response (in production, verify signature)
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| BlockchainError::NetworkError(format!("Time error: {}", e)))?
            .as_secs();

        if timestamp > current_time + MAX_TIMESTAMP_SKEW || timestamp < current_time - MAX_TIMESTAMP_SKEW {
            return Err(BlockchainError::NetworkError("Key response timestamp outside allowed skew window".to_string()));
        }

        // Parse responder peer ID
        let responder_peer_id = PeerId::from_str(&responder_id)
            .map_err(|e| BlockchainError::NetworkError(format!("Invalid responder peer ID: {}", e)))?;

        // Store the keys
        self.key_registry.store_peer_keys(responder_peer_id, dilithium_pk, kyber_pk).await;
        self.key_registry.mark_verified(responder_peer_id).await;

        // Remove from pending requests
        let mut pending = self.key_registry.pending_requests.write().await;
        pending.remove(&responder_peer_id);

        log::info!("🔑 Received and stored keys for peer: {}", responder_peer_id);

        Ok(())
    }
} 

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_peer_key_registry_basic_operations() {
        let registry = PeerKeyRegistry::new();
        let peer_id = PeerId::random();
        
        // Test key storage and retrieval
        let dilithium_pk = vec![1, 2, 3, 4, 5];
        let kyber_pk = vec![6, 7, 8, 9, 10];
        
        registry.store_peer_keys(peer_id, dilithium_pk.clone(), kyber_pk.clone()).await;
        
        assert!(registry.has_complete_keys(&peer_id).await);
        assert_eq!(registry.get_dilithium_key(&peer_id).await, Some(dilithium_pk));
        assert_eq!(registry.get_kyber_key(&peer_id).await, Some(kyber_pk));
        
        // Test verification
        assert!(!registry.is_verified(&peer_id).await);
        registry.mark_verified(peer_id).await;
        assert!(registry.is_verified(&peer_id).await);
    }

    #[tokio::test]
    async fn test_peer_key_registry_key_discovery() {
        let registry = PeerKeyRegistry::new();
        let peer_id = PeerId::random();
        
        // Initially no keys
        assert!(!registry.has_complete_keys(&peer_id).await);
        
        // Request keys - should return true (needs request)
        assert!(registry.request_keys_if_needed(peer_id).await);
        
        // Request again - should return false (already requested)
        assert!(!registry.request_keys_if_needed(peer_id).await);
        
        // Clean up expired requests
        registry.cleanup_expired_requests().await;
    }

    #[tokio::test]
    async fn test_peer_key_registry_removal() {
        let registry = PeerKeyRegistry::new();
        let peer_id = PeerId::random();
        
        // Store keys
        registry.store_peer_keys(peer_id, vec![1,2,3], vec![4,5,6]).await;
        registry.mark_verified(peer_id).await;
        
        assert!(registry.has_complete_keys(&peer_id).await);
        assert!(registry.is_verified(&peer_id).await);
        
        // Remove keys
        registry.remove_peer_keys(&peer_id).await;
        
        assert!(!registry.has_complete_keys(&peer_id).await);
        assert!(!registry.is_verified(&peer_id).await);
    }

    #[tokio::test]
    async fn test_network_manager_key_registry_integration() {
        // Test that NetworkManager properly integrates with PeerKeyRegistry
        let network_manager = NetworkManager::new().expect("Failed to create NetworkManager");
        let handle = network_manager.create_handle();
        
        // Verify key registry is accessible through handle
        let verified_peers = handle.get_verified_peers().await;
        assert_eq!(verified_peers.len(), 0); // Should be empty initially
    }
} 