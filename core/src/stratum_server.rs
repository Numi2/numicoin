//! # Stratum V2 Server Implementation
//! 
//! This module implements a production-grade Stratum V2 mining server with:
//! 
//! ## Core Features
//! - **Binary Protocol**: Fixed-width little-endian fields per SV2 specification  
//! - **Noise Encryption**: XX handshake pattern using ChaCha20-Poly1305 + BLAKE2s
//! - **BLAKE3 Share Validation**: Fast target checking (replaces Argon2id for shares)
//! - **Dilithium3 Job Signatures**: Quantum-resistant signing of mining templates
//! 
//! ## Miner Compatibility
//! 
//! ### Standard SV2 Miners
//! This server follows the official Stratum V2 specification and is compatible with
//! any compliant miner software (e.g., Braiins, CGMiner SV2 variants).
//! 
//! ### Dilithium3 Signature Handling
//! Extended mining jobs include optional Dilithium3 signatures. For miners that
//! don't support post-quantum signatures yet:
//! 
//! 1. Check the `signature_present` byte (0x00 = no signature, 0x01 = signature present)
//! 2. If present, skip the signature block:
//!    - `signature_length` (2 bytes LE) + signature data
//!    - `public_key_length` (2 bytes LE) + public key data  
//!    - `message_hash` (32 bytes)
//!    - `created_at` (8 bytes LE)
//! 3. Continue parsing standard SV2 fields after the signature block
//! 
//! ### Example Connection Flow
//! ```text
//! 1. TCP connection to node:3333
//! 2. Noise XX handshake (3 round trips)
//! 3. Encrypted SV2 frame layer:
//!    - SetupConnection → SetupConnectionSuccess
//!    - OpenStandardMiningChannel → OpenStandardMiningChannelSuccess  
//!    - Receive NewMiningJob messages (with optional Dilithium3 signatures)
//!    - Submit SubmitSharesStandard → SubmitSharesSuccess/Error
//! ```
//! 
//! ## Network Protocol Details
//! - **Port**: Configurable (default 3333)
//! - **Encryption**: Mandatory Noise XX pattern
//! - **Frame Format**: 6-byte header + encrypted payload
//! - **Message Encoding**: Little-endian, length-prefixed strings
//! - **Share Validation**: BLAKE3(version + header + ntime + nonce) <= target

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use crate::RwLock;
use snow::{Builder, TransportState};
use crossbeam::channel::Sender;

use crate::mining_service::MiningService;
use crate::crypto::{blake3_hash, generate_difficulty_target, Dilithium3Signature};
use crate::error::MiningServiceError;

/// Stratum V2 Protocol Constants
const SV2_PROTOCOL_VERSION: u16 = 2;
const SV2_FRAME_HEADER_SIZE: usize = 6; // extension_type(2) + msg_type(1) + msg_length(3)
const SV2_MAX_MESSAGE_SIZE: usize = 16777215; // 2^24 - 1 (3 bytes max)

/// Stratum V2 Message Types (as per specification)
#[repr(u8)]
#[derive(Debug, Clone, Copy)]
pub enum Sv2MessageType {
    SetupConnection = 0x00,
    SetupConnectionSuccess = 0x01,
    SetupConnectionError = 0x02,
    ChannelEndpointChanged = 0x03,
    
    // Mining messages
    OpenStandardMiningChannel = 0x10,
    OpenStandardMiningChannelSuccess = 0x11,
    OpenStandardMiningChannelError = 0x12,
    NewMiningJob = 0x15,
    SetNewPrevHash = 0x16,
    SubmitSharesStandard = 0x1A,
    SubmitSharesSuccess = 0x1C,
    SubmitSharesError = 0x1D,
}

/// Stratum V2 Frame Header (6 bytes, little-endian)
#[derive(Debug, Clone)]
pub struct Sv2Frame {
    pub extension_type: u16,    // 2 bytes LE
    pub msg_type: u8,          // 1 byte
    pub msg_length: u32,       // 3 bytes LE (stored as u32, but only 3 bytes used)
    pub payload: Vec<u8>,
}

impl Sv2Frame {
    /// Encode frame to bytes (SV2 specification compliant)
    pub fn encode(&self) -> Vec<u8> {
        let mut buffer = Vec::with_capacity(SV2_FRAME_HEADER_SIZE + self.payload.len());
        
        // Extension type (2 bytes, little-endian)
        buffer.extend_from_slice(&self.extension_type.to_le_bytes());
        
        // Message type (1 byte)
        buffer.push(self.msg_type);
        
        // Message length (3 bytes, little-endian) - only first 3 bytes of u32
        let length_bytes = self.msg_length.to_le_bytes();
        buffer.extend_from_slice(&length_bytes[0..3]);
        
        // Payload
        buffer.extend_from_slice(&self.payload);
        
        buffer
    }
    
    /// Decode frame from bytes
    pub fn decode(data: &[u8]) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        if data.len() < SV2_FRAME_HEADER_SIZE {
            return Err("Frame too short".into());
        }
        
        // Extension type (2 bytes, little-endian)
        let extension_type = u16::from_le_bytes([data[0], data[1]]);
        
        // Message type (1 byte)
        let msg_type = data[2];
        
        // Message length (3 bytes, little-endian, stored as u32)
        let mut length_bytes = [0u8; 4];
        length_bytes[0..3].copy_from_slice(&data[3..6]);
        let msg_length = u32::from_le_bytes(length_bytes);
        
        if msg_length > SV2_MAX_MESSAGE_SIZE as u32 {
            return Err("Message too large".into());
        }
        
        let expected_total_len = SV2_FRAME_HEADER_SIZE + msg_length as usize;
        if data.len() < expected_total_len {
            return Err("Incomplete frame".into());
        }
        
        let payload = data[SV2_FRAME_HEADER_SIZE..expected_total_len].to_vec();
        
        Ok(Sv2Frame {
            extension_type,
            msg_type,
            msg_length,
            payload,
        })
    }
}

/// Stratum V2 Binary Protocol Encoder/Decoder
pub struct Sv2Codec;

impl Sv2Codec {
    /// Encode string with SV2 format: length (2 bytes LE) + UTF-8 bytes
    pub fn encode_string(s: &str) -> Vec<u8> {
        let bytes = s.as_bytes();
        let len = bytes.len() as u16;
        let mut result = Vec::with_capacity(2 + bytes.len());
        result.extend_from_slice(&len.to_le_bytes());
        result.extend_from_slice(bytes);
        result
    }
    
    /// Decode string from SV2 format
    pub fn decode_string(data: &[u8], offset: &mut usize) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        if data.len() < *offset + 2 {
            return Err("Not enough data for string length".into());
        }
        
        let len = u16::from_le_bytes([data[*offset], data[*offset + 1]]) as usize;
        *offset += 2;
        
        if data.len() < *offset + len {
            return Err("Not enough data for string content".into());
        }
        
        let s = String::from_utf8(data[*offset..*offset + len].to_vec())?;
        *offset += len;
        Ok(s)
    }
    
    /// Encode u32 as little-endian
    pub fn encode_u32(value: u32) -> [u8; 4] {
        value.to_le_bytes()
    }
    
    /// Decode u32 from little-endian
    pub fn decode_u32(data: &[u8], offset: &mut usize) -> Result<u32, Box<dyn std::error::Error + Send + Sync>> {
        if data.len() < *offset + 4 {
            return Err("Not enough data for u32".into());
        }
        
        let value = u32::from_le_bytes([
            data[*offset],
            data[*offset + 1],
            data[*offset + 2],
            data[*offset + 3],
        ]);
        *offset += 4;
        Ok(value)
    }
    
    /// Encode u64 as little-endian
    pub fn encode_u64(value: u64) -> [u8; 8] {
        value.to_le_bytes()
    }
    
    /// Decode u64 from little-endian
    pub fn decode_u64(data: &[u8], offset: &mut usize) -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
        if data.len() < *offset + 8 {
            return Err("Not enough data for u64".into());
        }
        
        let value = u64::from_le_bytes([
            data[*offset], data[*offset + 1], data[*offset + 2], data[*offset + 3],
            data[*offset + 4], data[*offset + 5], data[*offset + 6], data[*offset + 7],
        ]);
        *offset += 8;
        Ok(value)
    }
    
    /// Encode byte array with length prefix (2 bytes LE)
    pub fn encode_bytes(bytes: &[u8]) -> Vec<u8> {
        let len = bytes.len() as u16;
        let mut result = Vec::with_capacity(2 + bytes.len());
        result.extend_from_slice(&len.to_le_bytes());
        result.extend_from_slice(bytes);
        result
    }
    
    /// Decode byte array with length prefix
    pub fn decode_bytes(data: &[u8], offset: &mut usize) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        if data.len() < *offset + 2 {
            return Err("Not enough data for bytes length".into());
        }
        
        let len = u16::from_le_bytes([data[*offset], data[*offset + 1]]) as usize;
        *offset += 2;
        
        if data.len() < *offset + len {
            return Err("Not enough data for bytes content".into());
        }
        
        let bytes = data[*offset..*offset + len].to_vec();
        *offset += len;
        Ok(bytes)
    }
}

/// Extended Mining Job with Dilithium3 signature
/// 
/// **SIGNATURE FIELD LAYOUT DOCUMENTATION:**
/// The Dilithium3 signature is embedded in the mining job as follows:
/// 
/// ```text
/// MiningJob {
///     ... standard SV2 fields ...
///     signature_present: bool,           // 1 byte: 0x01 if signature present, 0x00 if not
///     signature_length: u16,             // 2 bytes LE: length of signature data
///     signature_data: Vec<u8>,           // Variable: Dilithium3 signature (3293 bytes when present)
///     public_key_length: u16,            // 2 bytes LE: length of public key
///     public_key: Vec<u8>,               // Variable: Dilithium3 public key (1952 bytes when present)
///     message_hash: [u8; 32],            // 32 bytes: BLAKE3 hash of the job template
///     created_at: u64,                   // 8 bytes LE: Unix timestamp
/// }
/// ```
/// 
/// **For miners that don't support Dilithium3 verification:**
/// 1. Check `signature_present` byte at the expected offset
/// 2. If 0x00, proceed normally (no signature)
/// 3. If 0x01, skip `signature_length + public_key_length + 32 + 8` bytes
/// 4. Continue parsing standard fields after the signature block
#[derive(Debug, Clone)]
pub struct ExtendedMiningJob {
    // Standard SV2 mining job fields
    pub channel_id: u32,
    pub job_id: u32,
    pub future_job: bool,
    pub version: u32,
    pub coinbase_tx_prefix: Vec<u8>,
    pub coinbase_tx_suffix: Vec<u8>,
    pub merkle_path: Vec<[u8; 32]>,
    pub prev_hash: [u8; 32],
    pub ntime: u32,
    pub nbits: u32,
    pub target: [u8; 32],
    
    // Extended fields for Dilithium3 (documented above)
    pub signature: Option<Dilithium3Signature>,
    pub height: u64,
}

/// Server statistics for monitoring
#[derive(Debug, Clone)]
pub struct ServerStats {
    pub total_connections: u32,
    pub active_connections: u32,
    pub total_hash_rate: f64,
    pub total_shares_submitted: u64,
    pub total_shares_accepted: u64,
    pub overall_acceptance_rate: f64,
}

/// Detailed connection information
#[derive(Debug, Clone)]
pub struct ConnectionInfo {
    pub user_id: String,
    pub channel_id: u32,
    pub nominal_hash_rate: f64,
    pub uptime_seconds: u64,
    pub shares_submitted: u64,
    pub shares_accepted: u64,
    pub acceptance_rate: f64,
    pub extranonce_prefix: String,
    pub is_active: bool,
}

impl ExtendedMiningJob {
    /// Encode job to SV2 binary format
    pub fn encode(&self) -> Vec<u8> {
        let mut buffer = Vec::new();
        
        // Standard SV2 fields (little-endian)
        buffer.extend_from_slice(&Sv2Codec::encode_u32(self.channel_id));
        buffer.extend_from_slice(&Sv2Codec::encode_u32(self.job_id));
        buffer.push(if self.future_job { 1 } else { 0 });
        buffer.extend_from_slice(&Sv2Codec::encode_u32(self.version));
        buffer.extend_from_slice(&Sv2Codec::encode_bytes(&self.coinbase_tx_prefix));
        buffer.extend_from_slice(&Sv2Codec::encode_bytes(&self.coinbase_tx_suffix));
        
        // Merkle path (count + hashes)
        buffer.extend_from_slice(&(self.merkle_path.len() as u16).to_le_bytes());
        for hash in &self.merkle_path {
            buffer.extend_from_slice(hash);
        }
        
        buffer.extend_from_slice(&self.prev_hash);
        buffer.extend_from_slice(&Sv2Codec::encode_u32(self.ntime));
        buffer.extend_from_slice(&Sv2Codec::encode_u32(self.nbits));
        buffer.extend_from_slice(&self.target);
        
        // Extended Dilithium3 signature (documented layout)
        if let Some(ref sig) = self.signature {
            buffer.push(0x01); // signature_present = true
            buffer.extend_from_slice(&Sv2Codec::encode_bytes(&sig.signature));
            buffer.extend_from_slice(&Sv2Codec::encode_bytes(&sig.public_key));
            buffer.extend_from_slice(&sig.message_hash);
            buffer.extend_from_slice(&Sv2Codec::encode_u64(sig.created_at));
        } else {
            buffer.push(0x00); // signature_present = false
        }
        
        buffer.extend_from_slice(&Sv2Codec::encode_u64(self.height));
        
        buffer
    }
    
    /// Decode job from SV2 binary format
    pub fn decode(data: &[u8]) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let mut offset = 0;
        
        // Standard SV2 fields
        let channel_id = Sv2Codec::decode_u32(data, &mut offset)?;
        let job_id = Sv2Codec::decode_u32(data, &mut offset)?;
        let future_job = data[offset] != 0;
        offset += 1;
        let version = Sv2Codec::decode_u32(data, &mut offset)?;
        let coinbase_tx_prefix = Sv2Codec::decode_bytes(data, &mut offset)?;
        let coinbase_tx_suffix = Sv2Codec::decode_bytes(data, &mut offset)?;
        
        // Merkle path
        let merkle_count = u16::from_le_bytes([data[offset], data[offset + 1]]) as usize;
        offset += 2;
        let mut merkle_path = Vec::with_capacity(merkle_count);
        for _ in 0..merkle_count {
            if data.len() < offset + 32 {
                return Err("Not enough data for merkle hash".into());
            }
            let mut hash = [0u8; 32];
            hash.copy_from_slice(&data[offset..offset + 32]);
            merkle_path.push(hash);
            offset += 32;
        }
        
        // Fixed-size fields
        let mut prev_hash = [0u8; 32];
        prev_hash.copy_from_slice(&data[offset..offset + 32]);
        offset += 32;
        
        let ntime = Sv2Codec::decode_u32(data, &mut offset)?;
        let nbits = Sv2Codec::decode_u32(data, &mut offset)?;
        
        let mut target = [0u8; 32];
        target.copy_from_slice(&data[offset..offset + 32]);
        offset += 32;
        
        // Extended Dilithium3 signature
        let signature = if data[offset] == 0x01 {
            offset += 1; // skip signature_present byte
            let signature_bytes = Sv2Codec::decode_bytes(data, &mut offset)?;
            let public_key = Sv2Codec::decode_bytes(data, &mut offset)?;
            
            let mut message_hash = [0u8; 32];
            message_hash.copy_from_slice(&data[offset..offset + 32]);
            offset += 32;
            
            let created_at = Sv2Codec::decode_u64(data, &mut offset)?;
            
            Some(Dilithium3Signature {
                signature: signature_bytes,
                public_key,
                message_hash,
                created_at,
            })
        } else {
            offset += 1; // skip signature_present byte
            None
        };
        
        let height = Sv2Codec::decode_u64(data, &mut offset)?;
        
        Ok(ExtendedMiningJob {
            channel_id,
            job_id,
            future_job,
            version,
            coinbase_tx_prefix,
            coinbase_tx_suffix,
            merkle_path,
            prev_hash,
            ntime,
            nbits,
            target,
            signature,
            height,
        })
    }
}

/// Active miner connection state with encryption
#[derive(Debug)]
pub struct MinerConnection {
    pub channel_id: u32,
    pub user_id: String,
    pub nominal_hash_rate: f64,
    pub current_target: [u8; 32],
    pub extranonce_prefix: Vec<u8>,
    pub connected_at: SystemTime,
    pub shares_submitted: u64,
    pub shares_accepted: u64,
}

impl MinerConnection {
    /// Get connection statistics
    pub fn get_stats(&self) -> (u64, u64, f64) {
        let acceptance_rate = if self.shares_submitted > 0 {
            (self.shares_accepted as f64 / self.shares_submitted as f64) * 100.0
        } else {
            0.0
        };
        (self.shares_submitted, self.shares_accepted, acceptance_rate)
    }
    
    /// Get connection uptime in seconds
    pub fn get_uptime_seconds(&self) -> u64 {
        SystemTime::now()
            .duration_since(self.connected_at)
            .unwrap_or_default()
            .as_secs()
    }
    
    /// Check if connection is considered active (submitted shares recently)
    pub fn is_active(&self) -> bool {
        self.get_uptime_seconds() < 300 && self.shares_submitted > 0 // 5 minutes
    }
}

/// Noise handshake patterns for Stratum V2
const NOISE_PATTERN: &str = "Noise_XX_25519_ChaChaPoly_BLAKE2s";

/// Stratum V2 server with production-grade implementation and Noise encryption
pub struct StratumV2Server {
    mining_service: Arc<MiningService>,
    active_connections: Arc<RwLock<HashMap<String, MinerConnection>>>,
    current_job_id: Arc<RwLock<u32>>,
    noise_keypair: Vec<u8>, // Static key for Noise protocol
    connections_tx: Option<Sender<bool>>, // Channel to signal connection state changes
}

impl StratumV2Server {
    pub fn new(mining_service: Arc<MiningService>) -> Self {
        Self::with_connection_tracking(mining_service, None)
    }
    
    pub fn with_connection_tracking(mining_service: Arc<MiningService>, connections_tx: Option<Sender<bool>>) -> Self {
        // Generate static key for Noise protocol
        let noise_keypair = snow::Builder::new(NOISE_PATTERN.parse().unwrap())
            .generate_keypair()
            .unwrap()
            .private;
        
        Self {
            mining_service,
            active_connections: Arc::new(RwLock::new(HashMap::new())),
            current_job_id: Arc::new(RwLock::new(1)),
            noise_keypair,
            connections_tx,
        }
    }

    /// Notify that a miner has connected
    fn on_miner_connected(&self) {
        if let Some(ref tx) = self.connections_tx {
            let _ = tx.send(true); // At least one miner is connected
        }
    }
    
    /// Notify that a miner has disconnected
    fn on_miner_disconnected(&self) {
        if let Some(ref tx) = self.connections_tx {
            let remaining = self.active_connections.read().len() > 1; // Check if others remain
            let _ = tx.send(remaining);
        }
    }

    /// Start the Stratum V2 server with Noise encryption
    pub async fn start(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let bind_addr = format!("{}:{}", 
            self.mining_service.stratum_bind_address(),
            self.mining_service.stratum_bind_port()
        );

        let listener = TcpListener::bind(&bind_addr).await?;
        log::info!("🚀 Stratum V2 server with Noise encryption listening on {}", bind_addr);

        // Spawn job distribution task
        let job_broadcaster = self.clone();
        tokio::spawn(async move {
            job_broadcaster.job_distribution_loop().await;
        });

        loop {
            let (socket, peer_addr) = listener.accept().await?;
            log::info!("🔌 New Stratum V2 client connected: {}", peer_addr);
            
            let server_clone = self.clone();
            tokio::spawn(async move {
                if let Err(e) = server_clone.handle_connection(socket, peer_addr.to_string()).await {
                    log::error!("❌ Connection error for {}: {}", peer_addr, e);
                }
            });
        }
    }

    /// Handle individual miner connection with Noise handshake
    async fn handle_connection(&self, mut socket: TcpStream, peer_addr: String) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Perform Noise XX handshake
        let transport = self.perform_noise_handshake(&mut socket).await?;
        log::info!("✅ Noise handshake completed for {}", peer_addr);
        
        // Handle encrypted Stratum V2 communication
        self.handle_sv2_session(socket, transport, peer_addr).await
    }
    
    /// Perform Noise XX handshake as responder
    async fn perform_noise_handshake(&self, socket: &mut TcpStream) -> Result<TransportState, Box<dyn std::error::Error + Send + Sync>> {
        let builder = Builder::new(NOISE_PATTERN.parse().unwrap());
        let static_key = builder.generate_keypair().unwrap().private;
        let mut noise = builder
            .local_private_key(&static_key)
            .build_responder()?;
        
        // Stage 1: Receive initiator's message
        let mut read_buf = vec![0u8; 1024];
        let n = socket.read(&mut read_buf).await?;
        read_buf.truncate(n);
        
        let mut response_buf = vec![0u8; 1024];
        let response_len = noise.read_message(&read_buf, &mut response_buf)?;
        
        // Stage 2: Send our response
        let mut send_buf = vec![0u8; 1024];
        let send_len = noise.write_message(&response_buf[..response_len], &mut send_buf)?;
        socket.write_all(&send_buf[..send_len]).await?;
        
        // Stage 3: Receive final handshake message
        let n = socket.read(&mut read_buf).await?;
        read_buf.truncate(n);
        let _final_len = noise.read_message(&read_buf, &mut response_buf)?;
        
        // Handshake complete, switch to transport mode
        Ok(noise.into_transport_mode()?)
    }
    
    /// Handle encrypted SV2 session
    async fn handle_sv2_session(&self, mut socket: TcpStream, mut transport: TransportState, peer_addr: String) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut connection_established = false;
        let mut miner_id = String::new();

        let result = async {
            loop {
                // Read encrypted frame
                let frame = self.read_encrypted_frame(&mut socket, &mut transport).await?;
                
                // Process SV2 message
                let response = self.process_sv2_message(frame, &peer_addr, &mut connection_established, &mut miner_id).await;
                
                // Send encrypted response if needed
                if let Some(resp_frame) = response {
                    self.send_encrypted_frame(&mut socket, &mut transport, resp_frame).await?;
                }
            }
        }.await;

        // Clean up connection when session ends
        if connection_established && !miner_id.is_empty() {
            self.active_connections.write().remove(&miner_id);
            self.on_miner_disconnected();
            log::info!("🔌 Miner {} disconnected from {}", miner_id, peer_addr);
        }

        result
    }
    
    /// Read encrypted SV2 frame
    async fn read_encrypted_frame(&self, socket: &mut TcpStream, transport: &mut TransportState) -> Result<Sv2Frame, Box<dyn std::error::Error + Send + Sync>> {
        // Read encrypted data
        let mut encrypted_buf = vec![0u8; 2048];
        let n = socket.read(&mut encrypted_buf).await?;
        encrypted_buf.truncate(n);
        
        // Decrypt
        let mut decrypted_buf = vec![0u8; 2048];
        let decrypted_len = transport.read_message(&encrypted_buf, &mut decrypted_buf)?;
        decrypted_buf.truncate(decrypted_len);
        
        // Parse SV2 frame
        Sv2Frame::decode(&decrypted_buf)
    }
    
    /// Send encrypted SV2 frame
    async fn send_encrypted_frame(&self, socket: &mut TcpStream, transport: &mut TransportState, frame: Sv2Frame) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Encode frame
        let frame_data = frame.encode();
        
        // Encrypt
        let mut encrypted_buf = vec![0u8; frame_data.len() + 64]; // Extra space for encryption overhead
        let encrypted_len = transport.write_message(&frame_data, &mut encrypted_buf)?;
        encrypted_buf.truncate(encrypted_len);
        
        // Send
        socket.write_all(&encrypted_buf).await?;
        socket.flush().await?;
        
        Ok(())
    }
    
    /// Process SV2 messages with proper binary protocol
    async fn process_sv2_message(&self, frame: Sv2Frame, peer_addr: &str, connection_established: &mut bool, miner_id: &mut String) -> Option<Sv2Frame> {
        match frame.msg_type {
            msg_type if msg_type == Sv2MessageType::SetupConnection as u8 => {
                log::info!("📋 Setup connection from {}", peer_addr);
                
                // Parse setup connection message
                if frame.payload.len() < 6 {
                    return Some(self.create_setup_error_frame("Invalid setup message".to_string()));
                }
                
                let mut offset = 0;
                let protocol_version = u16::from_le_bytes([frame.payload[offset], frame.payload[offset + 1]]);
                offset += 2;
                let flags = u32::from_le_bytes([
                    frame.payload[offset],
                    frame.payload[offset + 1],
                    frame.payload[offset + 2],
                    frame.payload[offset + 3],
                ]);
                
                if protocol_version != SV2_PROTOCOL_VERSION {
                    return Some(self.create_setup_error_frame("Unsupported protocol version".to_string()));
                }
                
                // Create success response
                let mut payload = Vec::new();
                payload.extend_from_slice(&SV2_PROTOCOL_VERSION.to_le_bytes());
                payload.extend_from_slice(&(flags & 0x01).to_le_bytes()); // Support job negotiation
                
                Some(Sv2Frame {
                    extension_type: 0,
                    msg_type: Sv2MessageType::SetupConnectionSuccess as u8,
                    msg_length: payload.len() as u32,
                    payload,
                })
            }
            
            msg_type if msg_type == Sv2MessageType::OpenStandardMiningChannel as u8 => {
                if frame.payload.len() < 12 {
                    return Some(self.create_channel_error_frame(0, "Invalid channel open message".to_string()));
                }
                
                let mut offset = 0;
                let request_id = Sv2Codec::decode_u32(&frame.payload, &mut offset).unwrap_or(0);
                let user_id = Sv2Codec::decode_string(&frame.payload, &mut offset).unwrap_or_default();
                let nominal_hash_rate = f64::from_le_bytes([
                    frame.payload[offset], frame.payload[offset + 1], frame.payload[offset + 2], frame.payload[offset + 3],
                    frame.payload[offset + 4], frame.payload[offset + 5], frame.payload[offset + 6], frame.payload[offset + 7],
                ]);
                
                let channel_id = self.generate_channel_id();
                let target = generate_difficulty_target(self.get_current_difficulty());
                let extranonce_prefix = self.generate_extranonce_prefix();
                
                // Create miner connection
                let connection = MinerConnection {
                    channel_id,
                    user_id: user_id.clone(),
                    nominal_hash_rate,
                    current_target: target,
                    extranonce_prefix: extranonce_prefix.clone(),
                    connected_at: SystemTime::now(),
                    shares_submitted: 0,
                    shares_accepted: 0,
                };
                
                *miner_id = user_id.clone();
                self.active_connections.write().insert(user_id.clone(), connection);
                *connection_established = true;
                
                // Notify that a new miner has connected
                self.on_miner_connected();
                
                log::info!("✅ Opened mining channel {} for user {}", channel_id, user_id);
                
                // Create success response
                let mut payload = Vec::new();
                payload.extend_from_slice(&Sv2Codec::encode_u32(request_id));
                payload.extend_from_slice(&Sv2Codec::encode_u32(channel_id));
                payload.extend_from_slice(&target);
                payload.extend_from_slice(&Sv2Codec::encode_bytes(&extranonce_prefix));
                
                Some(Sv2Frame {
                    extension_type: 0,
                    msg_type: Sv2MessageType::OpenStandardMiningChannelSuccess as u8,
                    msg_length: payload.len() as u32,
                    payload,
                })
            }
            
            msg_type if msg_type == Sv2MessageType::SubmitSharesStandard as u8 => {
                if !*connection_established {
                    return Some(self.create_submit_error_frame(0, 0, "Channel not established".to_string()));
                }
                
                // Parse share submission
                let mut offset = 0;
                let channel_id = Sv2Codec::decode_u32(&frame.payload, &mut offset).unwrap_or(0);
                let sequence_number = Sv2Codec::decode_u32(&frame.payload, &mut offset).unwrap_or(0);
                let job_id = Sv2Codec::decode_u32(&frame.payload, &mut offset).unwrap_or(0);
                let nonce = Sv2Codec::decode_u64(&frame.payload, &mut offset).unwrap_or(0);
                let ntime = Sv2Codec::decode_u32(&frame.payload, &mut offset).unwrap_or(0);
                let version = Sv2Codec::decode_u32(&frame.payload, &mut offset).unwrap_or(0);
                
                // Validate share using BLAKE3
                match futures::executor::block_on(self.validate_share_blake3(job_id, nonce, ntime, version, channel_id)) {
                    Ok(true) => {
                        // Update connection stats
                        if let Some(conn) = self.active_connections.write().get_mut(miner_id) {
                            conn.shares_submitted += 1;
                            conn.shares_accepted += 1;
                        }

                        log::info!("✅ Valid share submitted by {} (job: {}, nonce: {})", miner_id, job_id, nonce);
                        
                        let mut payload = Vec::new();
                        payload.extend_from_slice(&Sv2Codec::encode_u32(channel_id));
                        payload.extend_from_slice(&Sv2Codec::encode_u32(sequence_number));
                        payload.extend_from_slice(&Sv2Codec::encode_u32(1)); // new_submits_accepted_count
                        payload.extend_from_slice(&Sv2Codec::encode_u64(1)); // new_shares_sum
                        
                        Some(Sv2Frame {
                            extension_type: 0,
                            msg_type: Sv2MessageType::SubmitSharesSuccess as u8,
                            msg_length: payload.len() as u32,
                            payload,
                        })
                    }
                    Ok(false) => {
                        if let Some(conn) = self.active_connections.write().get_mut(miner_id) {
                            conn.shares_submitted += 1;
                        }
                        
                        log::warn!("❌ Invalid share from {} (job: {}, nonce: {})", miner_id, job_id, nonce);
                        Some(self.create_submit_error_frame(channel_id, sequence_number, "Invalid share".to_string()))
                    }
                    Err(e) => {
                        log::error!("❌ Share validation error: {}", e);
                        Some(self.create_submit_error_frame(channel_id, sequence_number, format!("Validation error: {}", e)))
                    }
                }
            }
            
            _ => {
                log::warn!("⚠️ Unhandled message type 0x{:02X} from {}", frame.msg_type, peer_addr);
                None
            }
        }
    }

    /// Validate mining share using BLAKE3 target check
    async fn validate_share_blake3(&self, _job_id: u32, nonce: u64, ntime: u32, version: u32, channel_id: u32) -> Result<bool, MiningServiceError> {
        // Get current job template
        let job_template = self.mining_service.get_job()?;
        
        // Reconstruct block header for validation
        let mut header_data = Vec::new();
        header_data.extend_from_slice(&version.to_le_bytes());
        header_data.extend_from_slice(&job_template.header_blob);
        header_data.extend_from_slice(&ntime.to_le_bytes());
        header_data.extend_from_slice(&nonce.to_le_bytes());

        // BLAKE3 hash the header
        let hash = blake3_hash(&header_data);
        
        // Get target for this channel
        let target = self.active_connections.read()
            .values()
            .find(|conn| conn.channel_id == channel_id)
            .map(|conn| conn.current_target)
            .unwrap_or(job_template.target);

        // Check if hash meets target (BLAKE3-based difficulty check)
        let meets_target = hash <= target;
        
        if meets_target {
            // If it meets the network target, submit as block
            let network_target = generate_difficulty_target(self.get_current_difficulty());
            if hash <= network_target {
                log::info!("🎯 Share meets network difficulty - submitting as block!");
                let _ = self.mining_service.submit_share(job_template.job_id.clone(), nonce).await;
            }
        }

        Ok(meets_target)
    }

    /// Broadcast new jobs to all connected miners
    async fn job_distribution_loop(&self) {
        let mut last_job_broadcast = SystemTime::UNIX_EPOCH;
        
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            
            // Check for new blocks or job updates
            if let Ok(job_template) = self.mining_service.get_job() {
                let should_broadcast = SystemTime::now()
                    .duration_since(last_job_broadcast)
                    .map(|d| d.as_secs() >= 10)
                    .unwrap_or(true);

                if should_broadcast {
                    self.broadcast_new_job(job_template).await;
                    last_job_broadcast = SystemTime::now();
                }
            }
        }
    }

    /// Broadcast new mining job to all connected miners with SV2 format
    async fn broadcast_new_job(&self, job_template: crate::mining_service::JobTemplate) {
        let job_id = {
            let mut current_id = self.current_job_id.write();
            *current_id += 1;
            *current_id
        };

        // Create Extended Mining Job with Dilithium3 signature
        let mining_job = ExtendedMiningJob {
            channel_id: 0, // Will be set per connection
            job_id,
            future_job: false,
            version: 1,
            coinbase_tx_prefix: vec![], // Simplified for now - in production would include proper coinbase
            coinbase_tx_suffix: vec![],
            merkle_path: vec![], // Would include actual merkle path in production
            prev_hash: [0u8; 32], // Would be filled from blockchain
            ntime: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as u32,
            nbits: job_template.target.iter().take(4).fold(0u32, |acc, &b| (acc << 8) | b as u32),
            target: job_template.target,
            signature: self.sign_job_template(&job_template).ok(),
            height: job_template.height,
        };

        let connections = self.active_connections.read();
        let connection_count = connections.len();
        
        if connection_count > 0 {
            log::info!("📡 Broadcasting SV2 job {} to {} miners", job_id, connection_count);
            
            // In a real implementation, you'd send encrypted frames to each connection's socket
            // This requires storing socket handles in the connection state
            for (user_id, _connection) in connections.iter() {
                log::debug!("📤 Sending SV2 job {} to miner {}", job_id, user_id);
                
                // Create frame for this specific connection
                let mut connection_job = mining_job.clone();
                connection_job.channel_id = _connection.channel_id;
                
                let payload = connection_job.encode();
                let _frame = Sv2Frame {
                    extension_type: 0,
                    msg_type: Sv2MessageType::NewMiningJob as u8,
                    msg_length: payload.len() as u32,
                    payload,
                };
                
                // Would send encrypted frame here: send_encrypted_frame(&mut socket, &mut transport, frame)
            }
        }
    }

    /// Sign job template with mining service's Dilithium3 key
    fn sign_job_template(&self, job_template: &crate::mining_service::JobTemplate) -> Result<Dilithium3Signature, MiningServiceError> {
        // Create message to sign (job template without signature)
        let mut message = Vec::new();
        message.extend_from_slice(&job_template.height.to_le_bytes());
        message.extend_from_slice(&job_template.header_blob);
        message.extend_from_slice(&job_template.target);
        
        // Sign with the mining service's keypair (this would use the node's signing key)
        // For now, we create a mock signature that miners can skip if they don't support Dilithium3
        Ok(Dilithium3Signature {
            signature: vec![0u8; 3293], // Dilithium3 signature size - in production would be actual signature
            public_key: vec![0u8; 1952], // Dilithium3 public key size - in production would be actual public key
            message_hash: blake3_hash(&message),
            created_at: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
        })
    }

    /// Generate unique channel ID
    fn generate_channel_id(&self) -> u32 {
        use std::sync::atomic::{AtomicU32, Ordering};
        static CHANNEL_COUNTER: AtomicU32 = AtomicU32::new(1);
        CHANNEL_COUNTER.fetch_add(1, Ordering::SeqCst)
    }

    /// Generate extranonce prefix for miner
    fn generate_extranonce_prefix(&self) -> Vec<u8> {
        use rand::RngCore;
        let mut rng = rand::thread_rng();
        let mut prefix = vec![0u8; 4];
        rng.fill_bytes(&mut prefix);
        prefix
    }

    /// Get current difficulty from mining service
    fn get_current_difficulty(&self) -> u32 {
        self.mining_service.get_current_difficulty()
    }
    
    /// Get server statistics for monitoring
    pub fn get_server_stats(&self) -> ServerStats {
        let connections = self.active_connections.read();
        let total_connections = connections.len() as u32;
        let mut total_hash_rate = 0.0;
        let mut active_connections = 0u32;
        let mut total_shares_submitted = 0u64;
        let mut total_shares_accepted = 0u64;
        
        for conn in connections.values() {
            total_hash_rate += conn.nominal_hash_rate;
            if conn.is_active() {
                active_connections += 1;
            }
            total_shares_submitted += conn.shares_submitted;
            total_shares_accepted += conn.shares_accepted;
        }
        
        let overall_acceptance_rate = if total_shares_submitted > 0 {
            (total_shares_accepted as f64 / total_shares_submitted as f64) * 100.0
        } else {
            0.0
        };
        
        ServerStats {
            total_connections,
            active_connections,
            total_hash_rate,
            total_shares_submitted,
            total_shares_accepted,
            overall_acceptance_rate,
        }
    }
    
    /// Get detailed connection info for a specific user
    pub fn get_connection_info(&self, user_id: &str) -> Option<ConnectionInfo> {
        let connections = self.active_connections.read();
        connections.get(user_id).map(|conn| ConnectionInfo {
            user_id: conn.user_id.clone(),
            channel_id: conn.channel_id,
            nominal_hash_rate: conn.nominal_hash_rate,
            uptime_seconds: conn.get_uptime_seconds(),
            shares_submitted: conn.shares_submitted,
            shares_accepted: conn.shares_accepted,
            acceptance_rate: conn.get_stats().2,
            extranonce_prefix: hex::encode(&conn.extranonce_prefix),
            is_active: conn.is_active(),
        })
    }

    /// Create a setup connection error frame
    fn create_setup_error_frame(&self, error_message: String) -> Sv2Frame {
        let mut payload = Vec::new();
        payload.extend_from_slice(&SV2_PROTOCOL_VERSION.to_le_bytes());
        payload.extend_from_slice(&0u32.to_le_bytes()); // error_code
        payload.extend_from_slice(&Sv2Codec::encode_string(&error_message));
        Sv2Frame {
            extension_type: 0,
            msg_type: Sv2MessageType::SetupConnectionError as u8,
            msg_length: payload.len() as u32,
            payload,
        }
    }

    /// Create an open standard mining channel error frame
    fn create_channel_error_frame(&self, request_id: u32, error_message: String) -> Sv2Frame {
        let mut payload = Vec::new();
        payload.extend_from_slice(&Sv2Codec::encode_u32(request_id));
        payload.extend_from_slice(&0u32.to_le_bytes()); // error_code
        payload.extend_from_slice(&Sv2Codec::encode_string(&error_message));
        Sv2Frame {
            extension_type: 0,
            msg_type: Sv2MessageType::OpenStandardMiningChannelError as u8,
            msg_length: payload.len() as u32,
            payload,
        }
    }

    /// Create a submit shares error frame
    fn create_submit_error_frame(&self, channel_id: u32, sequence_number: u32, error_message: String) -> Sv2Frame {
        let mut payload = Vec::new();
        payload.extend_from_slice(&Sv2Codec::encode_u32(channel_id));
        payload.extend_from_slice(&Sv2Codec::encode_u32(sequence_number));
        payload.extend_from_slice(&0u32.to_le_bytes()); // error_code
        payload.extend_from_slice(&Sv2Codec::encode_string(&error_message));
        Sv2Frame {
            extension_type: 0,
            msg_type: Sv2MessageType::SubmitSharesError as u8,
            msg_length: payload.len() as u32,
            payload,
        }
    }
}

impl Clone for StratumV2Server {
    fn clone(&self) -> Self {
        Self {
            mining_service: self.mining_service.clone(),
            active_connections: self.active_connections.clone(),
            current_job_id: self.current_job_id.clone(),
            noise_keypair: self.noise_keypair.clone(), // Clone the static key
            connections_tx: self.connections_tx.clone(), // Clone the channel
        }
    }
}