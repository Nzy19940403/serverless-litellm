# Official open-source LiteLLM proxy on Cloud Run (Python).
# Supports OpenAI + Anthropic (/v1/messages) clients → Vertex Gemini / Claude, etc.
FROM mirror.gcr.io/library/python:3.12-slim-bookworm

WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && pip install --no-cache-dir --upgrade pip \
    && pip install --no-cache-dir "litellm[proxy]" httpx

COPY config.yaml /app/config.yaml
COPY custom_auth.py /app/custom_auth.py

ENV PYTHONPATH=/app \
    PORT=8080 \
    LITELLM_CONFIG=/app/config.yaml \
    # Defaults for this project (override in Cloud Run if needed)
    NA_VERIFY_URL=http://gcp.nzysxc.com:8789/v1/auth/verify \
    GCP_PROJECT=project-8d01f8fd-0b09-42c6-974 \
    GCP_LOCATION=global \
    VERTEXAI_PROJECT=project-8d01f8fd-0b09-42c6-974 \
    VERTEXAI_LOCATION=global

# Cloud Run injects PORT
EXPOSE 8080

# Single worker is fine for Cloud Run (scales with instances)
CMD ["sh", "-c", "litellm --config /app/config.yaml --host 0.0.0.0 --port ${PORT:-8080} --num_workers 1"]
