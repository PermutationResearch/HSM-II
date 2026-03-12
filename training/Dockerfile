# HSM-II Training Dockerfile
# Supports: CPU, CUDA 12.1

FROM nvidia/cuda:12.1.0-runtime-ubuntu22.04

ENV DEBIAN_FRONTEND=noninteractive
ENV RUSTUP_HOME=/usr/local/rustup
ENV CARGO_HOME=/usr/local/cargo
ENV PATH=/usr/local/cargo/bin:$PATH

# Install system dependencies
RUN apt-get update && apt-get install -y \
    build-essential \
    curl \
    git \
    python3 \
    python3-pip \
    htop \
    jq \
    zstd \
    && rm -rf /var/lib/apt/lists/*

# Install Rust
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
RUN chmod -R a+w $RUSTUP_HOME $CARGO_HOME

# Install Ollama
RUN curl -fsSL https://ollama.com/install.sh | sh

# Install Python ML dependencies
RUN pip3 install --no-cache-dir \
    torch torchvision torchaudio --index-url https://download.pytorch.org/whl/cu121 \
    && pip3 install --no-cache-dir \
    numpy pandas matplotlib seaborn plotly wandb tensorboard

# Set working directory
WORKDIR /workspace/hsm2

# Copy project files
COPY Cargo.toml Cargo.lock ./
COPY src ./src

# Build release binary
RUN cargo build --release --bin batch_experiment

# Copy training configs
COPY training ./training

# Expose ports for monitoring
EXPOSE 6006 11434

# Set environment for Ollama
ENV OLLAMA_HOST=0.0.0.0:11434
ENV OLLAMA_NUM_PARALLEL=4
ENV OLLAMA_MAX_LOADED_MODELS=2

# Default command
CMD ["./training/scripts/start_training.sh"]
