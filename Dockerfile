# syntax=docker/dockerfile:1
# Multi-stage: tiny Cloud Run image (~10-20MB compressed typical)

FROM rust:1.85-bookworm AS builder
WORKDIR /app

# Cache dependencies
COPY Cargo.toml ./
RUN mkdir src && echo 'fn main(){}' > src/main.rs \
    && cargo build --release \
    && rm -rf src

COPY src ./src
COPY config.yaml ./config.yaml
# Force rebuild of actual sources
RUN touch src/main.rs && cargo build --release

# Runtime: distroless or debian slim
FROM gcr.io/distroless/cc-debian12:nonroot
WORKDIR /app

COPY --from=builder /app/target/release/serverless-litellm /app/serverless-litellm
COPY --from=builder /app/config.yaml /app/config.yaml

ENV CONFIG_PATH=/app/config.yaml
ENV PORT=4000
ENV RUST_LOG=serverless_litellm=info

EXPOSE 4000
USER nonroot:nonroot
ENTRYPOINT ["/app/serverless-litellm"]
