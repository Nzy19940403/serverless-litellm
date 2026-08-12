use axum::extract::Request;
use axum::http::header;
use axum::middleware::Next;
use axum::response::Response;
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde::Deserialize;
use std::env;
use std::fs;
use std::sync::Arc;

use crate::config::is_prod;
use crate::error::AppError;

/// Runtime auth: shared master key and/or RS256 JWT public key.
/// Secrets come from env / Secret Manager — never from git.
#[derive(Clone)]
pub struct AuthState {
    master_key: String,
    jwt_key: Option<DecodingKey>,
    jwt_validation: Validation,
}

#[derive(Debug, Deserialize)]
struct JwtClaims {
    /// optional subject (e.g. agent name)
    #[allow(dead_code)]
    sub: Option<String>,
    /// validated by jsonwebtoken when validate_exp is true
    #[allow(dead_code)]
    exp: i64,
}

impl AuthState {
    pub fn from_env(master_key: String) -> Result<Self, anyhow::Error> {
        let jwt_key = load_jwt_public_key()?;
        let mut jwt_validation = Validation::new(Algorithm::RS256);
        jwt_validation.validate_exp = true;
        // allow small clock skew
        jwt_validation.leeway = 60;

        if let Ok(iss) = env::var("JWT_ISSUER") {
            if !iss.is_empty() {
                jwt_validation.set_issuer(&[iss]);
            }
        }
        if let Ok(aud) = env::var("JWT_AUDIENCE") {
            if !aud.is_empty() {
                jwt_validation.set_audience(&[aud]);
            }
        } else {
            // do not require aud unless configured
            jwt_validation.validate_aud = false;
        }

        if master_key.is_empty() && jwt_key.is_none() && is_prod() {
            anyhow::bail!(
                "Auth not configured: set LITELLM_MASTER_KEY and/or JWT_PUBLIC_KEY \
                 (or JWT_PUBLIC_KEY_FILE) on Cloud Run"
            );
        }

        Ok(Self {
            master_key,
            jwt_key,
            jwt_validation,
        })
    }

    pub fn master_key_set(&self) -> bool {
        !self.master_key.is_empty()
    }

    pub fn jwt_enabled(&self) -> bool {
        self.jwt_key.is_some()
    }

    fn accept_token(&self, token: &str) -> bool {
        if !self.master_key.is_empty() && token == self.master_key {
            return true;
        }
        if let Some(key) = &self.jwt_key {
            return decode::<JwtClaims>(token, key, &self.jwt_validation).is_ok();
        }
        false
    }
}

fn load_jwt_public_key() -> Result<Option<DecodingKey>, anyhow::Error> {
    let pem = if let Ok(path) = env::var("JWT_PUBLIC_KEY_FILE") {
        if path.is_empty() {
            None
        } else {
            Some(fs::read_to_string(&path).map_err(|e| {
                anyhow::anyhow!("read JWT_PUBLIC_KEY_FILE ({path}): {e}")
            })?)
        }
    } else if let Ok(raw) = env::var("JWT_PUBLIC_KEY") {
        if raw.is_empty() {
            None
        } else {
            // Cloud Run env often stores PEM with literal \n
            Some(raw.replace("\\n", "\n"))
        }
    } else {
        None
    };

    match pem {
        None => Ok(None),
        Some(p) => {
            let key = DecodingKey::from_rsa_pem(p.trim().as_bytes())
                .map_err(|e| anyhow::anyhow!("invalid JWT_PUBLIC_KEY PEM: {e}"))?;
            Ok(Some(key))
        }
    }
}

/// Public: `/`, `/health`, `/healthz`, `/ui`
/// Protected APIs: Bearer master key **or** RS256 JWT (signed offline with private key)
pub async fn gateway_auth(
    axum::extract::State(auth): axum::extract::State<Arc<AuthState>>,
    req: Request,
    next: Next,
) -> Result<Response, AppError> {
    let path = req.uri().path();
    if matches!(path, "/" | "/health" | "/healthz" | "/ui" | "/ui/")
        || path.starts_with("/ui/")
    {
        return Ok(next.run(req).await);
    }

    // Dev with no auth configured at all
    if auth.master_key.is_empty() && auth.jwt_key.is_none() {
        if is_prod() {
            return Err(AppError::Internal(
                "No LITELLM_MASTER_KEY or JWT_PUBLIC_KEY configured".into(),
            ));
        }
        return Ok(next.run(req).await);
    }

    let Some(token) = extract_token(&req) else {
        return Err(AppError::Unauthorized(
            "Missing credentials. Use Authorization: Bearer <master_key|jwt> or x-api-key".into(),
        ));
    };

    if auth.accept_token(&token) {
        return Ok(next.run(req).await);
    }

    Err(AppError::Unauthorized(
        "Invalid credentials (master key or JWT verification failed)".into(),
    ))
}

fn extract_token(req: &Request) -> Option<String> {
    if let Some(auth) = req.headers().get(header::AUTHORIZATION) {
        if let Ok(s) = auth.to_str() {
            if let Some(rest) = s.strip_prefix("Bearer ") {
                let t = rest.trim();
                if !t.is_empty() {
                    return Some(t.to_string());
                }
            }
        }
    }
    req.headers()
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}
