# Open-source LiteLLM proxy on Cloud Run (Python only — no Rust)
FROM mirror.gcr.io/library/python:3.12-slim-bookworm

WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && pip install --no-cache-dir --upgrade pip \
    && pip install --no-cache-dir "litellm[proxy]" httpx

COPY config.yaml /app/config.yaml
COPY custom_auth.py /app/custom_auth.py
COPY entrypoint.sh /app/entrypoint.sh
RUN chmod +x /app/entrypoint.sh

ENV PYTHONPATH=/app \
    PORT=8080 \
    # Project defaults (override in Cloud Run console if needed)
    NA_VERIFY_URL=http://gcp.nzysxc.com:8789/v1/auth/verify \
    GCP_PROJECT=project-8d01f8fd-0b09-42c6-974 \
    GCP_LOCATION=global \
    VERTEXAI_PROJECT=project-8d01f8fd-0b09-42c6-974 \
    VERTEXAI_LOCATION=global \
    # LiteLLM / Google ADC on Cloud Run
    GOOGLE_CLOUD_PROJECT=project-8d01f8fd-0b09-42c6-974

EXPOSE 8080

# Cloud Run: process must bind $PORT quickly enough for startup probe
CMD ["/app/entrypoint.sh"]
