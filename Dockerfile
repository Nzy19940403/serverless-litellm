# Simplest possible LiteLLM on Cloud Run
# Official image + CLI bind to $PORT (no custom entrypoint complexity)
FROM ghcr.io/berriai/litellm:main-stable

WORKDIR /app
COPY config.yaml /app/config.yaml
COPY custom_auth.py /app/custom_auth.py

ENV PYTHONPATH=/app \
    PORT=8080 \
    LITELLM_MASTER_KEY=sk-litellm-change-me \
    NA_VERIFY_URL=http://gcp.nzysxc.com:8789/v1/auth/verify \
    GCP_PROJECT=project-8d01f8fd-0b09-42c6-974 \
    GCP_LOCATION=global \
    VERTEXAI_PROJECT=project-8d01f8fd-0b09-42c6-974 \
    VERTEXAI_LOCATION=global \
    GOOGLE_CLOUD_PROJECT=project-8d01f8fd-0b09-42c6-974 \
    NO_REDOC=True \
    LITELLM_LOG=INFO

# Clear any base-image ENTRYPOINT so CMD fully controls startup
ENTRYPOINT []

# IMPORTANT: listen on 0.0.0.0 and Cloud Run's PORT
CMD ["sh", "-c", "echo starting litellm on PORT=${PORT:-8080} && exec litellm --config /app/config.yaml --host 0.0.0.0 --port ${PORT:-8080} --num_workers 1"]
