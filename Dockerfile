# HSM-II Production Docker Image
# Multi-stage build for minimal final image

# Build stage
FROM rust:1.81-slim-bookworm AS builder

WORKDIR /build

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy manifests first for better caching
COPY Cargo.toml Cargo.lock ./
COPY integrations/hermes/hermes-bridge/Cargo.toml integrations/hermes/hermes-bridge/

# Copy source code
COPY src ./src
COPY integrations/hermes/hermes-bridge/src ./integrations/hermes/hermes-bridge/src

# Build release binary
RUN cargo build --release --features gpu

# Runtime stage
FROM debian:bookworm-slim

WORKDIR /app

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    curl \
    jq \
    git \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN groupadd -r hsm && useradd -r -g hsm hsm

# Copy binary from builder
COPY --from=builder /build/target/release/hyper-stigmergy /usr/local/bin/
COPY --from=builder /build/target/release/batch_experiment /usr/local/bin/
COPY --from=builder /build/target/release/investigate /usr/local/bin/

# Copy configuration files
COPY ops/config ./config
COPY banner.txt ./

# Set up directories
RUN mkdir -p /app/data /app/logs /app/experiments && \
    chown -R hsm:hsm /app

# Switch to non-root user
USER hsm

# Expose ports
# 8080: HTTP API
# 9000: Prometheus metrics
# 9090: gRPC (future)
EXPOSE 8080 9000 9090

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:8080/health || exit 1

# Environment variables (override at runtime)
ENV RUST_LOG=info
ENV HSM_DATA_DIR=/app/data
ENV HSM_LOG_DIR=/app/logs

# Default command
CMD ["hyper-stigmergy"]
