# 🚀 NumiCoin Deployment Guide

Multiple ways to deploy and participate in NumiCoin mining!

## 🐳 Docker Deployment (Recommended)

### Quick Start with Docker

```bash
# 1. Clone the repository
git clone https://github.com/your-repo/numicoin.git
cd numicoin/core

# 2. Build and run with Docker Compose
docker-compose up -d

# 3. Access the dashboard
open http://localhost:3000
```

### Manual Docker Build

```bash
# Build the image
docker build -t numicoin-miner .

# Run the container
docker run -d \
  --name numicoin-miner \
  -p 8333:8333 \
  -p 8080:8080 \
  -v numicoin-data:/app/data \
  numicoin-miner
```

## ☁️ Cloud Deployment

### AWS EC2

```bash
# Launch EC2 instance (t3.medium or larger)
# Connect via SSH and run:

# Install Docker
curl -fsSL https://get.docker.com -o get-docker.sh
sh get-docker.sh

# Clone and deploy
git clone https://github.com/your-repo/numicoin.git
cd numicoin/core
docker-compose up -d
```

### Google Cloud Platform

```bash
# Create VM instance
gcloud compute instances create numicoin-miner \
  --machine-type=e2-medium \
  --zone=us-central1-a \
  --image-family=debian-11 \
  --image-project=debian-cloud

# Install Docker and deploy
gcloud compute ssh numicoin-miner
# Then follow AWS instructions above
```

### DigitalOcean Droplet

```bash
# Create droplet with Docker image
# Or create regular droplet and install Docker

# Deploy using docker-compose
git clone https://github.com/your-repo/numicoin.git
cd numicoin/core
docker-compose up -d
```

## 🏠 Home Server Deployment

### Raspberry Pi 4

```bash
# Install Docker on Raspberry Pi
curl -fsSL https://get.docker.com -o get-docker.sh
sh get-docker.sh

# Deploy with reduced resources
docker run -d \
  --name numicoin-miner \
  -p 8333:8333 \
  -p 8080:8080 \
  -v numicoin-data:/app/data \
  --cpus=2 \
  --memory=2g \
  numicoin-miner
```

### Home Server (Linux)

```bash
# Install Docker
sudo apt update
sudo apt install docker.io docker-compose

# Deploy
git clone https://github.com/your-repo/numicoin.git
cd numicoin/core
sudo docker-compose up -d
```

## 📱 Mobile/Edge Deployment

### Android (Termux)

```bash
# Install Termux and run:
pkg update && pkg upgrade
pkg install rust git

# Clone and build
git clone https://github.com/your-repo/numicoin.git
cd numicoin/core
cargo build --release

# Run with minimal resources
./target/release/numi-core start --enable-mining --mining-threads 1 --config mobile.toml
```

### iOS (iSH)

```bash
# Install iSH from App Store
# Install Alpine Linux packages:
apk add rust cargo git

# Follow Android instructions above
```

## 🌐 Kubernetes Deployment

### Basic Kubernetes Deployment

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: numicoin-miner
spec:
  replicas: 1
  selector:
    matchLabels:
      app: numicoin-miner
  template:
    metadata:
      labels:
        app: numicoin-miner
    spec:
      containers:
      - name: numicoin-miner
        image: numicoin-miner:latest
        ports:
        - containerPort: 8333
        - containerPort: 8080
        resources:
          requests:
            memory: "512Mi"
            cpu: "500m"
          limits:
            memory: "2Gi"
            cpu: "2000m"
        volumeMounts:
        - name: numicoin-data
          mountPath: /app/data
      volumes:
      - name: numicoin-data
        persistentVolumeClaim:
          claimName: numicoin-pvc
---
apiVersion: v1
kind: Service
metadata:
  name: numicoin-service
spec:
  selector:
    app: numicoin-miner
  ports:
  - port: 8333
    targetPort: 8333
  - port: 8080
    targetPort: 8080
  type: LoadBalancer
```

## 🔧 Configuration Options

### Environment Variables

```bash
# Set custom configuration
export RUST_LOG=info
export NUMICOIN_DATA_DIR=/custom/data/path
export NUMICOIN_RPC_PORT=8081
export NUMICOIN_NETWORK_PORT=8334
```

### Custom Configuration Files

```bash
# Create custom config
cp numi.toml custom-config.toml

# Edit custom-config.toml with your settings
# Then run with:
./target/release/numi-core start --config custom-config.toml --enable-mining
```

## 📊 Monitoring & Observability

### Built-in Monitoring

```bash
# Check node status
curl http://localhost:8080/status

# Health check
curl http://localhost:8080/health

# Metrics (if available)
curl http://localhost:8080/metrics
```

### External Monitoring

```bash
# Prometheus configuration
cat > prometheus.yml << EOF
global:
  scrape_interval: 15s

scrape_configs:
  - job_name: 'numicoin'
    static_configs:
      - targets: ['localhost:8080']
EOF

# Run Prometheus
docker run -d \
  --name prometheus \
  -p 9090:9090 \
  -v $(pwd)/prometheus.yml:/etc/prometheus/prometheus.yml \
  prom/prometheus
```

### Grafana Dashboard

```bash
# Run Grafana
docker run -d \
  --name grafana \
  -p 3001:3000 \
  grafana/grafana

# Access Grafana at http://localhost:3001
# Add Prometheus as data source
# Import dashboard templates
```

## 🔒 Security Considerations

### Firewall Configuration

```bash
# UFW (Ubuntu)
sudo ufw allow 8333/tcp  # P2P network
sudo ufw allow 8080/tcp  # RPC (if public)
sudo ufw enable

# iptables
sudo iptables -A INPUT -p tcp --dport 8333 -j ACCEPT
sudo iptables -A INPUT -p tcp --dport 8080 -j ACCEPT
```

### SSL/TLS (for RPC)

```bash
# Generate certificates
openssl req -x509 -newkey rsa:4096 -keyout key.pem -out cert.pem -days 365 -nodes

# Run with HTTPS
./target/release/numi-core start --enable-mining --rpc-port 8443 --ssl-cert cert.pem --ssl-key key.pem
```

## 🚀 Performance Optimization

### Resource Limits

```bash
# Docker with resource limits
docker run -d \
  --name numicoin-miner \
  --cpus=4 \
  --memory=4g \
  --memory-swap=0 \
  numicoin-miner
```

### CPU Affinity

```bash
# Pin to specific CPU cores
docker run -d \
  --name numicoin-miner \
  --cpuset-cpus="0,1,2,3" \
  numicoin-miner
```

## 📈 Scaling

### Multiple Miners

```bash
# Run multiple instances on different ports
docker run -d --name numicoin-miner-1 -p 8333:8333 -p 8080:8080 numicoin-miner
docker run -d --name numicoin-miner-2 -p 8334:8333 -p 8081:8080 numicoin-miner
docker run -d --name numicoin-miner-3 -p 8335:8333 -p 8082:8080 numicoin-miner
```

### Load Balancing

```bash
# Use nginx for RPC load balancing
docker run -d \
  --name nginx-lb \
  -p 80:80 \
  -v $(pwd)/nginx.conf:/etc/nginx/nginx.conf \
  nginx
```

## 🆘 Troubleshooting

### Common Issues

1. **Port already in use**
   ```bash
   # Check what's using the port
   sudo netstat -tulpn | grep :8333
   
   # Kill the process
   sudo kill -9 <PID>
   ```

2. **Permission denied**
   ```bash
   # Fix permissions
   sudo chown -R $USER:$USER /app/data
   chmod +x target/release/numi-core
   ```

3. **Out of memory**
   ```bash
   # Reduce mining threads
   ./target/release/numi-core start --enable-mining --mining-threads 2
   ```

### Logs and Debugging

```bash
# View logs
docker logs numicoin-miner

# Follow logs
docker logs -f numicoin-miner

# Debug mode
RUST_LOG=debug ./target/release/numi-core start --enable-mining
```

## 🎯 Deployment Checklist

- [ ] Choose deployment method
- [ ] Set up environment
- [ ] Configure networking
- [ ] Set resource limits
- [ ] Configure monitoring
- [ ] Set up backups
- [ ] Test deployment
- [ ] Monitor performance
- [ ] Set up alerts

Happy deploying! 🚀 