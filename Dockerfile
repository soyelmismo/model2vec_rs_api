# ─── Stage 1: Builder ────────────────────────────────────────────────────────
FROM rust:slim AS builder

# Install only what's needed to compile
RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config \
        libssl-dev \
        ca-certificates \
        g++ \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Cache dependency layer: copy manifests, build a dummy main, then overwrite.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main(){}' > src/main.rs \
    && cargo build --release \
    && rm src/main.rs

# Build the real binary
COPY src ./src
RUN touch src/main.rs \
    && cargo build --release

# ─── Stage 2: Distroless runtime ─────────────────────────────────────────────
# gcr.io/distroless/cc-debian13 contains glibc + libgcc — nothing else.
# Perfect for statically/dynamically linked Rust binaries.
FROM gcr.io/distroless/cc-debian13:nonroot

# Copy CA certificates so HTTPS (HuggingFace downloads) works at runtime
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/

# Copy the compiled binary
COPY --from=builder /build/target/release/model2vec-api /model2vec-api

# Models are mounted at /models — users can bind-mount local model directories.
VOLUME ["/models"]

EXPOSE 8080

ENTRYPOINT ["/model2vec-api"]
