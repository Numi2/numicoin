# 🚀 NumiCoin Testnet Mining Guide

Welcome to the NumiCoin testnet! This guide will help you get started with mining and participating in our quantum-safe blockchain.

## 📋 Prerequisites

- **Operating System**: Linux, macOS, or Windows
- **RAM**: Minimum 4GB, Recommended 8GB+
- **Storage**: At least 10GB free space
- **Internet**: Stable broadband connection
- **Rust**: Latest stable version (1.70+)

## 🛠️ Quick Setup

### 1. Install Rust (if not already installed)

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
```

### 2. Clone and Build

```bash
git clone https://github.com/your-repo/numicoin.git
cd numicoin/core
cargo build --release
```

### 3. Initialize Your Node

```bash
./target/release/numi-core init --force
```

## ⛏️ Start Mining

### Option 1: Simple Mining (Recommended for beginners)

```bash
# Start mining with default settings
./target/release/numi-core start --enable-mining
```

### Option 2: Custom Mining Configuration

```bash
# Start with custom thread count
./target/release/numi-core start --enable-mining --mining-threads 4

# Start with custom RPC port
./target/release/numi-core start --enable-mining --rpc-port 8081
```

### Option 3: Mining Only (without full node)

```bash
# Mine a single block
./target/release/numi-core mine --threads 4 --timeout 300
```

## 🔑 Generate Your Wallet

```bash
# Generate a new wallet
./target/release/numi-core generate-key --output my-wallet.json

# View your wallet address
cat my-wallet.json | jq -r '.public_key | map(sprintf("%02x"; .)) | join("")' | head -c 128
```

## 💰 Check Your Balance

```bash
# Replace with your wallet address
./target/release/numi-core balance --address YOUR_WALLET_ADDRESS
```

## 📊 Monitor Your Node

```bash
# Check node status
./target/release/numi-core status

# Check blockchain status
./target/release/numi-core status --detailed
```

## 🔄 Submit Transactions

```bash
# Send NUMI to another address
./target/release/numi-core submit \
  --from-key my-wallet.json \
  --to RECIPIENT_ADDRESS \
  --amount 1000000 \
  --memo "Test transaction"
```

## 🌐 Network Configuration

### Default Testnet Settings
- **Network Port**: 8333
- **RPC Port**: 8080
- **Block Time**: ~10-15 seconds
- **Difficulty**: Auto-adjusting
- **Mining Reward**: 100 NUMI per block

### Connect to Testnet Peers

The node will automatically discover peers using mDNS. To connect to specific peers:

```bash
# Start with specific bootstrap nodes
./target/release/numi-core start --enable-mining \
  --config testnet.toml
```

## 📈 Mining Performance Tips

### CPU Optimization
- **Single Core**: Use 1-2 threads
- **Multi-Core**: Use 4-8 threads (depending on CPU)
- **High-End**: Use 8-16 threads

### Memory Settings
- **Low RAM (4GB)**: Use 2-4 threads
- **Medium RAM (8GB)**: Use 4-8 threads
- **High RAM (16GB+)**: Use 8-16 threads

### Example Configurations

```bash
# Low-end system
./target/release/numi-core start --enable-mining --mining-threads 2

# Mid-range system
./target/release/numi-core start --enable-mining --mining-threads 6

# High-end system
./target/release/numi-core start --enable-mining --mining-threads 12
```

## 🔧 Troubleshooting

### Common Issues

1. **"Resource temporarily unavailable"**
   ```bash
   # Kill any existing processes
   pkill -f numi-core
   # Wait a few seconds, then try again
   ```

2. **"Failed to acquire data directory lock"**
   ```bash
   # Remove lock file
   rm -f dev-data/.lock
   ```

3. **"Permission denied"**
   ```bash
   # Make binary executable
   chmod +x target/release/numi-core
   ```

### Performance Issues

1. **Slow mining**: Reduce thread count
2. **High CPU usage**: Adjust mining threads
3. **Memory issues**: Lower thread count or increase system RAM

## 📱 Mobile Mining (Advanced)

For mobile devices or low-power systems:

```bash
# Minimal mining configuration
./target/release/numi-core start \
  --enable-mining \
  --mining-threads 1 \
  --config mobile.toml
```

## 🎯 Mining Pool Setup

### Create a Mining Pool

```bash
# Pool configuration example
./target/release/numi-core start \
  --enable-mining \
  --mining-threads 8 \
  --rpc-port 8080 \
  --listen-addr 0.0.0.0
```

### Join a Pool

```bash
# Connect to pool
./target/release/numi-core start \
  --enable-mining \
  --mining-threads 4 \
  --config pool-client.toml
```

## 🔒 Security Best Practices

1. **Keep your private keys safe**
   ```bash
   # Store keys securely
   chmod 600 my-wallet.json
   ```

2. **Use firewall rules**
   ```bash
   # Allow only necessary ports
   sudo ufw allow 8333/tcp  # Network
   sudo ufw allow 8080/tcp  # RPC (if public)
   ```

3. **Regular backups**
   ```bash
   # Backup your wallet
   cp my-wallet.json my-wallet-backup.json
   
   # Backup blockchain data
   ./target/release/numi-core backup --output ./backup
   ```

## 📊 Monitoring Tools

### Built-in Monitoring
```bash
# Real-time status
watch -n 10 './target/release/numi-core status'

# Detailed statistics
./target/release/numi-core status --detailed
```

### External Monitoring
```bash
# Check if node is running
ps aux | grep numi-core

# Monitor network connections
netstat -tulpn | grep 8333
```

## 🎉 Getting Started Checklist

- [ ] Install Rust
- [ ] Clone repository
- [ ] Build binary
- [ ] Initialize blockchain
- [ ] Generate wallet
- [ ] Start mining
- [ ] Check balance
- [ ] Submit test transaction

## 📞 Support

- **Discord**: [Join our community](https://discord.gg/numicoin)
- **GitHub Issues**: [Report bugs](https://github.com/your-repo/numicoin/issues)
- **Documentation**: [Full docs](https://docs.numicoin.org)

## 🏆 Mining Rewards

- **Block Reward**: 100 NUMI per block
- **Transaction Fees**: Variable based on network load
- **Difficulty**: Adjusts every 20 blocks
- **Halving**: Every 210,000 blocks

Happy mining! 🚀⛏️ 