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
# 🚀 NumiCoin Blockchain - Deployment Guide

## Quick Start for Production Launch

### 1. Prerequisites
```bash
# Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# Clone the repository
git clone https://github.com/your-repo/numicoin.git
cd numicoin/core
```

### 2. Build for Production
```bash
# Build optimized release version
cargo build --release

# Verify the build
./target/release/numi-core --version
```

### 3. Initialize the Blockchain
```bash
# Create production configuration
./target/release/numi-core create-config --output numi-production.toml --env production

# Initialize blockchain with production settings
./target/release/numi-core init --force --config numi-production.toml
```

### 4. Start the Node
```bash
# Start full node with mining enabled
./target/release/numi-core start \
  --enable-mining \
  --mining-threads 4 \
  --config numi-production.toml
```

## Seed Node Deployment

### 1. Server Requirements
- **OS**: Ubuntu 20.04+ or CentOS 8+
- **CPU**: 4+ cores
- **RAM**: 8GB+
- **Storage**: 50GB+ SSD
- **Network**: 100Mbps+ bandwidth

### 2. Firewall Configuration
```bash
# Allow blockchain network port
sudo ufw allow 8333/tcp

# Allow RPC port (if public)
sudo ufw allow 8080/tcp

# Enable firewall
sudo ufw enable
```

### 3. Systemd Service Setup
```bash
# Create service file
sudo tee /etc/systemd/system/numi-node.service > /dev/null <<EOF
[Unit]
Description=NumiCoin Blockchain Node
After=network.target

[Service]
Type=simple
User=numi
WorkingDirectory=/opt/numicoin
ExecStart=/opt/numicoin/target/release/numi-core start --enable-mining --config /opt/numicoin/numi-production.toml
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
EOF

# Enable and start service
sudo systemctl enable numi-node
sudo systemctl start numi-node
```

## Monitoring Setup

### 1. Health Check Script
```bash
#!/bin/bash
# health-check.sh

NODE_STATUS=$(curl -s http://localhost:8080/status)
if [[ $? -eq 0 ]]; then
    echo "Node is healthy: $(date)"
    echo "$NODE_STATUS" | jq '.'
else
    echo "Node is down: $(date)"
    # Send alert
    curl -X POST "your-webhook-url" -d "Node is down"
fi
```

### 2. Log Monitoring
```bash
# View logs
sudo journalctl -u numi-node -f

# Check for errors
sudo journalctl -u numi-node --since "1 hour ago" | grep ERROR
```

## Network Configuration

### 1. Bootstrap Nodes
Update your `numi-production.toml` with seed node addresses:
```toml
[network]
bootstrap_nodes = [
    "/ip4/seed1.numicoin.com/tcp/8333",
    "/ip4/seed2.numicoin.com/tcp/8333",
    "/ip4/seed3.numicoin.com/tcp/8333"
]
```

### 2. DNS Configuration
Set up DNS records for your seed nodes:
```
seed1.numicoin.com  A  YOUR_IP_1
seed2.numicoin.com  A  YOUR_IP_2
seed3.numicoin.com  A  YOUR_IP_3
```

## Security Hardening

### 1. User Setup
```bash
# Create dedicated user
sudo useradd -r -s /bin/false numi
sudo mkdir -p /opt/numicoin
sudo chown numi:numi /opt/numicoin
```

### 2. SSL/TLS for RPC
```bash
# Generate SSL certificate
openssl req -x509 -newkey rsa:4096 -keyout key.pem -out cert.pem -days 365 -nodes

# Update RPC configuration
[security]
require_https = true
ssl_cert_path = "/path/to/cert.pem"
ssl_key_path = "/path/to/key.pem"
```

### 3. API Key Management
```bash
# Generate secure API key
openssl rand -hex 32

# Set environment variable
export NUMI_ADMIN_KEY="your-generated-key"
```

## Backup Strategy

### 1. Automated Backups
```bash
#!/bin/bash
# backup.sh

BACKUP_DIR="/backup/numicoin"
DATE=$(date +%Y%m%d_%H%M%S)

# Stop node
sudo systemctl stop numi-node

# Create backup
tar -czf "$BACKUP_DIR/numicoin_$DATE.tar.gz" /opt/numicoin/data

# Start node
sudo systemctl start numi-node

# Clean old backups (keep 7 days)
find $BACKUP_DIR -name "numicoin_*.tar.gz" -mtime +7 -delete
```

### 2. Cron Job Setup
```bash
# Add to crontab
0 2 * * * /opt/numicoin/backup.sh
```

## Performance Tuning

### 1. System Limits
```bash
# Increase file descriptor limits
echo "numi soft nofile 65536" | sudo tee -a /etc/security/limits.conf
echo "numi hard nofile 65536" | sudo tee -a /etc/security/limits.conf
```

### 2. Network Optimization
```bash
# Optimize network settings
echo 'net.core.rmem_max = 16777216' | sudo tee -a /etc/sysctl.conf
echo 'net.core.wmem_max = 16777216' | sudo tee -a /etc/sysctl.conf
sudo sysctl -p
```

## Troubleshooting

### Common Issues

1. **Port Already in Use**
```bash
# Check what's using the port
sudo netstat -tulpn | grep :8333
sudo lsof -i :8333
```

2. **Permission Denied**
```bash
# Fix permissions
sudo chown -R numi:numi /opt/numicoin
sudo chmod 755 /opt/numicoin
```

3. **Out of Memory**
```bash
# Check memory usage
free -h
# Consider reducing mining threads
```

4. **Storage Full**
```bash
# Check disk usage
df -h
# Clean old data if needed
```

### Log Analysis
```bash
# View recent errors
sudo journalctl -u numi-node --since "1 hour ago" | grep -i error

# Monitor performance
sudo journalctl -u numi-node -f | grep -E "(block|transaction|peer)"
```

## Launch Checklist

- [ ] Production build completed
- [ ] Configuration files created
- [ ] Firewall configured
- [ ] Systemd service installed
- [ ] Monitoring setup
- [ ] Backup strategy implemented
- [ ] SSL certificates generated
- [ ] API keys configured
- [ ] Performance tuning applied
- [ ] Health checks working
- [ ] Log monitoring active
- [ ] Team notified of launch

## Support Contacts

- **Technical Issues**: tech-support@numicoin.com
- **Security Issues**: security@numicoin.com
- **Community Support**: community@numicoin.com

## Emergency Procedures

### Node Down
1. Check system resources
2. Review logs for errors
3. Restart service: `sudo systemctl restart numi-node`
4. If persistent, restore from backup

### Network Issues
1. Check firewall settings
2. Verify DNS resolution
3. Test connectivity to peers
4. Update bootstrap nodes if needed

### Security Breach
1. Immediately stop the node
2. Isolate the system
3. Review logs for intrusion
4. Contact security team
5. Restore from clean backup

---

**Ready for Launch! 🚀**

The NumiCoin blockchain is now ready for production deployment. Follow this guide carefully and ensure all components are properly configured before going live. 

# 💰 NumiCoin Transaction Fee Structure - People's Blockchain

## 🎯 Goal: Ultra-Low Fees for Everyone

NumiCoin is designed as a **people's blockchain** with the goal of making cryptocurrency accessible to everyone, regardless of their financial situation. This means keeping transaction fees as low as possible while still maintaining network security.
