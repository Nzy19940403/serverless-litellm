#!/usr/bin/env python3
"""Mint RS256 JWT for serverless-litellm agents (private key stays local).

Usage:
  python scripts/mint_jwt.py --key keys/private.pem --days 30 --sub my-agent

Then agents use:
  Authorization: Bearer <token>
  # OpenAI SDK: api_key="<token>"
"""
from __future__ import annotations

import argparse
import sys
import time
from pathlib import Path

try:
    import jwt  # PyJWT
except ImportError:
    print("Install: pip install PyJWT cryptography", file=sys.stderr)
    sys.exit(1)


def main() -> None:
    p = argparse.ArgumentParser(description="Mint RS256 JWT for serverless-litellm")
    p.add_argument("--key", required=True, help="Path to RSA private.pem")
    p.add_argument("--days", type=float, default=30, help="Token lifetime in days")
    p.add_argument("--sub", default="agent", help="JWT subject (agent id)")
    p.add_argument("--iss", default=None, help="Optional issuer (must match JWT_ISSUER if set)")
    p.add_argument("--aud", default=None, help="Optional audience (must match JWT_AUDIENCE if set)")
    args = p.parse_args()

    private_key = Path(args.key).read_text(encoding="utf-8")
    now = int(time.time())
    payload = {
        "sub": args.sub,
        "iat": now,
        "exp": now + int(args.days * 86400),
    }
    if args.iss:
        payload["iss"] = args.iss
    if args.aud:
        payload["aud"] = args.aud

    token = jwt.encode(payload, private_key, algorithm="RS256")
    if isinstance(token, bytes):
        token = token.decode("utf-8")

    print(token)
    print(
        f"\n# exp in {args.days} day(s); use as OpenAI api_key / Bearer token",
        file=sys.stderr,
    )


if __name__ == "__main__":
    main()
