# 🚀 Quick Start: NumiCoin Mining

Get started with NumiCoin mining in 3 simple steps!

## ⚡ Super Quick Setup

### 1. Run the Setup Script
```bash
chmod +x setup-miner.sh
./setup-miner.sh
```

This will:
- ✅ Install Rust (if needed)
- ✅ Build the NumiCoin binary
- ✅ Initialize your blockchain
- ✅ Generate your wallet
- ✅ Create mining scripts

### 2. Start Mining
```bash
./start-mining.sh
```

### 3. Monitor Your Node
```bash
./monitor.sh
```

## 🌐 Web Dashboard

Open `dashboard.html` in your browser for a beautiful web interface to monitor your mining node.

## 📱 Mobile/Low-Power Mining

For mobile devices or low-power systems:
```bash
./target/release/numi-core start --enable-mining --mining-threads 1 --config mobile.toml
```

## 🔧 Manual Setup (Advanced)

If you prefer manual setup:

```bash
# 1. Build
cargo build --release

# 2. Initialize
./target/release/numi-core init --force

# 3. Generate wallet
./target/release/numi-core generate-key --output my-wallet.json

# 4. Start mining
./target/release/numi-core start --enable-mining --mining-threads 4
```

## 📊 Useful Commands

```bash
# Check status
./target/release/numi-core status

# Check balance
./target/release/numi-core balance --address YOUR_ADDRESS

# Submit transaction
./target/release/numi-core submit --from-key my-wallet.json --to RECIPIENT --amount 1000000

# Stop mining
pkill -f numi-core
```

## 🎯 Mining Rewards

- **Block Reward**: 100 NUMI per block
- **Block Time**: ~10-15 seconds
- **Difficulty**: Auto-adjusting
- **Halving**: Every 210,000 blocks

## 🔒 Security Tips

1. **Backup your wallet**: `cp miner-wallet.json backup-wallet.json`
2. **Secure your keys**: `chmod 600 miner-wallet.json`
3. **Use firewall**: Only allow ports 8333 (network) and 8080 (RPC)

## 📞 Need Help?

- 📚 **Full Guide**: See `MINING_GUIDE.md`
- 🐛 **Issues**: Check troubleshooting section in the guide
- 💬 **Community**: Join our Discord

## 🏆 Mining Tips

- **CPU Cores**: Use 50% of your CPU cores for mining
- **Memory**: 4GB+ recommended
- **Storage**: 10GB+ free space
- **Network**: Stable internet connection

Happy mining! 🚀⛏️ 