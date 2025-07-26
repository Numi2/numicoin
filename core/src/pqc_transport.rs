//! Post-quantum handshake transport: Kyber KEM for key exchange + Dilithium3 authentication.
use tokio::io::{AsyncRead, AsyncWrite, AsyncReadExt, AsyncWriteExt};
use crate::crypto::{Dilithium3Keypair, blake3_hash, BlockchainError, Result};
use rand::RngCore;
use std::time::{SystemTime, UNIX_EPOCH};
use pqcrypto_kyber::kyber768::{PublicKey as KyberPublicKey, SecretKey as KyberSecretKey, Ciphertext as KyberCiphertext};
use pqcrypto_dilithium::dilithium3::{PublicKey as DilithiumPublicKey, DetachedSignature};
use aes_gcm::{Aes256Gcm, aead::{Aead, KeyInit}, Nonce};
use std::{pin::Pin, task::{Context, Poll}, io::{self, ErrorKind}};
use tokio::io::ReadBuf;

// Maximum allowed timestamp skew for handshake replay protection (5 minutes)
const MAX_HANDSHAKE_TIMESTAMP_SKEW: u64 = 300;

/// AEAD-secured stream wrapper
pub struct SecureStream<T> {
    inner: T,
    cipher: Aes256Gcm,
    send_nonce: u64,
    recv_nonce: u64,
    buf: Vec<u8>,
}

impl<T: AsyncRead + AsyncWrite + Unpin> SecureStream<T> {
    fn new(inner: T, key: &[u8]) -> Result<Self> {
        let cipher = Aes256Gcm::new_from_slice(key)
            .map_err(|e| BlockchainError::CryptographyError(format!("AEAD init error: {}", e)))?;
        Ok(Self { inner, cipher, send_nonce: 0, recv_nonce: 0, buf: Vec::new() })
    }
    fn nonce(counter: u64) -> Nonce {
        let mut bytes = [0u8; 12];
        bytes[4..].copy_from_slice(&counter.to_be_bytes());
        Nonce::from_slice(&bytes).clone()
    }
}

impl<T: AsyncRead + AsyncWrite + Unpin> AsyncWrite for SecureStream<T> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let nonce = Self::nonce(self.send_nonce);
        let ct = self.cipher.encrypt(nonce, buf)
            .map_err(|_| io::Error::new(ErrorKind::Other, "AEAD encrypt"))?;
        self.send_nonce = self.send_nonce.wrapping_add(1);
        let len = (ct.len() as u32).to_be_bytes();
        let payload = [&len[..], &ct[..]].concat();
        match Pin::new(&mut self.inner).poll_write(cx, &payload) {
            Poll::Ready(Ok(_)) => Poll::Ready(Ok(buf.len())),
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            Poll::Pending => Poll::Pending,
        }
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

impl<T: AsyncRead + AsyncWrite + Unpin> AsyncRead for SecureStream<T> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        out: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if !self.buf.is_empty() {
            let n = std::cmp::min(out.remaining(), self.buf.len());
            out.put_slice(&self.buf[..n]);
            self.buf.drain(..n);
            return Poll::Ready(Ok(()));
        }
        // Read frame length
        let mut len_buf = [0u8; 4];
        let mut rb = ReadBuf::new(&mut len_buf);
        match Pin::new(&mut self.inner).poll_read(cx, &mut rb) {
            Poll::Ready(Ok(())) => {}, Poll::Ready(Err(e)) => return Poll::Ready(Err(e)), Poll::Pending => return Poll::Pending,
        }
        let ct_len = u32::from_be_bytes(len_buf) as usize;
        // Read ciphertext
        let mut ct = vec![0u8; ct_len];
        let mut rb2 = ReadBuf::new(&mut ct);
        match Pin::new(&mut self.inner).poll_read(cx, &mut rb2) {
            Poll::Ready(Ok(())) => {}, Poll::Ready(Err(e)) => return Poll::Ready(Err(e)), Poll::Pending => return Poll::Pending,
        }
        let nonce = SecureStream::<T>::nonce(self.recv_nonce);
        let pt = self.cipher.decrypt(nonce, &ct)
            .map_err(|_| io::Error::new(ErrorKind::Other, "AEAD decrypt"))?;
        self.recv_nonce = self.recv_nonce.wrapping_add(1);
        let n = std::cmp::min(out.remaining(), pt.len());
        out.put_slice(&pt[..n]);
        self.buf = pt[n..].to_vec();
        Poll::Ready(Ok(()))
    }
}

/// Perform a PQC handshake over a raw stream using Kyber and Dilithium3,
/// then return an AEAD-wrapped secure stream.
pub async fn handshake<T>(
    mut stream: T,
    dilithium_kp: &Dilithium3Keypair,
    our_kyber_pk: &[u8],
    our_kyber_sk: &[u8],
) -> Result<SecureStream<T>>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    // 1. Exchange Dilithium3 public keys
    let our_dilithium_pk = dilithium_kp.public_key_bytes();
    stream.write_all(&(our_dilithium_pk.len() as u32).to_be_bytes()).await?;
    stream.write_all(our_dilithium_pk).await?;
    let mut buf4 = [0u8; 4];
    stream.read_exact(&mut buf4).await?;
    let peer_d_len = u32::from_be_bytes(buf4) as usize;
    let mut peer_d_pk = vec![0u8; peer_d_len];
    stream.read_exact(&mut peer_d_pk).await?;
    let peer_dil_pk = DilithiumPublicKey::from_bytes(&peer_d_pk)
        .map_err(|e| BlockchainError::CryptographyError(format!("Invalid peer Dilithium PK: {:?}", e)))?;

    // 2. Ephemeral Kyber handshake for PFS
    let (eph_pk, eph_sk) = kyber768::keypair();
    // exchange ephemeral public keys
    stream.write_all(&(eph_pk.as_bytes().len() as u32).to_be_bytes()).await?;
    stream.write_all(eph_pk.as_bytes()).await?;
    stream.read_exact(&mut buf4).await?;
    let peer_eph_len = u32::from_be_bytes(buf4) as usize;
    let mut peer_eph_pk_bytes = vec![0u8; peer_eph_len];
    stream.read_exact(&mut peer_eph_pk_bytes).await?;
    let peer_eph_pk = KyberPublicKey::from_bytes(&peer_eph_pk_bytes)
        .map_err(|e| BlockchainError::CryptographyError(format!("Invalid peer ephemeral Kyber PK: {:?}", e)))?;
    // symmetric KEM handshake (both sides encapsulate)
    let (ct1, ss1) = kyber768::encapsulate(&peer_eph_pk);
    stream.write_all(&(ct1.as_bytes().len() as u32).to_be_bytes()).await?;
    stream.write_all(ct1.as_bytes()).await?;
    stream.read_exact(&mut buf4).await?;
    let ct2_len = u32::from_be_bytes(buf4) as usize;
    let mut ct2_bytes = vec![0u8; ct2_len];
    stream.read_exact(&mut ct2_bytes).await?;
    let ct2 = KyberCiphertext::from_bytes(&ct2_bytes)
        .map_err(|e| BlockchainError::CryptographyError(format!("Invalid peer ciphertext: {:?}", e)))?;
    let ss2 = kyber768::decapsulate(&ct2, &eph_sk);
    // Derive symmetric key (PFS)
    let mut km = Vec::new();
    km.extend_from_slice(ss1.as_bytes());
    km.extend_from_slice(ss2.as_bytes());
    let sym_key = blake3_hash(&km);

    // 3. Sign handshake transcript
    // Generate timestamp and nonce for replay protection
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| BlockchainError::CryptographyError(format!("Time error: {}", e)))?
        .as_secs();
    let mut rng = rand::rngs::OsRng;
    let nonce = rng.next_u64();
    let mut transcript = Vec::new();
    transcript.extend_from_slice(our_dilithium_pk);
    transcript.extend_from_slice(&peer_d_pk);
    transcript.extend_from_slice(eph_pk.as_bytes());
    transcript.extend_from_slice(&peer_eph_pk_bytes);
    transcript.extend_from_slice(ct1.as_bytes());
    transcript.extend_from_slice(&ct2_bytes);
    // Include timestamp and nonce in transcript
    transcript.extend_from_slice(&timestamp.to_be_bytes());
    transcript.extend_from_slice(&nonce.to_be_bytes());
    let sig = dilithium_kp.sign(&transcript)?;
    let sig_bytes = &sig.signature;
    // Send timestamp, nonce, and signature
    stream.write_all(&timestamp.to_be_bytes()).await?;
    stream.write_all(&nonce.to_be_bytes()).await?;
    stream.write_all(&(sig_bytes.len() as u32).to_be_bytes()).await?;
    stream.write_all(sig_bytes).await?;

    // Receive and verify peer signature
    // Receive and verify peer timestamp and nonce
    let mut ts_buf = [0u8; 8];
    stream.read_exact(&mut ts_buf).await?;
    let peer_timestamp = u64::from_be_bytes(ts_buf);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| BlockchainError::CryptographyError(format!("Time error: {}", e)))?
        .as_secs();
    if peer_timestamp > now + MAX_HANDSHAKE_TIMESTAMP_SKEW || peer_timestamp < now - MAX_HANDSHAKE_TIMESTAMP_SKEW {
        return Err(BlockchainError::CryptographyError("Handshake timestamp outside allowed skew".to_string()));
    }
    let mut nonce_buf = [0u8; 8];
    stream.read_exact(&mut nonce_buf).await?;
    let peer_nonce = u64::from_be_bytes(nonce_buf);
    // TODO: implement nonce replay detection if needed
    // Include peer timestamp and nonce in transcript for signature verification
    transcript.extend_from_slice(&peer_timestamp.to_be_bytes());
    transcript.extend_from_slice(&peer_nonce.to_be_bytes());
    // Now read peer signature length and signature
    stream.read_exact(&mut buf4).await?;
    let psig_len = u32::from_be_bytes(buf4) as usize;
    let mut psig = vec![0u8; psig_len];
    stream.read_exact(&mut psig).await?;
    let peer_sig = DetachedSignature::from_bytes(&psig)
        .map_err(|e| BlockchainError::CryptographyError(format!("Invalid peer signature: {:?}", e)))?;
    if !Dilithium3Keypair::verify(&transcript, &peer_sig, &peer_d_pk)? {
        return Err(BlockchainError::CryptographyError("Handshake signature invalid".to_string()));
    }

    SecureStream::new(stream, &sym_key)
} 

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{duplex, AsyncReadExt, AsyncWriteExt};
    use crate::crypto::Dilithium3Keypair;
    use pqcrypto_kyber::kyber768;

    // Helper to create fresh static Kyber and Dilithium3 keypairs
    fn make_keys() -> (Dilithium3Keypair, Vec<u8>, Vec<u8>) {
        let dil = Dilithium3Keypair::new().unwrap();
        let (kyber_pk, kyber_sk) = kyber768::keypair();
        (dil, kyber_pk.as_bytes().to_vec(), kyber_sk.as_bytes().to_vec())
    }

    #[tokio::test]
    async fn pqc_transport_roundtrip() {
        // 1) build in-memory stream
        let (side_a, side_b) = duplex(64 * 1024);

        // 2) generate static keys for A and B
        let (dil_a, pk_a, sk_a) = make_keys();
        let (dil_b, pk_b, sk_b) = make_keys();

        // 3) perform handshake and exchange a message
        let a = tokio::spawn(async move {
            let mut stream = handshake(side_a, &dil_a, &pk_a, &sk_a)
                .await.expect("A handshake failed");
            let msg = b"hello from A";
            stream.write_all(msg).await.unwrap();
            stream.flush().await.unwrap();
            let mut buf = vec![0u8; msg.len()];
            stream.read_exact(&mut buf).await.unwrap();
            buf
        });

        let b = tokio::spawn(async move {
            let mut stream = handshake(side_b, &dil_b, &pk_b, &sk_b)
                .await.expect("B handshake failed");
            let mut buf = vec![0u8; b"hello from A".len()];
            stream.read_exact(&mut buf).await.unwrap();
            stream.write_all(&buf).await.unwrap();
            stream.flush().await.unwrap();
            buf
        });

        let (sent, received) = tokio::join!(a, b);
        assert_eq!(sent.unwrap(), received.unwrap());
    }
} 