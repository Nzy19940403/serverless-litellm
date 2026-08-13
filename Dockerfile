# Use official LiteLLM image (preinstalled, known-good startup)
# https://github.com/BerriAI/litellm
FROM ghcr.io/berriai/litellm:main-stable

WORKDIR /app

# Our config + Tokyo/NA auth hook
COPY config.yaml /app/config.yaml
COPY custom_auth.py /app/custom_auth.py
COPY run.py /app/run.py

ENV PYTHONPATH=/app \
    CONFIG_FILE_PATH=/app/config.yaml \
    PORT=8080 \
    NA_VERIFY_URL=http://gcp.nzysxc.com:8789/v1/auth/verify \
    GCP_PROJECT=project-8d01f8fd-0b09-42c6-974 \
    GCP_LOCATION=global \
    VERTEXAI_PROJECT=project-8d01f8fd-0b09-42c6-974 \
    VERTEXAI_LOCATION=global \
    GOOGLE_CLOUD_PROJECT=project-8d01f8fd-0b09-42c6-974 \
    # LiteLLM master key must be non-empty if referenced (override in console)
    LITELLM_MASTER_KEY=sk-litellm-change-me-in-console \
    NO_REDOC=True \
    LITELLM_LOG=INFO

# Official image ENTRYPOINT is often ["litellm"]; replace with our runner
ENTRYPOINT []
EXPOSE 8080

# Bind Cloud Run $PORT (must be 0.0.0.0)
CMD ["python", "-u", "/app/run.py"]
