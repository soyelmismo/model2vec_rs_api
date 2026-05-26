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

# Cache dependencies with BuildKit — Docker manages this internally,
# no host mount needed. Portable across machines and CI/CD.
COPY Cargo.toml Cargo.lock ./


COPY src ./src
RUN --mount=type=cache,target=/build/target \
    --mount=type=cache,target=/usr/local/cargo/registry \
    cargo build --release && \
    cp target/release/model2vec-api /model2vec-api

# ─── Stage 2: Distroless runtime ─────────────────────────────────────────────
# gcr.io/distroless/cc-debian13 contains glibc + libgcc — nothing else.
# Perfect for statically/dynamically linked Rust binaries.
FROM gcr.io/distroless/cc-debian13:nonroot

# Copy CA certificates so HTTPS (HuggingFace downloads) works at runtime
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/

# Copy the compiled binary (placed at /model2vec-api by the builder's cp step)
COPY --from=builder /model2vec-api /model2vec-api

# Models are mounted at /models — users can bind-mount local model directories.
VOLUME ["/models"]

EXPOSE 22671

HEALTHCHECK --interval=30s --timeout=5s --start-period=60s --retries=3 \
    CMD ["/model2vec-api", "healthcheck"]

ENTRYPOINT ["/model2vec-api"]
