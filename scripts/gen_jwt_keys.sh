#!/usr/bin/env bash
# Generate RSA keypair for gateway JWT auth. PRIVATE key stays on your machine only.
set -euo pipefail
DIR="${1:-./keys}"
mkdir -p "$DIR"
chmod 700 "$DIR" 2>/dev/null || true

openssl genrsa -out "$DIR/private.pem" 2048
openssl rsa -in "$DIR/private.pem" -pubout -out "$DIR/public.pem"
chmod 600 "$DIR/private.pem"

echo "Created:"
echo "  $DIR/private.pem  ← keep offline; NEVER commit or upload to Cloud Run"
echo "  $DIR/public.pem   ← paste into Cloud Run env JWT_PUBLIC_KEY (or Secret Manager)"
echo
echo "Public key PEM (for Cloud Run JWT_PUBLIC_KEY — use \\n for newlines if single-line env):"
echo "-----"
cat "$DIR/public.pem"
