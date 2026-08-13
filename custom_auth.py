"""
LiteLLM custom auth: Tokyo MFA JWT (or optional master key) → ask North America.

Flow (same idea as the old Rust gateway):
  - Optional LITELLM_MASTER_KEY for break-glass
  - Else Bearer / x-api-key = Tokyo access_token
  - Call NA_VERIFY_URL; sticky-cache by token+login_ip until exp
  - IP change → re-ask NA

Env:
  NA_VERIFY_URL          default http://gcp.nzysxc.com:8789/v1/auth/verify
  SERVERLESS_TO_NA_SECRET / NA_VERIFY_SECRET  optional header to NA
  LITELLM_MASTER_KEY     optional shared secret
  DISABLE_NA_VERIFY=1    skip NA (dev only)
"""

from __future__ import annotations

import hashlib
import os
import time
from typing import Dict, Optional, Tuple

import httpx
from fastapi import Request
from litellm.proxy._types import UserAPIKeyAuth

# token_fp -> (exp_unix, login_ip)
_CACHE: Dict[str, Tuple[int, str]] = {}
_HTTP: Optional[httpx.AsyncClient] = None


def _client() -> httpx.AsyncClient:
    global _HTTP
    if _HTTP is None:
        _HTTP = httpx.AsyncClient(timeout=httpx.Timeout(8.0, connect=3.0))
    return _HTTP


def _fp(token: str) -> str:
    return hashlib.sha256(token.encode("utf-8")).hexdigest()[:32]


def _norm_ip(ip: str) -> str:
    ip = (ip or "").strip()
    if ip.startswith("::ffff:"):
        return ip[7:]
    return ip


def _client_ip(request: Request) -> str:
    xff = request.headers.get("x-forwarded-for") or ""
    if xff:
        first = xff.split(",")[0].strip()
        if first:
            return first
    real = (request.headers.get("x-real-ip") or "").strip()
    if real:
        return real
    if request.client:
        return request.client.host or "0.0.0.0"
    return "0.0.0.0"


def _extract_key(request: Request, api_key: Optional[str]) -> str:
    if api_key and api_key.strip():
        return api_key.strip()
    auth = request.headers.get("authorization") or ""
    if auth.lower().startswith("bearer "):
        return auth[7:].strip()
    xk = request.headers.get("x-api-key") or ""
    return xk.strip()


async def user_api_key_auth(request: Request, api_key: str) -> UserAPIKeyAuth:
    """LiteLLM custom_auth entrypoint."""
    key = _extract_key(request, api_key)
    if not key:
        raise Exception("Missing API key (Authorization Bearer or x-api-key)")

    master = (os.environ.get("LITELLM_MASTER_KEY") or "").strip()
    if master and key == master:
        return UserAPIKeyAuth(api_key=key, user_id="master", team_id="break-glass")

    if os.environ.get("DISABLE_NA_VERIFY", "").strip() in ("1", "true", "yes"):
        return UserAPIKeyAuth(api_key=key, user_id="dev-no-na")

    na_url = (
        os.environ.get("NA_VERIFY_URL")
        or "http://gcp.nzysxc.com:8789/v1/auth/verify"
    ).strip()
    client_ip = _client_ip(request)
    fp = _fp(key)
    now = int(time.time())

    cached = _CACHE.get(fp)
    if cached:
        exp, login_ip = cached
        if exp > now + 5 and _norm_ip(client_ip) == _norm_ip(login_ip):
            return UserAPIKeyAuth(
                api_key=key,
                user_id="tokyo-jwt",
                metadata={"login_ip": login_ip, "auth": "cache"},
            )

    headers = {
        "Authorization": f"Bearer {key}",
        "x-client-ip": client_ip,
    }
    secret = (
        os.environ.get("SERVERLESS_TO_NA_SECRET")
        or os.environ.get("NA_VERIFY_SECRET")
        or ""
    ).strip()
    if secret:
        headers["x-serverless-secret"] = secret

    try:
        res = await _client().post(na_url, headers=headers)
        data = res.json() if res.content else {}
    except Exception as e:
        raise Exception(f"NA verify unreachable: {e}") from e

    if res.status_code != 200 or not data.get("active"):
        err = data.get("error") or f"NA denied HTTP {res.status_code}"
        _CACHE.pop(fp, None)
        raise Exception(f"Not allowed: {err}")

    exp = int(data.get("exp") or (now + 3600))
    login_ip = (data.get("login_ip") or "").strip() or client_ip
    # sticky only when current IP matches mint login_ip (or empty legacy tokens)
    if not login_ip or _norm_ip(client_ip) == _norm_ip(login_ip):
        _CACHE[fp] = (exp, login_ip or client_ip)
        if len(_CACHE) > 10000:
            # crude prune
            dead = [k for k, (e, _) in _CACHE.items() if e <= now]
            for k in dead[:5000]:
                _CACHE.pop(k, None)

    sub = data.get("sub") or "tokyo-jwt"
    return UserAPIKeyAuth(
        api_key=key,
        user_id=str(sub),
        metadata={
            "login_ip": login_ip,
            "client_ip": client_ip,
            "auth": "na-verify",
        },
    )
