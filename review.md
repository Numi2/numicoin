## Numi Blockchain Core ‑ Code Review (2025-07-22)

### Scope
This review covers the **`core`** crate (Rust) that implements the on-chain logic, cryptography, networking, storage, mining and RPC for the Numi blockchain.  Focus areas:
1. Consensus / chain management (`blockchain.rs`, `block.rs`)
2. Transaction layer & mempool (`transaction.rs`, `mempool.rs`)
3. Cryptography & PoW (`crypto.rs`, `miner.rs`)
4. Persistent storage (`storage.rs`, `secure_storage.rs`)
5. P2P layer & external interfaces (`network.rs`, `rpc.rs`)

---

### High-level Assessment
| Area | Status | Notes |
|------|--------|-------|
| Core data structures (Block, Tx, Account) | ✅ solid | Comprehensive and serialisable. |
| Consensus & re-org | ✅ feature-complete | Longest-chain + cumulative difficulty implemented, orphan pool handled. |
| Difficulty retarget | ✅ basic | Needs tuning & test vectors. |
| Mempool | ✅ MVP | Fee/size based eviction, rate-limit hooks. Balance/unstake validation **TODO**. |
| Mining | ✅ functional | Multithreaded, Argon2id+BLAKE3 PoW. RPC glue missing. |
| Crypto (Dilithium3) | ✅ integrates pqcrypto | **Needs** optional switch to liboqs once prod ready. |
| Storage (sled) | ✅ works | RocksDB feature is unused; choose one KV backend. |
| P2P (libp2p) | 🟡 partial | FloodSub works, but message routing to chain/mempool still **TODO**. No header/chain-sync logic. |
| RPC (warp) | 🟡 partial | Many endpoints done, but block-by-hash, fee calc, mining endpoint, sync status still **TODO**. |
| Tests / CI | 🟡 moderate | Unit tests present; need integration & network tests + CI pipeline. |
| Compilation on current tool-chain | 🔴 fails | ~60 errors due to libp2p API drift, mismatched field names, missing Error variants. |

---

### Detailed Findings & Action Items

#### 1. Consensus / Blockchain (`blockchain.rs`, `block.rs`)
- [ ] **Formal proofs & fuzzing** – add proptests for fork handling, difficulty retarget edge cases.
- [ ] **Prune side-chains** – orphan/side-chain blocks can grow unbounded; implement LRU trimming policy.
- [ ] **Checkpointing** – periodic sealed checkpoints for fast sync.

#### 2. Transaction Layer & Mempool
- [ ] Implement balance check in `mempool::validate_transaction` (line 362) – requires read-only access to current account state.
- [ ] Implement unstake-rule validation (line 370).
- [ ] Dynamic **minimum fee rate** based on mempool pressure; expose via config.
- [ ] Add mempool → miner pipeline for real-time tx selection (currently pulled by blockchain only).

#### 3. P2P Layer (`network.rs`)
- [ ] Wire **incoming messages** to blockchain / mempool:
  * `TOPIC_BLOCKS` ⇒ `NumiBlockchain::add_block`
  * `TOPIC_TRANSACTIONS` ⇒ `NumiBlockchain::add_transaction`
- [ ] Populate and broadcast `PeerInfo`; maintain `chain_height`/`is_syncing` flags.
- [ ] Replace FloodSub with Gossipsub v1.1 or Kademlia+Bitswap for better scalability.
- [ ] Implement **header/chain-sync**: headers first, then blocks (IBD).
- [ ] Persist peer reputation to disk; evict indefinitely banned peers.

#### 4. RPC Layer (`rpc.rs`)
- [ ] **Status endpoint** – fetch `network_peers` from `NetworkManager` & expose `is_syncing`.
- [ ] **Block lookup by hash** (line 668) – maintain `Hash→Block` index or query storage tree keyed by hash.
- [ ] **Fee statistics** – compute per-tx fee & include in `TransactionSummary`.
- [ ] **Mining endpoint** – start `Miner`, stream progress, return mined block.
- [ ] Harden rate-limiter – store state in `tokio::RwLock` instead of DashMap to drop `unsafe impl Send`.

#### 5. Mining / PoW
- [ ] Accept **dynamic Argon2 parameters** from chain state (for ASIC-resistance tuning).
- [ ] Add GPU/OpenCL experimental backend (feature-flagged).
- [ ] Expose miner control over RPC / CLI.

#### 6. Cryptography
- [ ] Implement feature-flag `real-liboqs` end-to-end and provide migration path.
- [ ] Benchmark Dilithium3 verification throughput; consider caching verified pubkeys.
- [ ] Add BIP-39-style mnemonic for key backup.

#### 7. Storage
- [ ] Decide on primary backend: **sled vs rocksdb**.  Currently both deps exist but only sled used.
- [ ] Batch writes & snapshots for chain state on every N blocks.
- [ ] Database migration tool (for breaking schema changes).

#### 8. Compilation & Dependencies
- [ ] Update to latest `libp2p (>=0.53)`; adjust derive macro path (`#[derive(NetworkBehaviour)]`).
- [ ] Add missing `BlockchainError::*` variants cited in code (SerializationError, InvalidSignature, etc.).
- [ ] Audit versions for security advisories (argon2 <0.5.2 vulnerability CVE-2023-XXXX).
- [ ] Set MSRV in **Cargo.toml** and add CI check.

#### 9. Testing & Tooling
- [ ] Add **integration tests**: multi-node network simulation with `tokio::test` & in-memory transport.
- [ ] Property-based tests for tx validity using `proptest`.
- [ ] Benchmark harness (`criterion`) for mining & block validation.
- [ ] GitHub Actions CI: build, clippy (`-D warnings`), test, fmt.

#### 10. Documentation & UX
- [ ] Sync README & `tasks.md` with real code status; currently over-states completion.
- [ ] Provide API swagger / OpenAPI spec for RPC.
- [ ] Write ADRs (architecture decision records) for consensus, PoW algorithm choices.

---

### Architectural Decisions Pending
1. **Fee model** – Flat per-byte vs dynamic (EIP-1559-style). Needs economics study.
2. **State model** – UTXO-like vs account-based (current code mixes notions). Choose and refactor accordingly.
3. **Database backend** – Sled is simpler, RocksDB gives better large-scale performance; pick one.
4. **PoW future-proofing** – Argon2id parameters hard-coded; plan upgrade path or PoS migration.
5. **Network protocol** – Remain on FloodSub or migrate to Gossipsub + Bitswap for scalability.

Document each decision in `/docs/adr-NNN-*.md` going forward.

---

### Prioritised Roadmap (next 4 weeks)
| Week | Goals |
|------|-------|
| 1 | Fix compilation errors; CI green; implement missing `BlockchainError` variants; add basic block-by-hash RPC. |
| 2 | Wire P2P ↔ blockchain/mempool; implement fee calculation; expose peer count & sync status via RPC. |
| 3 | Integration tests (3-node localnet); finish mempool balance & unstake validation; choose database backend. |
| 4 | Complete mining RPC, dynamic difficulty retarget tests; prepare beta release & public testnet launch. |

---

### Quick-start Checklist for New Contributors
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo fmt --all`
- Run unit tests: `cargo test --all`
- Use `RUST_LOG=info` for meaningful logs during dev.
- For faster mining during tests: `export NUMI_DEV_POW=1` (uses `Argon2Config::development`).

---

*Generated by code review on 2025-07-22.*