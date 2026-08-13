# serverless-litellm

**Open-source [LiteLLM](https://github.com/BerriAI/litellm) proxy** on **Google Cloud Run** (Python).

- OpenAI clients → `/v1/chat/completions`
- **Anthropic / Claude agent** → `/v1/messages` (LiteLLM does protocol conversion)
- Upstream: **Vertex / Agent Platform** Gemini (and Claude if quota allows)
- Auth: Tokyo MFA JWT → North America `/v1/auth/verify` (`custom_auth.py`)

> Older **Rust** gateway code remains under `src/` for reference only.  
> **Deploy uses Python LiteLLM** (`Dockerfile` + `config.yaml` + `custom_auth.py`).

## Clients

### OpenAI SDK / Cursor

```text
base_url = https://YOUR-CLOUD-RUN-URL/v1
api_key  = <Tokyo access_token>
model    = gemini-3.5-flash-lite   # or gemini-3.6-flash, …
```

### Claude agent / Anthropic SDK

LiteLLM accepts Anthropic-shaped traffic and can route to Gemini:

```text
ANTHROPIC_BASE_URL = https://YOUR-CLOUD-RUN-URL
ANTHROPIC_API_KEY  = <Tokyo access_token>
# model name as registered in config.yaml, e.g. gemini-3.5-flash
```

Exact env names depend on your Claude agent; point base URL at this service and use the Tokyo JWT as the API key.

## Auth

| Key | Meaning |
|-----|---------|
| Tokyo `access_token` | Normal path → `NA_VERIFY_URL` allow |
| `LITELLM_MASTER_KEY` | Optional break-glass |

Env (defaults baked in Dockerfile for this project):

| Env | Default |
|-----|---------|
| `NA_VERIFY_URL` | `http://gcp.nzysxc.com:8789/v1/auth/verify` |
| `GCP_PROJECT` / `VERTEXAI_PROJECT` | `project-8d01f8fd-0b09-42c6-974` |
| `GCP_LOCATION` / `VERTEXAI_LOCATION` | `global` |

Cloud Run **runtime SA** needs **Agent Platform User** (`roles/aiplatform.user`).

## Deploy

Git push → Cloud Build → Cloud Run (existing continuous deploy).

Local:

```bash
pip install -r requirements.txt
export GCP_PROJECT=project-8d01f8fd-0b09-42c6-974
export GCP_LOCATION=global
export GOOGLE_APPLICATION_CREDENTIALS=...   # or gcloud ADC
export NA_VERIFY_URL=http://gcp.nzysxc.com:8789/v1/auth/verify
litellm --config config.yaml --port 8080
```

## Models

See `config.yaml` (`gemini-3.5-flash-lite`, `gemini-3.6-flash`, Claude aliases, …).
