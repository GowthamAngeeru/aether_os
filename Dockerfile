# Phase 1: The Build Environment
FROM rust:slim-bookworm as builder
WORKDIR /usr/src/app

# Install build tools
RUN apt-get update && apt-get install -y pkg-config libssl-dev protobuf-compiler
COPY . .
RUN cargo build --release

# Phase 2: The Production Environment (Self-Contained & Immutable)
FROM debian:bookworm-slim
WORKDIR /app

# Install required C-libraries AND curl
RUN apt-get update && apt-get install -y libssl3 libgomp1 ca-certificates curl && rm -rf /var/lib/apt/lists/*

# Download the official ONNX engine and embed it permanently into the Linux core libraries
RUN curl -sLO https://github.com/microsoft/onnxruntime/releases/download/v1.17.1/onnxruntime-linux-x64-1.17.1.tgz && \
    tar -xzf onnxruntime-linux-x64-1.17.1.tgz && \
    mv onnxruntime-linux-x64-1.17.1/lib/libonnxruntime.so* /usr/lib/ && \
    rm -rf onnxruntime-linux-x64-1.17.1*

# Copy the compiled Gateway binary from Phase 1
COPY --from=builder /usr/src/app/target/release/aether_os /usr/local/bin/aether_os

EXPOSE 3000
CMD ["aether_os"]