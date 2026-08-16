# syntax=docker/dockerfile:1.7
#
# Dockerfile for model2vec-api using distroless image with pre-built release binaries.
#
# Expects pre-built binaries at:
#   bin/amd64/model2vec-api
#   bin/arm64/model2vec-api
#
# Usage (CI / pre-built):
#   docker buildx build --platform linux/amd64,linux/arm64 -t model2vec-api .
#

FROM gcr.io/distroless/cc-debian13:nonroot AS runtime

ARG TARGETARCH

# Copy the pre-built binary for target architecture.
COPY --chown=65532:65532 bin/${TARGETARCH}/model2vec-api /model2vec-api

# CA certificates so HTTPS (HuggingFace downloads) works at runtime.
COPY --from=busybox:1.36 /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt

USER 65532:65532
WORKDIR /opt/model2vec

# Model weights can be bind-mounted at /models (read-only) — see docker-compose.yml.
VOLUME ["/models"]

EXPOSE 22671

HEALTHCHECK --interval=30s --timeout=5s --start-period=60s --retries=3 \
    CMD ["/model2vec-api", "healthcheck"]

ENTRYPOINT ["/model2vec-api"]