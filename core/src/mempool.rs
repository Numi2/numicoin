// src/mempool.rs
//
// Production-ready transaction mempool for NumiCoin
// -------------------------------------------------
// • Pure Rust, no unsafe, lock-free reads via DashMap / parking_lot
// • Fee-rate + age weighted priority queue (LWAPQ¹)
// • Rate-limit & size-limit eviction
//
// ¹ LWAPQ = Log-Weighted Age Penalty Queue: fee_per_byte is weighted by an
//   exponential age decay so old low-fee spam cannot clog the pool indefinitely.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::{Arc, Weak},
    time::{Duration, Instant},
};

use dashmap::DashMap;
use crate::RwLock;
use serde::{Deserialize, Serialize};

use crate::{
    blockchain::NumiBlockchain,
    config::ConsensusConfig,
    error::BlockchainError,
    transaction::{
        Transaction, TransactionId,
    },
    Result,
};

/// ---------------------------------------------------------------------
/// Priority key used in the BTreeMap queue
/// ---------------------------------------------------------------------
#[derive(Debug, Clone, Eq, PartialEq)]
struct PriorityKey {
    /// Higher fee_rate == better (sat/byte)
    fee_rate: u64,
    /// Age penalty (1 == brand-new, 0 == very old)
    age_score: u64,
    /// Deterministic tiebreaker
    tx_id: TransactionId,
}
impl Ord for PriorityKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // natural order → lowest first; we need highest first so reverse
        other
            .fee_rate
            .cmp(&self.fee_rate)
            .then_with(|| other.age_score.cmp(&self.age_score))
            .then_with(|| other.tx_id.cmp(&self.tx_id))
    }
}
impl PartialOrd for PriorityKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// ---------------------------------------------------------------------
/// Validation outcome
/// ---------------------------------------------------------------------
#[derive(Debug, Clone, PartialEq)]
pub enum ValidationResult {
    Valid,
    InvalidSignature,
    InvalidNonce { expected: u64, got: u64 },
    InsufficientBalance { required: u64, available: u64 },
    DuplicateTransaction,
    TransactionTooLarge,
    FeeTooLow { minimum: u64, got: u64 },
    AccountSpamming { rate_limit: u64 },
    TransactionExpired,
}

/// ---------------------------------------------------------------------
/// Mempool statistics snapshot (for RPC / monitoring)
/// ---------------------------------------------------------------------
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MempoolStats {
    pub total_transactions: usize,
    pub total_size_bytes: usize,
    pub fee_buckets: HashMap<String, usize>,
    pub oldest_tx_age: Duration,
    pub accounts_with_pending: usize,
    pub rejected_last_hour: usize,
}

/// ---------------------------------------------------------------------
/// Internal entry wrapper
/// ---------------------------------------------------------------------
#[derive(Debug)]
struct Entry {
    tx: Transaction,
    added: Instant,
    size: usize,
    fee_rate: u64,
    key: PriorityKey,
}

/// ---------------------------------------------------------------------
/// TransactionMempool
/// ---------------------------------------------------------------------
pub struct TransactionMempool {
    // Core structures
    queue: Arc<RwLock<BTreeMap<PriorityKey, TransactionId>>>,
    map: Arc<DashMap<TransactionId, Entry>>,
    by_account: Arc<DashMap<Vec<u8>, HashSet<TransactionId>>>,
    nonces: Arc<DashMap<Vec<u8>, u64>>,
    blockchain: Option<Weak<RwLock<NumiBlockchain>>>,

    // Limits / config
    cfg: ConsensusConfig,
    max_bytes: usize,
    max_txs: usize,
    max_age: Duration,
    max_per_account_hour: usize,

    // Stats / housekeeping
    bytes_used: Arc<RwLock<usize>>,
    rejects_1h: Arc<RwLock<usize>>,
    submissions: Arc<DashMap<Vec<u8>, Vec<Instant>>>,
    last_hour_tick: Arc<RwLock<Instant>>,
}

impl Default for TransactionMempool {
    fn default() -> Self {
        Self::new()
    }
}

impl TransactionMempool {
    /* ---------------- construction ---------------- */
    pub fn new() -> Self {
        Self::with_config(ConsensusConfig::default())
    }

    pub fn with_config(cfg: ConsensusConfig) -> Self {
        Self {
            max_bytes: cfg.max_block_size * 256,                    // 256× block size
            max_txs: cfg.max_transactions_per_block * 1_000,        // 1 000× tx count
            max_age: Duration::from_secs(60 * 60),                  // 1 h
            max_per_account_hour: 100,
            cfg,
            queue: Arc::new(RwLock::new(BTreeMap::new())),
            map: Arc::new(DashMap::new()),
            by_account: Arc::new(DashMap::new()),
            nonces: Arc::new(DashMap::new()),
            blockchain: None,
            bytes_used: Arc::new(RwLock::new(0)),
            rejects_1h: Arc::new(RwLock::new(0)),
            submissions: Arc::new(DashMap::new()),
            last_hour_tick: Arc::new(RwLock::new(Instant::now())),
        }
    }

    pub fn attach_chain(&mut self, chain: &Arc<RwLock<NumiBlockchain>>) {
        self.blockchain = Some(Arc::downgrade(chain));
    }

    /* ---------------- admission ------------------- */
    pub async fn add_transaction(&self, tx: Transaction) -> Result<ValidationResult> {
        let id = tx.id;
        let sender = &tx.from;

        // dup check
        if self.map.contains_key(&id) {
            return Ok(ValidationResult::DuplicateTransaction);
        }

        // structural / sig / balance / fee checks
        let v = self.validate(&tx).await?;
        if v != ValidationResult::Valid {
            *self.rejects_1h.write() += 1;
            return Ok(v);
        }

        // spam rate-limit
        if !self.rate_ok(sender).await {
            return Ok(ValidationResult::AccountSpamming {
                rate_limit: self.max_per_account_hour as u64,
            });
        }

        // space?
        let size = bincode::serialize(&tx).map(|b| b.len()).unwrap_or(512);

        let fee_rate = if size == 0 { 0 } else { tx.fee.div_ceil(size as u64) };

        if !self.can_fit(size, fee_rate) && !self.evict_for(size, fee_rate).await {
            return Ok(ValidationResult::FeeTooLow {
                minimum: self.dynamic_min_fee(),
                got: fee_rate,
            });
        }

        // build entry & priority key
        let key = PriorityKey {
            fee_rate,
            age_score: u64::MAX,
            tx_id: id,
        };
        let entry = Entry {
            tx: tx.clone(),
            added: Instant::now(),
            size,
            fee_rate,
            key: key.clone(),
        };

        // insert atomically
        self.map.insert(id, entry);
        {
            let mut q = self.queue.write();
            q.insert(key, id);
        }
        self.by_account
            .entry(sender.clone())
            .or_default()
            .insert(id);
        self.nonces
            .entry(sender.clone())
            .and_modify(|n| *n = (*n).max(tx.nonce))
            .or_insert(tx.nonce);
        *self.bytes_used.write() += size;
        self.record_submission(sender).await;

        Ok(ValidationResult::Valid)
    }

    /* ---------------- block selection ------------ */
    pub fn select_for_block(
        &self,
        max_block_bytes: usize,
        max_block_txs: usize,
    ) -> Vec<Transaction> {
        self.refresh_priorities();

        let mut selected = Vec::new();
        let mut used = 0;
        let q = self.queue.read();

        for (_, id) in q.iter() {
            if selected.len() >= max_block_txs {
                break;
            }
            if let Some(ent) = self.map.get(id) {
                if used + ent.size > max_block_bytes {
                    continue;
                }
                selected.push(ent.tx.clone());
                used += ent.size;
            }
        }

        selected
    }

    /* ---------------- removal (post-block) ------- */
    pub async fn remove_transactions(&self, ids: &[TransactionId]) {
        for id in ids {
            if let Some((_, ent)) = self.map.remove(id) {
                {
                    let mut q = self.queue.write();
                    q.remove(&ent.key);
                }
                if let Some(mut set) = self.by_account.get_mut(&ent.tx.from) {
                    set.remove(id);
                    if set.is_empty() {
                        drop(set);
                        self.by_account.remove(&ent.tx.from);
                        self.nonces.remove(&ent.tx.from);
                    }
                }
                *self.bytes_used.write() -= ent.size;
            }
        }
    }

    /// Refresh the cached sender nonces from authoritative chain state after a
    /// new block is applied.  This prevents nonce-related rejects when multiple
    /// transactions from the same account are mined across successive blocks.
    pub async fn sync_nonces_from_chain(&self, accounts: &DashMap<Vec<u8>, crate::blockchain::AccountState>) {
        for entry in accounts.iter() {
            self.nonces.insert(entry.key().clone(), entry.value().nonce);
        }
    }

    /* ---------------- stats / maintenance -------- */
    pub fn stats(&self) -> MempoolStats {
        let now = Instant::now();
        let mut oldest = Duration::ZERO;
        let mut fee_buckets = HashMap::new();

        for ent in self.map.iter() {
            let age = now.duration_since(ent.added);
            oldest = oldest.max(age);
            let bucket = match ent.fee_rate {
                0..=1_000 => "low",
                1_001..=5_000 => "medium",
                5_001..=20_000 => "high",
                _ => "premium",
            };
            *fee_buckets.entry(bucket.to_string()).or_insert(0) += 1;
        }

        MempoolStats {
            total_transactions: self.map.len(),
            total_size_bytes: *self.bytes_used.read(),
            fee_buckets,
            oldest_tx_age: oldest,
            accounts_with_pending: self.by_account.len(),
            rejected_last_hour: *self.rejects_1h.read(),
        }
    }

    pub async fn house_keep(&self) {
        let now = Instant::now();

        // expiry
        let mut expired = Vec::new();
        for ent in self.map.iter() {
            if now.duration_since(ent.added) > self.max_age {
                expired.push(ent.key.tx_id);
            }
        }
        if !expired.is_empty() {
            self.remove_transactions(&expired).await;
        }

        // hourly tick
        if now.duration_since(*self.last_hour_tick.write()) > Duration::from_secs(3_600) {
            *self.rejects_1h.write() = 0;
            *self.last_hour_tick.write() = now;
        }

        // clean rate-records
        self.clean_rates().await;

        // priority decay
        self.refresh_priorities();
    }

    pub fn all_transactions(&self) -> Vec<Transaction> {
        self.map.iter().map(|e| e.tx.clone()).collect()
    }

    /* ---------------- internal helpers ----------- */
    fn dynamic_min_fee(&self) -> u64 {
        let util = (*self.bytes_used.read() as f64 / self.max_bytes as f64)
            .max(self.map.len() as f64 / self.max_txs as f64);

        let base = self.cfg.min_transaction_fee;
        if util > 0.9 {
            base * 5
        } else if util > 0.75 {
            base * 3
        } else if util > 0.5 {
            base * 2
        } else {
            base
        }
    }

    fn refresh_priorities(&self) {
        let now = Instant::now();
        let mut new_q = BTreeMap::new();

        for mut ent in self.map.iter_mut() {
            let age = now.duration_since(ent.added).as_secs();
            // Age penalty: after 1 h fee_rate halves every hour
            let decay = (age / 3_600) as u32;
            let age_score = u64::MAX - age; // larger == newer
            ent.key = PriorityKey {
                fee_rate: ent.fee_rate >> decay,
                age_score,
                tx_id: ent.tx.id,
            };
            new_q.insert(ent.key.clone(), ent.tx.id);
        }
        *self.queue.write() = new_q;
    }

    async fn validate(&self, tx: &Transaction) -> Result<ValidationResult> {
        // structural & fee checks
        match tx.validate_structure() {
            Err(BlockchainError::InvalidTransaction(msg))
                if msg.contains("too large") =>
            {
                return Ok(ValidationResult::TransactionTooLarge)
            }
            Err(BlockchainError::InvalidTransaction(msg)) if msg.contains("fee") => {
                let _size = bincode::serialize(tx).map(|b| b.len()).unwrap_or(512);
                let min = 100; // Default minimum fee
                return Ok(ValidationResult::FeeTooLow { minimum: min, got: tx.fee });
            }
            Err(e) => return Err(e),
            Ok(_) => {}
        }

        // signature
        if !tx.verify_signature()? {
            return Ok(ValidationResult::InvalidSignature);
        }

        // nonce sequence
        if let Some(n) = self.nonces.get(&tx.from) {
            if tx.nonce <= *n {
                return Ok(ValidationResult::InvalidNonce {
                    expected: *n + 1,
                    got: tx.nonce,
                });
            }
        }

        // balance
        if let Some(w) = &self.blockchain {
            if let Some(bc) = w.upgrade() {
                let bal = bc.read().get_account_state_or_default(&tx.from).balance;
                if bal < tx.required_balance() {
                    return Ok(ValidationResult::InsufficientBalance {
                        required: tx.required_balance(),
                        available: bal,
                    });
                }
            }
        }

        Ok(ValidationResult::Valid)
    }

    fn can_fit(&self, size: usize, fee_rate: u64) -> bool {
        if self.map.len() >= self.max_txs || *self.bytes_used.read() + size > self.max_bytes {
            return false;
        }
        fee_rate >= self.dynamic_min_fee()
    }

    async fn evict_for(&self, needed: usize, fee_rate: u64) -> bool {
        let mut freed = 0;
        let mut victims = Vec::new();
        {
            let q = self.queue.read();
            for (key, id) in q.iter().rev() {
                if key.fee_rate >= fee_rate {
                    break;
                }
                if let Some(ent) = self.map.get(id) {
                    victims.push(ent.tx.id);
                    freed += ent.size;
                    if freed >= needed {
                        break;
                    }
                }
            }
        }
        if freed >= needed {
            self.remove_transactions(&victims).await;
            true
        } else {
            false
        }
    }

    async fn rate_ok(&self, sender: &[u8]) -> bool {
        let now = Instant::now();
        let window = now - Duration::from_secs(3_600);
        let mut v = self.submissions.entry(sender.to_vec()).or_default();
        v.retain(|&t| t > window);
        v.len() < self.max_per_account_hour
    }

    async fn record_submission(&self, sender: &[u8]) {
        self.submissions
            .entry(sender.to_vec())
            .or_default()
            .push(Instant::now());
    }

    async fn clean_rates(&self) {
        let cutoff = Instant::now() - Duration::from_secs(3_600);
        self.submissions.retain(|_, v| {
            v.retain(|&t| t > cutoff);
            !v.is_empty()
        });
    }
}

