# Numi Core Code Review

_Date: 2025-07-23_

> This document contains a **high-level engineering review** of every artefact currently present in the `core/` directory.  The goal is to give maintainers quick, actionable feedback without getting lost in generated or third-party code.  Lines flagged with 🚩 deserve higher priority fixes.

---

## Top-level files

### `core/Cargo.toml`
* **Purpose** – Dependency & workspace manifest.
* **Strengths**
  * Clear grouping of crates (crypto, networking, async, dev-deps).
  * Optional `real-liboqs` feature anticipates future migration to a production PQ library.
* **Observations / Suggestions**
  * 🚩  A mix of cryptography crates (`pqcrypto-dilithium`, `oqs`, `base64ct`) with different maturity levels.  Once `liboqs` is enabled the temporary crate should be gated behind the same feature flag to avoid duplicate algorithms.
  * Versions such as `base64ct = "=1.7.1"` are *pinned* with the `=` requirement.  This prevents receiving security patches.  Use `^1.7.1` or a range unless a reproducible build is mandatory.
  * Both `sled` _and_ `rocksdb` are listed though only `sled` appears in the code.  Remove unused deps to shrink build time.
  * Re-evaluate `libp2p 0.56` → latest (currently 0.58) to receive bug-fixes.

### `core/Cargo.lock`
* Generated – should not be hand-edited.  ✅  No human review required.

### `core/README.md`
* Concise update log focused on economic incentives.
* Consider expanding build & test instructions and linking to the CLI reference (`main.rs`).

### `core/data/`
* `db` / `snap.*` look binary → exclude from VCS unless intentionally versioned.
* `conf` contains stray binary bytes (`v�Q�`).  Verify encoding or regenerate.

---

## `src/` modules

### `lib.rs`
* Lightweight re-export hub – good.
* Suggest exporting types behind feature flags (e.g. `#[cfg(feature = "rpc")]`).

### `error.rs`
* Comprehensive set of domain errors with `From<..>` impls – nice!
* 🚩  Macro such as `thiserror::Error` could reduce boilerplate and auto-generate `std::error::Error`.

### `crypto.rs`
* Ambitious 1100-line module covering hashing, Dilithium3 wrapper, Argon2 PoW & helpers.
* **Positives**
  * Uses `zeroize` to wipe secret material ✔️
  * Batch signature verification & time-bounded verification are advanced touches.
* **Concerns**
  * 🚩  Very large single file → split into `hash`, `pow`, `dilithium`, `kdf` sub-modules.
  * Directly exposes `Vec<u8>` for keys; consider new-type wrappers with length invariants.
  * Missing property tests for `constant_time_eq`.

### `block.rs`
* Clean, <300 lines.  Blocks are immutable once created apart from `sign()` which mutates – that is fine.
* Suggest caching `hash` after first computation to avoid repeated Merkle + hash work.

### `transaction.rs`
* Rich, well-documented enum of transaction types.
* Validation logic thorough but spans 1 k lines – candidate for refactor (`fee`, `validation`, `helpers` modules).
* 🚩  `calculate_size()` serialises via `bincode` on every call; cache once.

### `mempool.rs`
* Uses concurrent structures (`DashMap`, `RwLock`) – good.
* Fee-priority queue implemented via `BTreeMap` – O(log n) operations.
* Improvement ideas:
  * Apply `parking_lot::Mutex` instead of `RwLock` around `BTreeMap` to avoid writer starvation.
  * Rate-limiting currently uses a `Vec<Instant>` per account – unbounded growth.  Purge old samples on insert rather than scheduled cleanup.

### `blockchain.rs`
* 2 k+ lines central engine.  Design shows **modularity gaps**:
  * Validation, orphan management, checkpoints, snapshots are all embedded – extract to dedicated structs/services.
  * Several `async fn` call synchronous heavy CPU (e.g. `apply_transaction`) – wrap in `spawn_blocking`.
* 🚩  `MAX_BLOCK_PROCESSING_TIME_MS` is enforced but heavy disk I/O (`save_to_storage`) happens *before* timer ends → potential DoS.
* Consider introducing `tracing` spans; logging currently minimal.

### `miner.rs`
* Rayon-like thread spawner with pause/resume.
* Good use of `AtomicU64` for nonce sharing.
* 🚩  Temperature / power hooks are stubs – document that they are placeholders to avoid false expectations.

### `network.rs`
* Libp2p wrapper using Floodsub & mDNS only.  Kad DHT is enabled in features but not wired – either remove feature or implement.
* Peer reputation model simple (`i32`), ban logic exists but untested.
* Consider upgrading to Tokio-based `rust-libp2p::swarm::behaviour` derive v0.59 API.

### `rpc.rs`
* Warp-based server with Tower middleware – solid.
* JWT/auth scaffolding present but actual verification is missing (search for `TODO` inside handler code).
* Rate limiting stored in `DashMap<SocketAddr, RateLimitEntry>` – memory might blow with NATed users; add eviction.

### `config.rs`
* Clear split into sub-configs.  Good default vs production constructors.
* Validation functions check ranges thoroughly.
* `apply_env_overrides()` could use `envy` crate to reduce boilerplate.

### `storage.rs`
* Simple sled wrapper, but `rocksdb` dependency unused.
* 🚩  `backup_to_directory` copies raw Sled files while DB open – risk of corruption; flush & checkpoint first or use Sled export.
* Missing column-family versioning for future migrations.

### `secure_storage.rs`
* AES-GCM 256 with Scrypt KDF – strong.
* Atomic writes & integrity checks provided.
* Suggest switching `fs::write` to `write_with_permissions` to enforce `0600` on UNIX.

### `main.rs`
* CLI neatly structured with `clap` 4.
* Async main spawns several services sequentially → consider `tokio::select!` for graceful shutdown.
* `acquire_data_dir_lock()` uses `fs2::FileExt::try_lock_exclusive` – nice.

---

## General recommendations
1. **Module decomposition** – Break down very large files (`blockchain.rs`, `crypto.rs`, `transaction.rs`) into sub-modules to aid readability and compile times.
2. **Error handling ergonomics** – Adopt `thiserror` or `anyhow` for automatic `source()` chaining.
3. **Testing** – Unit tests are present but integration tests (network sync, RPC, CLI) are missing.
4. **Security** – Audit external crates (some are outdated).  Implement real entropy check in `Dilithium3Keypair::validate_system_entropy`.
5. **Documentation** – Inline docs abundant, but `cargo doc --open` reveals many public items without summary; add `///` comments.

---

_Reviewed by **Cursor AI assistant**_