#!/usr/bin/env bash
# Deploy to Google Cloud Run
set -euo pipefail

PROJECT_ID="${PROJECT_ID:-$(gcloud config get-value project 2>/dev/null || true)}"
REGION="${REGION:-us-central1}"
SERVICE="${SERVICE:-serverless-litellm}"
IMAGE="${IMAGE:-${REGION}-docker.pkg.dev/${PROJECT_ID}/litellm/${SERVICE}:latest}"

# North America auth verifier (Tokyo-minted JWT)
NA_VERIFY_URL="${NA_VERIFY_URL:-http://gcp.nzysxc.com:8789/v1/auth/verify}"
GCP_LOCATION="${GCP_LOCATION:-global}"

if [[ -z "${PROJECT_ID}" || "${PROJECT_ID}" == "(unset)" ]]; then
  echo "Set PROJECT_ID or run: gcloud config set project YOUR_PROJECT"
  exit 1
fi

echo "Project : ${PROJECT_ID}"
echo "Region  : ${REGION}"
echo "Service : ${SERVICE}"
echo "Image   : ${IMAGE}"
echo "NA_VERIFY_URL: ${NA_VERIFY_URL}"

# Ensure Artifact Registry repo
gcloud artifacts repositories describe litellm --location="${REGION}" >/dev/null 2>&1 \
  || gcloud artifacts repositories create litellm \
       --repository-format=docker \
       --location="${REGION}" \
       --description="serverless-litellm images"

gcloud auth configure-docker "${REGION}-docker.pkg.dev" --quiet

# Build & push (Cloud Build — no local Docker required)
gcloud builds submit --tag "${IMAGE}" .

ENV_VARS="RUST_LOG=serverless_litellm=info"
ENV_VARS="${ENV_VARS},NA_VERIFY_URL=${NA_VERIFY_URL}"
ENV_VARS="${ENV_VARS},GCP_PROJECT=${PROJECT_ID}"
ENV_VARS="${ENV_VARS},GCP_LOCATION=${GCP_LOCATION}"
ENV_VARS="${ENV_VARS},JWT_ISSUER=${JWT_ISSUER:-litellm-mfa-tokyo}"
ENV_VARS="${ENV_VARS},JWT_AUDIENCE=${JWT_AUDIENCE:-serverless-litellm}"

[[ -n "${LITELLM_MASTER_KEY:-}" ]] && ENV_VARS="${ENV_VARS},LITELLM_MASTER_KEY=${LITELLM_MASTER_KEY}"
[[ -n "${SERVERLESS_TO_NA_SECRET:-}" ]] && ENV_VARS="${ENV_VARS},SERVERLESS_TO_NA_SECRET=${SERVERLESS_TO_NA_SECRET}"
[[ -n "${XAI_API_KEY:-}" ]] && ENV_VARS="${ENV_VARS},XAI_API_KEY=${XAI_API_KEY}"
[[ -n "${OPENAI_API_KEY:-}" ]] && ENV_VARS="${ENV_VARS},OPENAI_API_KEY=${OPENAI_API_KEY}"
[[ -n "${ANTHROPIC_API_KEY:-}" ]] && ENV_VARS="${ENV_VARS},ANTHROPIC_API_KEY=${ANTHROPIC_API_KEY}"
[[ -n "${GEMINI_API_KEY:-}" ]] && ENV_VARS="${ENV_VARS},GEMINI_API_KEY=${GEMINI_API_KEY}"

# Cloud Run sets PORT; container listens on PORT (default 8080 in Dockerfile)
gcloud run deploy "${SERVICE}" \
  --image "${IMAGE}" \
  --region "${REGION}" \
  --platform managed \
  --port 8080 \
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
echo
echo "If you use continuous deploy from Git, also set these on the Cloud Run service:"
echo "  NA_VERIFY_URL=${NA_VERIFY_URL}"
echo "  GCP_PROJECT=${PROJECT_ID}"
echo "  GCP_LOCATION=${GCP_LOCATION}"
