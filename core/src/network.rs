// src/network.rs
//
// Minimal P2P layer for Numicoin.
// --------------------------------------------------------------
// • libp2p TCP → Noise XX → Yamux transport
// • gossipsub v1.1 for blocks & transactions
// • mDNS for LAN discovery, static bootstrap list for WAN
// • NetworkHandle lets RPC layer broadcast tx/block & query peer count
//

use std::{collections::HashSet, sync::Arc};

use futures::{StreamExt, channel::mpsc};
use libp2p::{
    core::upgrade,
    gossipsub::{
        Behaviour as Gossipsub, Event as GossipsubEvent, IdentTopic, Config as GossipsubConfig, 
        MessageAuthenticity
    },
    identity,
    mdns::{tokio::Behaviour as Mdns, Event as MdnsEvent},
    noise,
    swarm::{NetworkBehaviour, Swarm, SwarmEvent},
    tcp, yamux, Multiaddr, PeerId, Transport,
};
use crate::RwLock;

use crate::{
    block::Block,
    transaction::Transaction,
    config::NetworkConfig,
    error::BlockchainError,
    Result,
};

// Events that go FROM network manager TO other parts of the app (inbound)
#[derive(Debug, Clone)]
pub enum InEvent {
    Block(Block),
    Tx(Transaction),
}

// Events that go FROM other parts TO network manager (outbound)
#[derive(Debug)]
pub enum OutEvent {
    BroadcastBlock(Block),
    BroadcastTx(Transaction),
}

// ---------- Behaviour  ---------------------------------------
#[derive(NetworkBehaviour)]
#[behaviour(to_swarm = "NetEvent")]
struct NetBehaviour {
    mdns: Mdns,
    gossipsub: Gossipsub,
}

#[derive(Debug)]
enum NetEvent {
    Mdns(MdnsEvent),
    Gossipsub(GossipsubEvent),
}

impl From<MdnsEvent> for NetEvent {
    fn from(event: MdnsEvent) -> Self {
        NetEvent::Mdns(event)
    }
}

impl From<GossipsubEvent> for NetEvent {
    fn from(event: GossipsubEvent) -> Self {
        NetEvent::Gossipsub(event)
    }
}

// ---------- Public handle (for RPC / miner) ------------------
#[derive(Clone)]
pub struct NetworkHandle {
    out_tx: mpsc::UnboundedSender<OutEvent>,
    peer_set: Arc<RwLock<HashSet<PeerId>>>,
}

impl NetworkHandle {
    pub fn peer_count(&self) -> usize {
        self.peer_set.read().len()
    }
    pub fn broadcast_block(&self, b: Block) -> Result<()> {
        self.out_tx.unbounded_send(OutEvent::BroadcastBlock(b))
            .map_err(|e| BlockchainError::NetworkError(format!("Send error: {e}")))
    }
    pub fn broadcast_tx(&self, t: Transaction) -> Result<()> {
        self.out_tx.unbounded_send(OutEvent::BroadcastTx(t))
            .map_err(|e| BlockchainError::NetworkError(format!("Send error: {e}")))
    }
}

// ---------- NetworkManager -----------------------------------
pub struct NetworkManager {
    swarm:        Swarm<NetBehaviour>,
    _in_tx:       mpsc::UnboundedSender<InEvent>,
    out_rx:       mpsc::UnboundedReceiver<OutEvent>,
    peer_set:     Arc<RwLock<HashSet<PeerId>>>,
    topic_blocks: IdentTopic,
    topic_txs:    IdentTopic,
}

impl NetworkManager {
    pub fn new(
        cfg: &NetworkConfig,
        in_tx: mpsc::UnboundedSender<InEvent>,
    ) -> Result<(Self, NetworkHandle)> {
        // --- keys & peer id ---
        let id_keys = identity::Keypair::generate_ed25519();
        let peer_id = PeerId::from(id_keys.public());
        log::info!("🕸  Local peer id {peer_id}");

        // --- transport: TCP → Noise XX → Yamux ---
        let transport = tcp::tokio::Transport::new(tcp::Config::default())
            .upgrade(upgrade::Version::V1)
            .authenticate(noise::Config::new(&id_keys).unwrap())
            .multiplex(yamux::Config::default())
            .boxed();

        // --- gossipsub config ---
        let gossipsub_config = GossipsubConfig::default();

        // --- gossipsub ---
        let mut gossipsub = Gossipsub::new(
            MessageAuthenticity::Signed(id_keys.clone()),
            gossipsub_config,
        ).map_err(|e| BlockchainError::NetworkError(format!("Gossipsub init: {e}")))?;

        let topic_blocks = IdentTopic::new("numicoin-blocks");
        let topic_txs = IdentTopic::new("numicoin-txs");
        
        gossipsub.subscribe(&topic_blocks)
            .map_err(|e| BlockchainError::NetworkError(format!("Subscribe blocks: {e}")))?;
        gossipsub.subscribe(&topic_txs)
            .map_err(|e| BlockchainError::NetworkError(format!("Subscribe txs: {e}")))?;

        // --- mdns ---
        let mdns = Mdns::new(Default::default(), peer_id)?;

        // --- behaviour / swarm ---
        let behaviour = NetBehaviour { mdns, gossipsub };
        let mut swarm = Swarm::new(
            transport, 
            behaviour, 
            peer_id, 
            libp2p::swarm::Config::with_tokio_executor()
        );

        // listen
        let listen_addr = format!("/ip4/{}/tcp/{}", cfg.listen_address, cfg.listen_port);
        let multiaddr = listen_addr.parse()
            .map_err(|e| BlockchainError::NetworkError(format!("Parse addr: {e}")))?;
        swarm.listen_on(multiaddr)
            .map_err(|e| BlockchainError::NetworkError(format!("Listen error: {e}")))?;

        // outbound channel
        let (out_tx, out_rx) = mpsc::unbounded();

        let peer_set = Arc::new(RwLock::new(HashSet::new()));

        let handle = NetworkHandle {
            out_tx,
            peer_set: peer_set.clone(),
        };

        Ok((
            Self {
                swarm,
                _in_tx: in_tx,
                out_rx,
                peer_set,
                topic_blocks,
                topic_txs,
            },
            handle,
        ))
    }

    /// bootstrap to the static list defined in config
    pub fn bootstrap(&mut self, list: &[Multiaddr]) {
        for addr in list {
            if let Err(e) = self.swarm.dial(addr.clone()) {
                log::warn!("Dial {addr} failed: {e}");
            }
        }
    }

    /// Run forever. Send inbound events to `in_tx`.
    pub async fn run(mut self) {
        loop {
            futures::select! {
                swarm_event = self.swarm.select_next_some() => {
                    match swarm_event {
                        SwarmEvent::Behaviour(NetEvent::Mdns(ev)) => match ev {
                            MdnsEvent::Discovered(list) => {
                                for (p, _addr) in list { 
                                    self.peer_set.write().insert(p);
                                    self.swarm.behaviour_mut().gossipsub.add_explicit_peer(&p);
                                }
                            }
                            MdnsEvent::Expired(list) => {
                                for (p, _addr) in list { 
                                    self.peer_set.write().remove(&p);
                                    self.swarm.behaviour_mut().gossipsub.remove_explicit_peer(&p);
                                }
                            }
                        },
                        SwarmEvent::Behaviour(NetEvent::Gossipsub(ev)) => match ev {
                            GossipsubEvent::Message { 
                                propagation_source: _,
                                message_id: _,
                                message,
                            } => {
                                if message.topic == self.topic_blocks.hash() {
                                    if let Ok(b) = bincode::deserialize::<Block>(&message.data) {
                                        let _ = self._in_tx.unbounded_send(InEvent::Block(b));
                                    }
                                } else if message.topic == self.topic_txs.hash() {
                                    if let Ok(tx) = bincode::deserialize::<Transaction>(&message.data) {
                                        let _ = self._in_tx.unbounded_send(InEvent::Tx(tx));
                                    }
                                }
                            }
                            GossipsubEvent::Subscribed { peer_id, topic: _ } => {
                                self.peer_set.write().insert(peer_id);
                            }
                            GossipsubEvent::Unsubscribed { peer_id, topic: _ } => {
                                self.peer_set.write().remove(&peer_id);
                            }
                            _ => {}
                        },
                        SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                            self.peer_set.write().insert(peer_id);
                        }
                        SwarmEvent::ConnectionClosed { peer_id, .. } => {
                            self.peer_set.write().remove(&peer_id);
                        }
                        _ => {}
                    }
                },
                out = self.out_rx.next() => {
                    match out {
                        Some(out_event) => {
                            match out_event {
                                OutEvent::BroadcastBlock(b) => {
                                    if let Ok(bytes) = bincode::serialize(&b) {
                                        let _ = self.swarm.behaviour_mut().gossipsub.publish(
                                            self.topic_blocks.clone(),
                                            bytes,
                                        );
                                    }
                                }
                                OutEvent::BroadcastTx(t) => {
                                    if let Ok(bytes) = bincode::serialize(&t) {
                                        let _ = self.swarm.behaviour_mut().gossipsub.publish(
                                            self.topic_txs.clone(),
                                            bytes,
                                        );
                                    }
                                }
                            }
                        },
                        None => {
                            // Channel closed, exit
                            break;
                        }
                    }
                }
            }
        }
    }
}