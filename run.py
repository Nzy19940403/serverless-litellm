"""
Cloud Run entry: bind $PORT ASAP with uvicorn + LiteLLM proxy app.
"""
from __future__ import annotations

import os
import sys


def main() -> None:
    port = int(os.environ.get("PORT", "8080"))
    host = "0.0.0.0"
    config = os.environ.get("CONFIG_FILE_PATH", "/app/config.yaml")

    # LiteLLM reads config from this env when using proxy_server module
    os.environ["CONFIG_FILE_PATH"] = config
    os.environ.setdefault("PYTHONPATH", "/app")

    print(f"[run] starting litellm proxy host={host} port={port} config={config}", flush=True)
    print(f"[run] VERTEXAI_PROJECT={os.environ.get('VERTEXAI_PROJECT')}", flush=True)
    print(f"[run] NA_VERIFY_URL={os.environ.get('NA_VERIFY_URL')}", flush=True)

    # Import after env is set
    try:
        import custom_auth  # noqa: F401

        print("[run] custom_auth import ok", flush=True)
    except Exception as e:
        print(f"[run] custom_auth import FAILED: {e}", flush=True)
        # Continue — config may still start without custom auth if removed
        raise

    try:
        import uvicorn
        from litellm.proxy.proxy_server import app
    except Exception as e:
        print(f"[run] FATAL import litellm proxy: {e}", flush=True)
        raise

    print("[run] uvicorn.run ...", flush=True)
    # workers=1: Cloud Run scales instances, not workers
    uvicorn.run(app, host=host, port=port, log_level="info", timeout_keep_alive=75)


if __name__ == "__main__":
    try:
        main()
    except Exception as e:
        print(f"[run] exit error: {e}", file=sys.stderr, flush=True)
        raise
