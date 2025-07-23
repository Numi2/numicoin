# Numi Blockchain Core

## Economic Incentives (2025-07 update)

The core now implements **production-grade mining rewards** that align miner incentives with network health.

1. **Block Subsidy**  
   • Follows a Bitcoin-style halving schedule (initial 50 NUMI, halves every 210 000 blocks, max 64 halvings).  
   • Implemented in `Miner::calculate_block_reward` and reused by validation logic.

2. **Transaction Fees**  
   • Every transaction carries a fee (`transaction.fee`).  
   • Fees are debited from the sender at execution (`apply_transaction`) and credited to the miner via the reward tx.

3. **Reward Transaction**  
   • Miner automatically constructs a `TransactionType::MiningReward` containing `subsidy + total_fees`.  
   • Placed as **tx[0]** in every block template and signed by the miner key.  
   • Validation (`validate_block_basic` & enhanced validator) enforces `reward ≤ subsidy + fees`.

4. **Consensus Impact**  
   • Fees are now part of the economic security budget; invalid fee handling will orphan a block.  
   • `BlockchainError` conversion for `JoinError` added (backup/restore tasks).

5. **Thread-safety Fixes**  
   • `NetworkManager` marked `Send + Sync` so it can be spawned in its own async task.  
   • Duplicate `NumiBehaviourEvent` alias removed.

Compile with:

```bash
cd core
cargo check
```

---

For the complete design rationale see the comments inside:

* `core/src/miner.rs`  – reward construction & PoW loop.  
* `core/src/blockchain.rs` – fee debits, reward validation, account updates.
* `core/src/error.rs` – new `TaskJoinError` variant.
