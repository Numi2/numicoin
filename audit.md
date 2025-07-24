# Audit Report: Red Flags

## core/src/block.rs
- unwrap_or_default on bincode::serialize hides serialization errors in header hashing.

## core/src/blockchain.rs
- calculate_block_work = 2u128.pow(difficulty) is overflow-prone and misrepresents mining work.
- Orphan block processing loops unbounded; vulnerable to DoS from orphan storms.

## core/src/config.rs
- Config::default embeds hardcoded JWT secret and admin API key; insecure for production.

## core/src/crypto.rs
- secret_key Vec<u8> not zeroized on drop; private material lingers in memory.
- No Kyber KEM implementation despite network handshake expecting kyber keys.

## core/src/error.rs
- No red flags identified.

## core/src/lib.rs
- No red flags identified.

## core/src/main.rs
- create_config produces insecure default secrets and HTTP RPC without TLS.

## core/src/mempool.rs
- Transaction::new defaults fee=0 and gas_limit=0, allowing spam free transactions.
- BTreeMap<TransactionPriority> ordering may collide on equal fee rates.

## core/src/miner.rs
- mining_thread_worker lacks nonce-range coordination; threads may duplicate work.
- No fail-safe on thread stalls or crashes; potential orphaned threads.

## core/src/network.rs
- SecureStream handshake relies on missing Kyber KEM; key exchange unimplemented.
- No TLS; custom crypto unvetted and likely vulnerable.

## core/src/pqc_transport.rs
- Nonce derived solely from counter; lacks randomness and replay protection.
- No binding between Dilithium signature and Kyber key exchange.

## core/src/rpc.rs
- AuthConfig.require_auth=false by default; RPC APIs exposed unauthenticated.
- Admin endpoints enabled by default without proper auth guards.

## core/src/secure_storage.rs
- Password hashes and KDF params stored in memory without zeroization.
- EncryptedKeyEntry metadata not integrity-protected; no MAC on metadata.

## core/src/storage.rs
- Data at rest stored unencrypted; no optional disk encryption.
- clear_all_data wipes all state indiscriminately; too permissive.

## core/src/transaction.rs
- Transaction::new uses zero fee and gas_limit by default; permits free and gas-less txs.
- Contract txs declared unsupported but not rejected early; risk runtime errors. 