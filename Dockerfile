# Phase 1: The Build Environment
FROM rust:1.75-slim-bookworm as builder
WORKDIR /usr/src/app

# Install the C-compilers and Protobuf tools needed to compile the gRPC bridge
RUN apt-get update && apt-get install -y pkg-config libssl-dev protobuf-compiler curl

# Copy your entire repository (including the brain/proto files)
COPY . .

# Compile the enterprise gateway
RUN cargo build --release

# Phase 2: The Production Environment (Ultra-lightweight)
FROM debian:bookworm-slim
WORKDIR /app

# Install the missing C-libraries: OpenSSL and the crucial libgomp1 for ONNX
RUN apt-get update && apt-get install -y libssl3 libgomp1 ca-certificates && rm -rf /var/lib/apt/lists/*

# Copy only the final compiled binary from Phase 1
COPY --from=builder /usr/src/app/target/release/aether_os /usr/local/bin/aether_os

# Expose the internal port
EXPOSE 3000

# Ignition
CMD ["aether_os"]