# Numi Blockchain Core
1 NUMI = 100 NANO
curl -s http://127.0.0.1:8082/status | jq .
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