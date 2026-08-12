#!/usr/bin/env bash
# Deploy to Google Cloud Run
set -euo pipefail

PROJECT_ID="${PROJECT_ID:-$(gcloud config get-value project 2>/dev/null || true)}"
REGION="${REGION:-us-central1}"
SERVICE="${SERVICE:-serverless-litellm}"
IMAGE="${IMAGE:-${REGION}-docker.pkg.dev/${PROJECT_ID}/litellm/${SERVICE}:latest}"

if [[ -z "${PROJECT_ID}" || "${PROJECT_ID}" == "(unset)" ]]; then
  echo "Set PROJECT_ID or run: gcloud config set project YOUR_PROJECT"
  exit 1
fi

echo "Project : ${PROJECT_ID}"
echo "Region  : ${REGION}"
echo "Service : ${SERVICE}"
echo "Image   : ${IMAGE}"

# Ensure Artifact Registry repo
gcloud artifacts repositories describe litellm --location="${REGION}" >/dev/null 2>&1 \
  || gcloud artifacts repositories create litellm \
       --repository-format=docker \
       --location="${REGION}" \
       --description="serverless-litellm images"

gcloud auth configure-docker "${REGION}-docker.pkg.dev" --quiet

# Build & push (Cloud Build — no local Docker required)
gcloud builds submit --tag "${IMAGE}" .

# Required secrets as env (set before deploy or pass here)
# Prefer Secret Manager in production.
MASTER_KEY="${LITELLM_MASTER_KEY:-}"
if [[ -z "${MASTER_KEY}" ]]; then
  echo "WARN: LITELLM_MASTER_KEY not set in shell; service may refuse traffic in Cloud Run"
fi

ENV_VARS="RUST_LOG=serverless_litellm=info"
[[ -n "${MASTER_KEY}" ]] && ENV_VARS="${ENV_VARS},LITELLM_MASTER_KEY=${MASTER_KEY}"
[[ -n "${XAI_API_KEY:-}" ]] && ENV_VARS="${ENV_VARS},XAI_API_KEY=${XAI_API_KEY}"
[[ -n "${OPENAI_API_KEY:-}" ]] && ENV_VARS="${ENV_VARS},OPENAI_API_KEY=${OPENAI_API_KEY}"
[[ -n "${ANTHROPIC_API_KEY:-}" ]] && ENV_VARS="${ENV_VARS},ANTHROPIC_API_KEY=${ANTHROPIC_API_KEY}"
[[ -n "${GEMINI_API_KEY:-}" ]] && ENV_VARS="${ENV_VARS},GEMINI_API_KEY=${GEMINI_API_KEY}"

gcloud run deploy "${SERVICE}" \
  --image "${IMAGE}" \
  --region "${REGION}" \
  --platform managed \
  --port 4000 \
  --memory 512Mi \
  --cpu 1 \
  --timeout 300 \
  --concurrency 80 \
  --min-instances 0 \
  --max-instances 10 \
  --allow-unauthenticated \
  --set-env-vars "${ENV_VARS}"

echo
echo "Done. Service URL:"
gcloud run services describe "${SERVICE}" --region "${REGION}" --format='value(status.url)'
