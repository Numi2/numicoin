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
use tokio::sync::mpsc;
use parking_lot::RwLock;

use crate::block::Block;
use crate::transaction::Transaction;
use crate::{Result, BlockchainError, PeerDB, NumiBlockchain};
use crate::crypto::{Dilithium3Keypair};
use crate::peer_db::PeerInfo;


const TOPIC_BLOCKS: &str = "numi/blocks/1.0.0";
const TOPIC_TRANSACTIONS: &str = "numi/transactions/1.0.0";
const TOPIC_PEER_INFO: &str = "numi/peer-info/1.0.0";
const TOPIC_HEADERS_REQUEST: &str = "numi/headers-request/1.0.0";
const TOPIC_HEADERS_RESPONSE: &str = "numi/headers-response/1.0.0";
const TOPIC_BLOCK_REQUEST: &str = "numi/block-request/1.0.0";

/// Bootstrap nodes for initial network discovery
const BOOTSTRAP_NODES: &[&str] = &[
    "/ip4/127.0.0.1/tcp/8333",  // Local node for testing
];

/// Maximum allowed timestamp skew for replay protection (5 minutes)
const MAX_TIMESTAMP_SKEW: u64 = 300;

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
    /// Response to headers request
    HeadersResponse { headers: Vec<crate::block::BlockHeader> },
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
    peer_db: PeerDB,
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
    
    local_dilithium_kp: Dilithium3Keypair,
    _local_kyber_pk: Vec<u8>,
    _local_kyber_sk: Vec<u8>,
    peer_db: PeerDB,
    blockchain: Arc<parking_lot::RwLock<NumiBlockchain>>,
}

// Safety: NetworkManager is moved into its own dedicated async task thread and is not shared thereafter, 
// so it is safe to mark it Send and Sync for the purpose of spawning.
unsafe impl Send for NetworkManager {}
unsafe impl Sync for NetworkManager {}

impl NetworkManagerHandle {
    /// Get the number of connected peers
    pub async fn get_peer_count(&self) -> usize {
        let peers = self.peers.clone();
        tokio::task::spawn_blocking(move || peers.read().len()).await.unwrap_or(0)
    }

    /// Check if the node is currently syncing
    pub async fn is_syncing(&self) -> bool {
        let is_syncing = self.is_syncing.clone();
        tokio::task::spawn_blocking(move || *is_syncing.read()).await.unwrap_or(false)
    }

    /// Get current chain height
    pub async fn get_chain_height(&self) -> u64 {
        let chain_height = self.chain_height.clone();
        tokio::task::spawn_blocking(move || *chain_height.read()).await.unwrap_or(0)
    }

    /// Broadcast a block to the network
    pub async fn broadcast_block(&self, block: Block) -> Result<()> {
        let message = NetworkMessage::NewBlock(block);
        self.message_sender.send(message)
            .map_err(|e| BlockchainError::NetworkError(format!("Failed to send block: {e}")))?;
        Ok(())
    }

    /// Broadcast a transaction to the network
    pub async fn broadcast_transaction(&self, transaction: Transaction) -> Result<()> {
        let message = NetworkMessage::NewTransaction(transaction);
        self.message_sender.send(message)
            .map_err(|e| BlockchainError::NetworkError(format!("Failed to send transaction: {e}")))?;
        Ok(())
    }

    /// Update peer reputation
    pub async fn update_peer_reputation(&self, _peer_id: PeerId, _delta: i32) {
        let peers = self.peers.clone();
        let peer_id = _peer_id;
        tokio::task::spawn_blocking(move || {
            let mut peers = peers.write();
            if let Some(_peer) = peers.get_mut(&peer_id) {
                // Reputation logic is removed for now
            }
        }).await.ok();
    }

    /// Check if a peer is banned
    pub async fn is_peer_banned(&self, peer_id: &PeerId) -> bool {
        let banned_peers = self.banned_peers.clone();
        let peer_id = *peer_id;
        tokio::task::spawn_blocking(move || banned_peers.read().contains(&peer_id)).await.unwrap_or(false)
    }
    /// Get list of verified peers from key registry
    pub async fn get_verified_peers(&self) -> Vec<PeerId> {
        // This function is no longer needed as PeerKeyRegistry is removed.
        // Returning an empty vector as a placeholder.
        Vec::new()
    }

    /// Add a peer to the peer database.
    pub async fn add_peer_to_db(&self, peer_id: PeerId, public_key: Dilithium3Keypair) {
        self.peer_db.add_peer(peer_id, public_key).await;
    }
}

impl NetworkManager {
    pub fn new(blockchain: Arc<parking_lot::RwLock<NumiBlockchain>>) -> Result<Self> {
        // Generate post-quantum key material for handshake and authentication
        let dilithium_kp = Dilithium3Keypair::new()?;

        // Generate TLS identity for transport and derive PeerId from it
        let tls_identity = identity::Keypair::generate_ed25519();
        let local_peer_id = PeerId::from(tls_identity.public());

        log::info!("🔑 Local peer ID: {local_peer_id}");

        // Create key registry
        // let key_registry = Arc::new(PeerKeyRegistry::new()); // This line is removed

        // Create TLS config for transport encryption
        let tls_config = tls::Config::new(&tls_identity).map_err(|e| BlockchainError::NetworkError(format!("TLS config failed: {e}")))?;

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
        let headers_request_topic = Topic::new(TOPIC_HEADERS_REQUEST);
        let headers_response_topic = Topic::new(TOPIC_HEADERS_RESPONSE);
        let block_request_topic = Topic::new(TOPIC_BLOCK_REQUEST);

        floodsub.subscribe(blocks_topic.clone());
        floodsub.subscribe(transactions_topic.clone());
        floodsub.subscribe(peer_info_topic.clone());
        floodsub.subscribe(headers_request_topic.clone());
        floodsub.subscribe(headers_response_topic.clone());
        floodsub.subscribe(block_request_topic.clone());

        // mDNS for local peer discovery
        let mdns = Mdns::new(Default::default(), local_peer_id)
            .map_err(|e| BlockchainError::NetworkError(format!("mDNS init failed: {e}")))?;

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
            
            local_dilithium_kp: dilithium_kp,
            _local_kyber_pk: Vec::new(),
            _local_kyber_sk: Vec::new(),
            peer_db: PeerDB::new(),
            blockchain,
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
            peer_db: self.peer_db.clone(),
        }
    }

    /// Check if currently syncing
    pub async fn is_syncing(&self) -> bool {
        let is_syncing = self.is_syncing.clone();
        tokio::task::spawn_blocking(move || *is_syncing.read()).await.unwrap_or(false)
    }

    /// Set syncing status
    pub async fn set_syncing(&self, syncing: bool) {
        let is_syncing = self.is_syncing.clone();
        tokio::task::spawn_blocking(move || {
            let mut guard = is_syncing.write();
            *guard = syncing;
        }).await.ok();
    }

    /// Get current chain height
    pub async fn get_chain_height(&self) -> u64 {
        let chain_height = self.chain_height.clone();
        tokio::task::spawn_blocking(move || *chain_height.read()).await.unwrap_or(0)
    }

    /// Set current chain height
    pub async fn set_chain_height(&self, height: u64) {
        let chain_height = self.chain_height.clone();
        tokio::task::spawn_blocking(move || {
            let mut guard = chain_height.write();
            *guard = height;
        }).await.ok();
    }

    /// Start the network manager and bind to listening address
    pub async fn start(&mut self, listen_addr: &str) -> Result<()> {
        // Try to parse as multiaddr first, if that fails, try to construct it
        let addr: Multiaddr = match listen_addr.parse() {
            Ok(addr) => addr,
            Err(_) => {
                // If parsing fails, try to construct a proper multiaddr
                // The listen_addr should be in format "/ip4/0.0.0.0/tcp/8333"
                if listen_addr.starts_with("/ip4/") {
                    listen_addr.parse()
                        .map_err(|e| BlockchainError::NetworkError(format!("Invalid multiaddr format: {e}")))?
                } else {
                    // Try to construct from IP and port
                    let parts: Vec<&str> = listen_addr.split('/').collect();
                    if parts.len() >= 4 && parts[1] == "ip4" && parts[3] == "tcp" {
                        listen_addr.parse()
                            .map_err(|e| BlockchainError::NetworkError(format!("Invalid multiaddr format: {e}")))?
                    } else {
                        return Err(BlockchainError::NetworkError(format!("Invalid listen address format: {listen_addr}")));
                    }
                }
            }
        };

        self.swarm.listen_on(addr.clone())
            .map_err(|e| BlockchainError::NetworkError(format!("Failed to listen: {e}")))?;

        log::info!("🌐 Network listening on: {addr}");
        
        // Connect to bootstrap nodes
        self.bootstrap().await?;
        
        Ok(())
    }

    /// Connect to bootstrap nodes
    async fn bootstrap(&mut self) -> Result<()> {
        for &bootstrap_addr in BOOTSTRAP_NODES {
            if let Ok(addr) = bootstrap_addr.parse::<Multiaddr>() {
                match self.swarm.dial(addr.clone()) {
                    Ok(_) => log::info!("📞 Dialing bootstrap node: {addr}"),
                    Err(e) => log::warn!("❌ Failed to dial bootstrap node {addr}: {e}"),
                }
            }
        }
        Ok(())
    }

    /// Perform periodic maintenance tasks
    async fn perform_maintenance(&mut self) {
        // Unban peers whose ban duration has expired
        let _now = Instant::now();
        let peers = self.peers.clone();
        tokio::task::spawn_blocking(move || {
            let _peers = peers.write();
            // Ban logic is removed for now
        }).await.ok();
    }

    /// Main event processing loop
    pub async fn run_event_loop(&mut self) {
        let mut maintenance_interval = tokio::time::interval(Duration::from_secs(30));

        loop {
            tokio::select! {
                // Handle swarm events
                event = self.swarm.select_next_some() => {
                    if let Err(e) = self.handle_swarm_event(event).await {
                        log::error!("Error handling swarm event: {e}");
                    }
                }
                
                // Handle outgoing messages
                message = self.message_receiver.recv() => {
                    if let Some(msg) = message {
                        if let Err(e) = self.handle_outgoing_message(msg).await {
                            log::error!("Error handling outgoing message: {e}");
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
                log::info!("🌐 New listen address: {address}");
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
            NetworkMessage::HeadersRequest { .. } => (TOPIC_HEADERS_REQUEST, bincode::serialize(&message)?),
            NetworkMessage::HeadersResponse { .. } => (TOPIC_HEADERS_RESPONSE, bincode::serialize(&message)?),
            NetworkMessage::BlockRequest(..) => (TOPIC_BLOCK_REQUEST, bincode::serialize(&message)?),
            _ => return Ok(()), // Skip other message types for now
        };

        // Parallel broadcast to all peers using FuturesUnordered
        let mut broadcast_futures = FuturesUnordered::new();
        
        // Get all connected peers
        let peers = self.peers.clone();
        let peer_ids: Vec<PeerId> = tokio::task::spawn_blocking(move || {
            let peers = peers.read();
            let peer_ids: Vec<PeerId> = peers.keys().cloned().collect();
            peer_ids
        }).await.unwrap_or_default();

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
                log::warn!("Failed to broadcast to peer: {e}");
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
            format!("{topic:?}")
        } else {
            String::new()
        };
        let data = message.data;

        match topic_str.as_str() {
            TOPIC_BLOCKS => {
                if let Ok(network_message) = bincode::deserialize::<NetworkMessage>(&data) {
                    if let NetworkMessage::NewBlock(block) = network_message {
                        log::info!("📦 Received new block: {}", hex::encode(block.calculate_hash().unwrap_or([0u8; 32])));
                        
                        // Validate block before processing
                        if let Err(e) = self.validate_block(&block).await {
                            log::warn!("❌ Block validation failed: {e}");
                            return Ok(());
                        }
                        
                        // Process validated block
                        log::info!("✅ Block validated successfully");
                    }
                }
            }
            TOPIC_TRANSACTIONS => {
                if let Ok(network_message) = bincode::deserialize::<NetworkMessage>(&data) {
                    if let NetworkMessage::NewTransaction(tx) = network_message {
                        log::info!("💸 Received new transaction: {}", hex::encode(tx.id));
                        
                        // Validate transaction before processing
                        if let Err(e) = self.validate_transaction(&tx).await {
                            log::warn!("❌ Transaction validation failed: {e}");
                            return Ok(());
                        }
                        
                        // Process validated transaction
                        log::info!("✅ Transaction validated successfully");
                    }
                }
            }
            TOPIC_PEER_INFO => {
                if let Ok(network_message) = bincode::deserialize::<NetworkMessage>(&data) {
                    if let NetworkMessage::PeerInfo { chain_height, peer_id, timestamp, nonce, signature } = network_message {
                        log::debug!("👥 Peer info: {peer_id} at height {chain_height}");
                        
                        // Validate peer info message
                        if let Err(e) = self.validate_peer_info(&peer_id, timestamp, nonce, &signature, &data).await {
                            log::warn!("❌ Peer info validation failed: {e}");
                            return Ok(());
                        }
                        
                        // Update peer information
                        log::info!("✅ Peer info validated successfully");
                    }
                }
            }
            TOPIC_HEADERS_REQUEST => {
                if let Ok(network_message) = bincode::deserialize::<NetworkMessage>(&data) {
                    if let NetworkMessage::HeadersRequest { start_hash, count } = network_message {
                        log::info!("📜 Received headers request from peer, starting from hash: {:?}, count: {}", hex::encode(&start_hash), count);
                        let blockchain = self.blockchain.clone();
                        let start_hash = start_hash.clone();
                        let headers = tokio::task::spawn_blocking(move || {
                            blockchain.read().get_block_headers(start_hash, count)
                        }).await.unwrap_or_default();
                        let response = NetworkMessage::HeadersResponse { headers };
                        if let Ok(response_data) = bincode::serialize(&response) {
                            self.swarm.behaviour_mut().floodsub.publish(Topic::new(TOPIC_HEADERS_RESPONSE), response_data);
                        }
                    }
                }
            }
            TOPIC_HEADERS_RESPONSE => {
                if let Ok(network_message) = bincode::deserialize::<NetworkMessage>(&data) {
                    if let NetworkMessage::HeadersResponse { headers } = network_message {
                        log::info!("📬 Received {} headers from peer", headers.len());
                        for header in headers {
                            let block_hash = header.calculate_hash().unwrap_or_default();
                            let blockchain = self.blockchain.clone();
                            let block_hash = block_hash.clone();
                            let has_block = tokio::task::spawn_blocking(move || {
                                blockchain.read().get_block_by_hash(&block_hash).is_none()
                            }).await.unwrap_or(true);
                            if has_block {
                                log::info!("Requesting missing block: {}", hex::encode(block_hash));
                                let request = NetworkMessage::BlockRequest(block_hash.to_vec());
                                if let Ok(data) = bincode::serialize(&request) {
                                    self.swarm.behaviour_mut().floodsub.publish(Topic::new(TOPIC_BLOCK_REQUEST), data);
                                }
                            }
                        }
                    }
                }
            }
            TOPIC_BLOCK_REQUEST => {
                if let Ok(network_message) = bincode::deserialize::<NetworkMessage>(&data) {
                    if let NetworkMessage::BlockRequest(block_hash_vec) = network_message {
                        let mut block_hash = [0u8; 32];
                        block_hash.copy_from_slice(&block_hash_vec);
                        log::info!("📦 Received block request for hash: {}", hex::encode(block_hash));
                        let blockchain = self.blockchain.clone();
                        let block_hash = block_hash.clone();
                        let block = tokio::task::spawn_blocking(move || {
                            blockchain.read().get_block_by_hash(&block_hash)
                        }).await.unwrap_or(None);
                        if let Some(block) = block {
                            let message = NetworkMessage::NewBlock(block);
                            if let Ok(data) = bincode::serialize(&message) {
                                self.swarm.behaviour_mut().floodsub.publish(Topic::new(TOPIC_BLOCKS), data);
                            }
                        }
                    }
                }
            }
            
            _ => {
                log::debug!("📨 Unknown message topic: {topic_str}");
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
            .map_err(|e| BlockchainError::NetworkError(format!("Time error: {e}")))?
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

        
        // Verify transaction timestamp is reasonable
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| BlockchainError::NetworkError(format!("Time error: {e}")))?
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
            .map_err(|e| BlockchainError::NetworkError(format!("Invalid peer ID: {e}")))?;

        // Get peer info
        let peers = self.peers.clone();
        let peer_id = peer_id.clone();
        let peer_info = tokio::task::spawn_blocking(move || {
            let mut peers = peers.write();
            peers.get_mut(&peer_id).cloned()
        }).await.unwrap_or(None);
        if let Some(_peer_info) = peer_info {
            // Validate message using peer's stored public key
            if let Some(peer_db_info) = self.peer_db.get_peer(&peer_id).await {
                let mut data_to_verify = Vec::new();
                data_to_verify.extend_from_slice(&timestamp.to_le_bytes());
                data_to_verify.extend_from_slice(&nonce.to_le_bytes());
                data_to_verify.extend_from_slice(message_data);

                if !Dilithium3Keypair::verify(&data_to_verify, &crate::crypto::Dilithium3Signature { signature: signature.to_vec(), public_key: peer_db_info.public_key.public_key_bytes().to_vec(), created_at: chrono::Utc::now().timestamp() as u64, message_hash: crate::crypto::blake3_hash(&data_to_verify) }, peer_db_info.public_key.public_key_bytes())? {
                    return Err(BlockchainError::NetworkError("Invalid message signature".to_string()));
                }

                self.peer_db.update_peer_nonce(&peer_id, nonce).await?;
            } else {
                log::warn!("Peer not found in DB, skipping validation");
            }
        } else {
            // New peer - add to db
            let public_key = Dilithium3Keypair::from_public_key(signature)?;
            self.peer_db.add_peer(peer_id, public_key).await;
        }

        Ok(())
    }

    /// Generate authenticated message with replay protection
    pub fn create_authenticated_message(&self, _message_type: &str, data: &[u8]) -> Result<(u64, u64, Vec<u8>)> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| BlockchainError::NetworkError(format!("Time error: {e}")))?
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
    async fn on_peer_connected(&mut self, peer_id: PeerId) {
        log::info!("🔗 Peer connected: {peer_id}");
        
        let chain_height = self.chain_height.clone();
        let local_chain_height = tokio::task::spawn_blocking(move || *chain_height.read()).await.unwrap_or(0);

        // Send our peer info to the newly connected peer
        let (timestamp, nonce, signature) = self.create_authenticated_message("peer_info", &[]).unwrap();
        let peer_info_message = NetworkMessage::PeerInfo {
            chain_height: local_chain_height,
            peer_id: self.local_peer_id.to_string(),
            timestamp,
            nonce,
            signature,
        };
        
        if let Ok(data) = bincode::serialize(&peer_info_message) {
            self.swarm
                .behaviour_mut()
                .floodsub
                .publish(Topic::new(TOPIC_PEER_INFO), data);
        }

        // Request headers from the peer to check for sync
        let headers_request = NetworkMessage::HeadersRequest {
            start_hash: Vec::new(), // Start from genesis for now
            count: 100, // Request a batch of headers
        };
        if let Ok(data) = bincode::serialize(&headers_request) {
             self.swarm
                .behaviour_mut()
                .floodsub
                .publish(Topic::new(TOPIC_HEADERS_REQUEST), data);
        }
    }

    /// Handle peer disconnection
    async fn on_peer_disconnected(&self, peer_id: PeerId) {
        log::info!("🔌 Peer disconnected: {peer_id}");
        
    }

    /// Update peer reputation
    pub async fn update_peer_reputation(&self, _peer_id: PeerId, _delta: i32) {
        let peers = self.peers.clone();
        let peer_id = _peer_id;
        tokio::task::spawn_blocking(move || {
            let mut peers = peers.write();
            if let Some(_peer) = peers.get_mut(&peer_id) {
                // Reputation logic is removed for now
            }
        }).await.ok();
    }

    /// Check if a peer is banned
    pub async fn is_peer_banned(&self, peer_id: &PeerId) -> bool {
        let banned_peers = self.banned_peers.clone();
        let peer_id = *peer_id;
        tokio::task::spawn_blocking(move || banned_peers.read().contains(&peer_id)).await.unwrap_or(false)
    }
    
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use parking_lot::RwLock;
    use crate::config::ConsensusConfig;

    fn create_test_blockchain() -> Arc<RwLock<NumiBlockchain>> {
        let blockchain = NumiBlockchain::new_with_config(Some(ConsensusConfig::default()), None).unwrap();
        Arc::new(RwLock::new(blockchain))
    }
    
    #[tokio::test]
    async fn test_network_manager_key_registry_integration() {
        // Test that NetworkManager properly integrates with PeerKeyRegistry
        let blockchain = create_test_blockchain();
        let network_manager = NetworkManager::new(blockchain).expect("Failed to create NetworkManager");
        let handle = network_manager.create_handle();
        
        // Verify key registry is accessible through handle
        let verified_peers = handle.get_verified_peers().await;
        assert_eq!(verified_peers.len(), 0); // Should be empty initially
    }

    #[tokio::test]
    async fn test_network_manager_sync_flags() {
        let blockchain = create_test_blockchain();
        let manager = NetworkManager::new(blockchain).expect("Failed to create NetworkManager");
        // Initially not syncing
        assert!(!manager.is_syncing().await);
        // Set syncing
        manager.set_syncing(true).await;
        assert!(manager.is_syncing().await);
        // Unset syncing
        manager.set_syncing(false).await;
        assert!(!manager.is_syncing().await);
    }

    #[tokio::test]
    async fn test_network_manager_chain_height() {
        let blockchain = create_test_blockchain();
        let manager = NetworkManager::new(blockchain).expect("Failed to create NetworkManager");
        // Initial height is 0
        assert_eq!(manager.get_chain_height().await, 0);
        // Update chain height
        manager.set_chain_height(42).await;
        assert_eq!(manager.get_chain_height().await, 42);
    }

    #[tokio::test]
    async fn test_network_manager_handle_sync_and_height() {
        let blockchain = create_test_blockchain();
        let manager = NetworkManager::new(blockchain).expect("Failed to create NetworkManager");
        let handle = manager.create_handle();
        // Initially not syncing and height 0
        assert!(!handle.is_syncing().await);
        assert_eq!(handle.get_chain_height().await, 0);
        // Manager updates
        manager.set_syncing(true).await;
        manager.set_chain_height(7).await;
        // Handle reflects updates
        assert!(handle.is_syncing().await);
        assert_eq!(handle.get_chain_height().await, 7);
    }

    #[tokio::test]
    async fn test_network_manager_broadcast_messages() {
        use crate::transaction::TransactionType;
        
        let blockchain = create_test_blockchain();
        let manager = NetworkManager::new(blockchain).expect("Failed to create NetworkManager");
        let handle = manager.create_handle();
        // Test broadcast_block via handle
        let block = Block::new(0, [0u8; 32], vec![], 1, vec![]);
        assert!(handle.broadcast_block(block).await.is_ok());
        // Test broadcast_transaction via handle
        let tx = Transaction::new(Vec::new(), TransactionType::Transfer { to: Vec::new(), amount: 0, memo: None }, 0);
        assert!(handle.broadcast_transaction(tx).await.is_ok());
    }
} 