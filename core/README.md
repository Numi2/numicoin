# Numi Blockchain Core - Quantum Safe Poeples blockchain.
1 NUMI = 100 NANO

Proof of work: Argon2d
Signature: Dilithium3
Key Exchange: Kyber768 (KEM)
Blake3: TransactionHashing & Data Integrity & Merkle tree
   Every transaction needs a unique identifier (TXID), which is created by hashing the transaction's data. Furthermore, when nodes sync with the network or receive new blocks and transactions, they must verify the integrity of that data by hashing it and comparing it to a known hash.
      Create and validate nodes MERKLE TREE: Blake3 (
         Constructing a Merkle tree requires hashing every single transaction and then repeatedly hashing the results until one root hash remains. For a block with thousands of transactions, this is a lot of hashing. BLAKE3's ability to process data in parallel makes it exceptionally fast at building these trees. This allows nodes to create and validate blocks much more quickly)


curl -s http://127.0.0.1:8080/status | jq .
RUST_LOG=info cargo run --release -- node --mining

# Start node with Stratum V2 mining
cargo run --release node --mining

# Create a wallet
cargo run --release wallet create --output wallet.json

# Check balance (works with file or address)
numi-core wallet balance my-wallet.json
numi-core wallet balance 167bwvP4puH2qS9EKRExnVhM4wWsZ38TZP

# Send transaction
cargo run --release send --wallet my-wallet.json [address] 10.5

# Get mining info
cargo run --release mining

File: wallet.json
   Address: 17GQtK9p1pu4aLRLqN9AaY68e5n2TVA7pH

   File: wallet2.json
   Address: 1CpD95owzVQ5BZhsYLssQpj4Sa6ufjtGuL