FROM rust:bookworm AS builder

WORKDIR /code

# Install runtime dependencies
RUN apt-get update -y && \
    apt-get install -y pkg-config libssl-dev && \
    apt-get install -y cmake protobuf-compiler && \
    apt-get clean && \
    rm -rf /var/lib/apt/lists/*

# Copy your code and dependencies
COPY . /code

# Build application in release mode
WORKDIR /code/qdrant-search

# Build application with release flag
RUN cargo build --release --bin server

# Final image - smaller runtime environment
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update -y && \
    apt-get install -y openssl libssl-dev ca-certificates curl && \
    apt-get clean && \
    rm -rf /var/lib/apt/lists/*

# Set up runtime environment
WORKDIR /app

# Expose application port
EXPOSE 3009

# Add the compiled binary
COPY --from=builder /code/target/release/server /app/qdrant-search

ENV RUST_LOG=info

# Run the binary
CMD ["/app/qdrant-search"]