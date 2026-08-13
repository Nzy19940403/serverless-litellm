# Cloud Run / Cloud Build friendly multi-stage image.
# Use mirror.gcr.io to avoid Docker Hub rate limits from GCP builders.

# ---------- build ----------
FROM mirror.gcr.io/library/rust:1-bookworm AS builder
WORKDIR /app

# Dependency layer cache: only recompile crates when Cargo.* changes
COPY Cargo.toml Cargo.lock ./
RUN mkdir src \
    && echo 'fn main() { println!("build stub"); }' > src/main.rs \
    && cargo build --release \
    && rm -rf src \
    && rm -f target/release/serverless-litellm \
    && rm -f target/release/deps/serverless_litellm* \
    && rm -rf target/release/.fingerprint/serverless-litellm*

COPY src ./src
COPY config.yaml ./config.yaml
# include_str! embeds static/index.html at compile time
COPY static ./static
RUN cargo build --release \
    && strip target/release/serverless-litellm || true

# ---------- runtime ----------
# debian-slim is more reliable on Cloud Run than distroless for first deploy/debug
FROM mirror.gcr.io/library/debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd -r -u 65532 -s /usr/sbin/nologin nonroot

WORKDIR /app
COPY --from=builder /app/target/release/serverless-litellm /app/serverless-litellm
COPY --from=builder /app/config.yaml /app/config.yaml

# Cloud Run injects PORT (usually 8080). Default 8080 so listen matches if env is missing.
ENV CONFIG_PATH=/app/config.yaml \
    PORT=8080 \
    RUST_LOG=serverless_litellm=info \
    RUST_BACKTRACE=1

EXPOSE 8080
USER nonroot
CMD ["/app/serverless-litellm"]
