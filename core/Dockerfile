# NumiCoin Mining Docker Image
FROM rust:1.75-slim as builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Set working directory
WORKDIR /app

# Copy source code
COPY . .

# Build the application
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN useradd -m -u 1000 numicoin

# Set working directory
WORKDIR /app

# Copy binary from builder stage
COPY --from=builder /app/target/release/numi-core /app/numi-core

# Copy configuration files
COPY --from=builder /app/numi.toml /app/numi.toml
COPY --from=builder /app/mobile.toml /app/mobile.toml

# Copy scripts
COPY --from=builder /app/setup-miner.sh /app/setup-miner.sh
COPY --from=builder /app/start-mining.sh /app/start-mining.sh
COPY --from=builder /app/monitor.sh /app/monitor.sh
COPY --from=builder /app/dashboard.html /app/dashboard.html

# Make scripts executable
RUN chmod +x /app/setup-miner.sh /app/start-mining.sh /app/monitor.sh

# Create data directory
RUN mkdir -p /app/data && chown -R numicoin:numicoin /app

# Switch to non-root user
USER numicoin

# Expose ports
EXPOSE 8333 8080

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:8080/health || exit 1

# Default command
CMD ["/app/numi-core", "start", "--enable-mining"] 