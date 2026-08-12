# serverless-litellm (Rust)

OpenAI-compatible **multi-provider LLM gateway** for **Google Cloud Run**, in the style of [LiteLLM](https://github.com/BerriAI/litellm).

> Official LiteLLM is Python. This repo is a **small Rust rewrite** focused on Cloud Run: low memory, small image, fast cold start.

## Why Rust here?

| | Node | Rust (this repo) |
|--|--|--|
| Binary / image | larger + runtime | static-ish binary, distroless |
| Idle memory on Cloud Run | higher | typically much lower |
| Cold start | ok | usually better |
| Code complexity | easier | a bit more, but still small |

## API

| Method | Path | Auth |
|--------|------|------|
| `GET` | `/health` | public |
| `GET` | `/v1/models` | Bearer master key |
| `POST` | `/v1/chat/completions` | Bearer master key |

Same shape as OpenAI Chat Completions — point any OpenAI SDK at this service.

```bash
export BASE=https://YOUR-CLOUD-RUN-URL
export KEY=sk-your-master-key

curl -s "$BASE/v1/chat/completions" \
  -H "Authorization: Bearer $KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "grok-4.5",
    "messages": [{"role":"user","content":"hi"}]
  }'
```

OpenAI Python SDK:

```python
from openai import OpenAI
client = OpenAI(base_url="https://YOUR-URL/v1", api_key="sk-your-master-key")
print(client.chat.completions.create(
    model="grok-4.5",
    messages=[{"role": "user", "content": "hi"}],
))
```

## Config (`config.yaml`)

LiteLLM-like `model_list`. Upstream keys come from env vars:

```yaml
model_list:
  - model_name: grok-4.5          # what clients request
    litellm_params:
      model: grok-4.5             # upstream model id
      api_base: https://api.x.ai/v1
      api_key_env: XAI_API_KEY
```

Supported `provider` values:

- omit / default → **OpenAI-compatible** (`/chat/completions`) — xAI, OpenAI, Gemini OpenAI-compat, …
- `anthropic` → Anthropic Messages API (mapped to OpenAI request/response)

## Local run

```bash
# Rust 1.75+
cp .env.example .env
# edit .env — at least LITELLM_MASTER_KEY + one provider key

export $(grep -v '^#' .env | xargs)
cargo run --release
# listens on :4000 (or $PORT)
```

## Deploy to Cloud Run

```bash
gcloud auth login
gcloud config set project YOUR_PROJECT_ID

export LITELLM_MASTER_KEY='sk-...'
export XAI_API_KEY='xai-...'   # or OPENAI_API_KEY / etc.

chmod +x scripts/deploy-cloud-run.sh
./scripts/deploy-cloud-run.sh
```

Defaults: region `us-central1`, min instances `0` (serverless scale-to-zero), port `4000`, 512Mi RAM.

**Production tip:** store keys in [Secret Manager](https://cloud.google.com/secret-manager) and mount with `--set-secrets` instead of plain env vars.

## Project layout

```
config.yaml          # model routing
src/main.rs          # axum server
src/config.rs        # load YAML + env
src/auth.rs          # master key middleware
src/providers.rs     # upstream OpenAI-compat + Anthropic
src/routes.rs        # HTTP handlers
src/error.rs         # API errors
Dockerfile           # multi-stage → distroless
scripts/deploy-cloud-run.sh
```

## Notes

- This is a **gateway**, not a full LiteLLM Admin UI / virtual-key DB.
- Streaming is supported (`"stream": true`).
- Health endpoints stay public for Cloud Run probes.
