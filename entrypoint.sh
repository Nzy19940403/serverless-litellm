#!/bin/sh
set -e
PORT="${PORT:-8080}"
echo "[entrypoint] PORT=$PORT"
echo "[entrypoint] GCP_PROJECT=${GCP_PROJECT:-} GCP_LOCATION=${GCP_LOCATION:-}"
echo "[entrypoint] NA_VERIFY_URL=${NA_VERIFY_URL:-}"

# Fail fast with a clear log line if imports/config are broken
python - <<'PY'
import os, sys
print("[entrypoint] python", sys.version)
try:
    import litellm
    print("[entrypoint] litellm", getattr(litellm, "__version__", "?"))
except Exception as e:
    print("[entrypoint] FATAL import litellm:", e)
    raise
try:
    import custom_auth
    print("[entrypoint] custom_auth ok")
except Exception as e:
    print("[entrypoint] FATAL import custom_auth:", e)
    raise
print("[entrypoint] imports ok")
PY

echo "[entrypoint] starting litellm..."
# --detailed_debug helps first deploys; remove later if noisy
exec litellm \
  --config /app/config.yaml \
  --host 0.0.0.0 \
  --port "$PORT" \
  --num_workers 1
