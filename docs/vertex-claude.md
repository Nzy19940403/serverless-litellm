# Scheme B: Claude via Vertex AI

Your Cloud Run gateway calls **Vertex AI Model Garden → Anthropic Claude**.  
Billing and quotas go through **GCP**, not console.anthropic.com.

## 1. Enable APIs & model access

```bash
gcloud config set project YOUR_PROJECT_ID

# Vertex AI API
gcloud services enable aiplatform.googleapis.com

# Open Model Garden in console and accept / enable Claude models you need:
# https://console.cloud.google.com/vertex-ai/publishers/anthropic/model-garden
```

Claude is **region-specific**. Common choices:

| Location | Notes |
|----------|--------|
| `us-east5` | Very common for Claude (default in this project) |
| `europe-west1` | EU, if available for your model |
| `global` | Global endpoint (if enabled for the model) |

If calls return 404 / not found, switch `GCP_LOCATION` or model id in `config.yaml`.

## 2. IAM for Cloud Run service account

Cloud Run runtime SA (often  
`PROJECT_NUMBER-compute@developer.gserviceaccount.com`  
or a custom SA) needs:

```bash
PROJECT_ID=$(gcloud config get-value project)
PROJECT_NUMBER=$(gcloud projects describe "$PROJECT_ID" --format='value(projectNumber)')
SA="${PROJECT_NUMBER}-compute@developer.gserviceaccount.com"
# If your service uses a custom SA, set SA=that@...

gcloud projects add-iam-policy-binding "$PROJECT_ID" \
  --member="serviceAccount:${SA}" \
  --role="roles/aiplatform.user"
```

## 3. Cloud Run env vars

| Name | Required | Example |
|------|----------|---------|
| `LITELLM_MASTER_KEY` | yes | `sk-my-gateway-xxx` |
| `GCP_PROJECT` | recommended | your project id |
| `GCP_LOCATION` | recommended | `us-east5` |

No `ANTHROPIC_API_KEY` needed for Vertex path.

On Cloud Run the gateway fetches  
`http://metadata.../token` automatically (ADC).

## 4. Client call

```bash
export BASE="https://YOUR-SERVICE-xxxxx.run.app"
export KEY="sk-my-gateway-xxx"

curl -s "$BASE/v1/chat/completions" \
  -H "Authorization: Bearer $KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "claude-sonnet",
    "messages": [{"role":"user","content":"你好，用一句话介绍 Claude"}],
    "max_tokens": 256
  }'
```

Models exposed by default:

- `claude-sonnet` → `claude-sonnet-4@20250514`
- `claude-haiku` → `claude-3-5-haiku@20241022`
- `claude-opus` → `claude-opus-4@20250514`

## 5. Local test (optional)

```bash
export LITELLM_MASTER_KEY=sk-test
export GCP_PROJECT=your-project
export GCP_LOCATION=us-east5
export VERTEX_ACCESS_TOKEN="$(gcloud auth print-access-token)"
cargo run --release
```

Your user/account needs Vertex AI User + Claude model enabled.

## 6. Smoke-test Vertex without the gateway

```bash
TOKEN=$(gcloud auth print-access-token)
PROJECT=your-project
LOC=us-east5
MODEL=claude-sonnet-4@20250514

curl -sS -X POST \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "Content-Type: application/json" \
  "https://${LOC}-aiplatform.googleapis.com/v1/projects/${PROJECT}/locations/${LOC}/publishers/anthropic/models/${MODEL}:rawPredict" \
  -d '{
    "anthropic_version": "vertex-2023-10-16",
    "max_tokens": 64,
    "messages": [{"role":"user","content":"Say hi"}]
  }'
```

If this fails, fix GCP/model access first — the gateway cannot work either.

## 7. Troubleshooting

| Symptom | Fix |
|---------|-----|
| `PERMISSION_DENIED` | Grant `roles/aiplatform.user` to Cloud Run SA |
| Model not found / 404 | Enable model in Model Garden; try another region |
| `Failed to fetch GCP access token` | Only happens off-GCP without `VERTEX_ACCESS_TOKEN` |
| Master key 401 | Client must use `LITELLM_MASTER_KEY` |
