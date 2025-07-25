# NumiCoin Testnet

This directory contains the NumiCoin testnet setup with full cryptographic security including Dilithium3 signatures and Argon2id proof-of-work.

## Quick Start

1. **Start the testnet node:**
   ```bash
   ./start-testnet.sh
   ```

2. **Monitor the testnet:**
   ```bash
   ./monitor.sh
   ```

3. **Use the faucet:**
   ```bash
   ./faucet.sh <recipient_address> <amount>
   ```

## Cryptographic Security

This testnet implements the same security standards as mainnet:

- **Dilithium3 Signatures**: Post-quantum secure digital signatures for all transactions
- **Argon2id Proof-of-Work**: Memory-hard proof-of-work algorithm
- **Blake3 Hashing**: Fast cryptographic hashing for block and transaction IDs
- **Kyber KEM**: Post-quantum key encapsulation for secure communication

## Testnet Configuration

- **Block Time**: 15 seconds
- **Difficulty Adjustment**: Every 30 blocks
- **Max Block Size**: 1MB
- **Max Transactions per Block**: 500
- **Min Transaction Fee**: 500 smallest units (0.0000005 NUMI)
- **RPC Port**: 8081
- **P2P Port**: 8334

## Pre-funded Accounts

The testnet includes several pre-funded accounts for testing:

1. **Developer Account**: 100,000 NUMI
2. **Faucet Account**: 500,000 NUMI
3. **Validator Account**: 200,000 NUMI
4. **User Account**: 50,000 NUMI

## Network Features

- **P2P Networking**: libp2p-based peer-to-peer communication
- **RPC API**: RESTful API for blockchain interaction
- **Mempool**: Transaction pool with fee-based prioritization
- **Mining**: CPU-based mining with configurable threads


## Security Features

- **Rate Limiting**: Protection against spam and DoS attacks
- **IP Blocking**: Automatic blocking of malicious peers
- **Transaction Validation**: Comprehensive transaction verification
- **Block Validation**: Full block structure and signature verification
- **Replay Protection**: Nonce-based transaction replay protection

## Monitoring and Maintenance

- **Automatic Backups**: Every 12 hours
- **Log Rotation**: Automatic log management
- **Health Checks**: Built-in node health monitoring
- **Performance Metrics**: Real-time performance tracking

## Troubleshooting

If you encounter issues:

1. Check the logs in `../testnet-data/logs/`
2. Verify the node is not already running
3. Ensure ports 8081 and 8334 are available
4. Check system resources (CPU, memory, disk)

## Development

For development and testing:

```bash
# Generate new keys
./core/target/release/numi-core generate-key --output new_key.json

# Submit a transaction
./core/target/release/numi-core submit --from-key new_key.json --to <recipient> --amount <amount>

# Check balance
./core/target/release/numi-core balance --address <address>
```
