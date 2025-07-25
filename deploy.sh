#!/bin/bash

# Numi Blockchain Production Deployment Script
# This script sets up and deploys the Numi blockchain in production environments

set -e  # Exit on any error

# Configuration
NUMI_USER="numi"
NUMI_HOME="/opt/numi"
NUMI_DATA="/var/lib/numi"
NUMI_LOGS="/var/log/numi"
SERVICE_NAME="numi-node"
RPC_PORT="8080"
P2P_PORT="8333"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Logging functions
log_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

log_step() {
    echo -e "${BLUE}[STEP]${NC} $1"
}

# Check if running as root
check_root() {
    if [[ $EUID -ne 0 ]]; then
        log_error "This script must be run as root (use sudo)"
        exit 1
    fi
}

# Install system dependencies
install_dependencies() {
    log_step "Installing system dependencies..."
    
    # Update package lists
    apt-get update -qq
    
    # Install required packages
    apt-get install -y \
        curl \
        build-essential \
        pkg-config \
        libssl-dev \
        ca-certificates \
        gnupg \
        lsb-release \
        ufw \
        htop \
        git
    
    log_info "System dependencies installed"
}

# Install Rust
install_rust() {
    log_step "Installing Rust toolchain..."
    
    # Check if Rust is already installed
    if command -v rustc &> /dev/null; then
        log_info "Rust is already installed: $(rustc --version)"
        return
    fi
    
    # Install Rust as numi user
    sudo -u $NUMI_USER bash -c "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y"
    sudo -u $NUMI_USER bash -c "source ~/.cargo/env && rustup update"
    
    log_info "Rust toolchain installed"
}

# Create system user
create_user() {
    log_step "Creating system user..."
    
    if id "$NUMI_USER" &>/dev/null; then
        log_info "User $NUMI_USER already exists"
    else
        useradd --system --shell /bin/bash --home $NUMI_HOME --create-home $NUMI_USER
        log_info "Created user $NUMI_USER"
    fi
}

# Create directories
create_directories() {
    log_step "Creating directories..."
    
    mkdir -p $NUMI_HOME $NUMI_DATA $NUMI_LOGS
#    chown -R $NUMI_USER:$NUMI_USER $NUMI_HOME $NUMI_DATA $NUMI_LOGS
    chmod 755 $NUMI_HOME $NUMI_DATA
    chmod 750 $NUMI_LOGS
    
    log_info "Directories created and permissions set"
}

# Build Numi blockchain
build_numi() {
    log_step "Building Numi blockchain..."
    
    cd $NUMI_HOME
    
    # Clone or update repository
    if [ -d "numi-core" ]; then
        log_info "Updating existing repository..."
        cd numi-core
        sudo -u $NUMI_USER git pull
    else
        log_info "Cloning repository..."
        sudo -u $NUMI_USER git clone https://github.com/numi-blockchain/numi-core.git
        cd numi-core
    fi
    
    # Build in release mode
    cd core
    sudo -u $NUMI_USER bash -c "source ~/.cargo/env && cargo build --release"
    
    # Copy binary to system location
    cp target/release/numi-node /usr/local/bin/
    chmod +x /usr/local/bin/numi-node
    
    log_info "Numi blockchain built and installed"
}

# Configure firewall
configure_firewall() {
    log_step "Configuring firewall..."
    
    # Enable UFW
    ufw --force enable
    
    # Allow SSH (assuming standard port)
    ufw allow 22/tcp
    
    # Allow Numi ports
    ufw allow $RPC_PORT/tcp comment "Numi RPC"
    ufw allow $P2P_PORT/tcp comment "Numi P2P"
    
    # Set default policies
    ufw default deny incoming
    ufw default allow outgoing
    
    log_info "Firewall configured"
}

# Create systemd service
create_service() {
    log_step "Creating systemd service..."
    
    cat > /etc/systemd/system/$SERVICE_NAME.service << EOF
[Unit]
Description=Numi Blockchain Node
Documentation=https://docs.numi.network
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=$NUMI_USER
Group=$NUMI_USER
WorkingDirectory=$NUMI_HOME
ExecStart=/usr/local/bin/numi-node rpc --port $RPC_PORT --data-dir $NUMI_DATA
ExecReload=/bin/kill -HUP \$MAINPID
Restart=always
RestartSec=10
TimeoutStopSec=30

# Security settings
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
ReadWritePaths=$NUMI_DATA $NUMI_LOGS
PrivateTmp=yes
ProtectKernelTunables=yes
ProtectKernelModules=yes
ProtectControlGroups=yes

# Resource limits
LimitNOFILE=65536
LimitNPROC=32768

# Environment
Environment=RUST_LOG=info
Environment=NUMI_DATA_DIR=$NUMI_DATA

# Logging
StandardOutput=journal
StandardError=journal
SyslogIdentifier=$SERVICE_NAME

[Install]
WantedBy=multi-user.target
EOF

    # Reload systemd and enable service
    systemctl daemon-reload
    systemctl enable $SERVICE_NAME
    
    log_info "Systemd service created and enabled"
}

# Initialize blockchain
initialize_blockchain() {
    log_step "Initializing blockchain..."
    
    # Initialize as numi user
    sudo -u $NUMI_USER /usr/local/bin/numi-node init --data-dir $NUMI_DATA
    
    log_info "Blockchain initialized"
}

# Configure log rotation
configure_logging() {
    log_step "Configuring log rotation..."
    
    cat > /etc/logrotate.d/numi << EOF
$NUMI_LOGS/*.log {
    daily
    rotate 30
    compress
    delaycompress
    missingok
    notifempty
    create 640 $NUMI_USER $NUMI_USER
    postrotate
        systemctl reload $SERVICE_NAME > /dev/null 2>&1 || true
    endscript
}
EOF

    log_info "Log rotation configured"
}

# Create monitoring script
create_monitoring() {
    log_step "Creating monitoring script..."
    
    cat > $NUMI_HOME/monitor.sh << 'EOF'
#!/bin/bash

# Numi Node Monitoring Script

NUMI_RPC_URL="http://localhost:8080"
NUMI_DATA="/var/lib/numi"
LOG_FILE="/var/log/numi/monitor.log"

# Function to log with timestamp
log_message() {
    echo "$(date '+%Y-%m-%d %H:%M:%S') - $1" | tee -a "$LOG_FILE"
}

# Check if service is running
check_service() {
    if systemctl is-active --quiet numi-node; then
        log_message "✅ Numi service is running"
        return 0
    else
        log_message "❌ Numi service is not running"
        return 1
    fi
}

# Check RPC API
check_api() {
    if curl -s --connect-timeout 5 "$NUMI_RPC_URL/health" > /dev/null; then
        log_message "✅ RPC API is responding"
        return 0
    else
        log_message "❌ RPC API is not responding"
        return 1
    fi
}

# Check disk space
check_disk_space() {
    local usage=$(df "$NUMI_DATA" | awk 'NR==2 {print $5}' | sed 's/%//')
    if [ "$usage" -lt 90 ]; then
        log_message "✅ Disk usage: ${usage}%"
        return 0
    else
        log_message "⚠️ High disk usage: ${usage}%"
        return 1
    fi
}

# Get blockchain status
get_status() {
    local status=$(curl -s --connect-timeout 5 "$NUMI_RPC_URL/status" 2>/dev/null | jq -r '.data.total_blocks // "unknown"' 2>/dev/null)
    log_message "📊 Current block height: $status"
}

# Main monitoring
main() {
    log_message "🔍 Starting monitoring check..."
    
    local issues=0
    
    check_service || ((issues++))
    check_api || ((issues++))
    check_disk_space || ((issues++))
    get_status
    
    if [ $issues -eq 0 ]; then
        log_message "✅ All checks passed"
    else
        log_message "⚠️ Found $issues issues"
    fi
    
    log_message "🔍 Monitoring check completed"
}

main "$@"
EOF

    chmod +x $NUMI_HOME/monitor.sh
#    chown $NUMI_USER:$NUMI_USER $NUMI_HOME/monitor.sh
    
    # Add to cron for periodic monitoring
    echo "*/5 * * * * $NUMI_USER $NUMI_HOME/monitor.sh" > /etc/cron.d/numi-monitor
    
    log_info "Monitoring script created"
}

# Start services
start_services() {
    log_step "Starting services..."
    
    systemctl start $SERVICE_NAME
    systemctl status $SERVICE_NAME --no-pager
    
    log_info "Services started"
}

# Display final information
display_info() {
    log_step "Deployment completed successfully! 🎉"
    
    echo
    echo "==================== NUMI BLOCKCHAIN NODE ===================="
    echo
    echo "🌐 RPC API: http://localhost:$RPC_PORT"
    echo "🔗 P2P Port: $P2P_PORT"
    echo "📁 Data Directory: $NUMI_DATA"
    echo "📋 Logs: journalctl -u $SERVICE_NAME -f"
    echo "🔧 Config: /etc/systemd/system/$SERVICE_NAME.service"
    echo
    echo "==================== USEFUL COMMANDS ========================="
    echo
    echo "# Check service status"
    echo "sudo systemctl status $SERVICE_NAME"
    echo
    echo "# View logs"
    echo "sudo journalctl -u $SERVICE_NAME -f"
    echo
    echo "# Restart service"
    echo "sudo systemctl restart $SERVICE_NAME"
    echo
    echo "# Check blockchain status"
    echo "curl http://localhost:$RPC_PORT/status"
    echo
    echo "# Run monitoring check"
    echo "sudo -u $NUMI_USER $NUMI_HOME/monitor.sh"
    echo
    echo "==================== SECURITY NOTES =========================="
    echo
    echo "🔒 Firewall is configured (UFW enabled)"
    echo "🔐 Service runs as non-root user ($NUMI_USER)"
    echo "📊 Monitoring is set up (check every 5 minutes)"
    echo "🗂️ Log rotation is configured (30 days retention)"
    echo
    echo "⚠️ Remember to:"
    echo "   - Change default passwords/keys"
    echo "   - Set up proper backup procedures"
    echo "   - Configure monitoring alerts"
    echo "   - Review security settings"
    echo
    echo "=============================================================="
}

# Main deployment function
main() {
    log_info "Starting Numi Blockchain production deployment..."
    
    check_root
    install_dependencies
    create_user
    create_directories
    install_rust
    build_numi
    configure_firewall
    initialize_blockchain
    create_service
    configure_logging
    create_monitoring
    start_services
    display_info
    
    log_info "Deployment completed successfully! 🚀"
}

# Run main function
main "$@"