use axum::extract::Request;
use axum::http::header;
use axum::middleware::Next;
use axum::response::Response;
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde::Deserialize;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

use crate::config::is_prod;
use crate::error::AppError;

/// Cached positive decision after NA verify (or local JWT when IP matches).
#[derive(Clone, Debug)]
struct SessionCache {
    /// JWT exp (unix seconds)
    exp: i64,
    /// login_ip bound at Tokyo mint (from NA / JWT claim)
    login_ip: String,
    /// last successful local decision
    #[allow(dead_code)]
    cached_at: Instant,
}

/// Runtime auth:
/// - optional master key
/// - optional local RS256 public key
/// - **NA remote verify** with **IP-sticky cache**:
///   while JWT not expired and request IP == login_ip → do **not** re-ask NA;
///   if request IP differs from login_ip → ask NA again.
#[derive(Clone)]
pub struct AuthState {
    master_key: String,
    jwt_key: Option<DecodingKey>,
    jwt_validation: Validation,
    /// e.g. http://gcp.nzysxc.com:8789/v1/auth/verify
    na_verify_url: Option<String>,
    na_secret: Option<String>,
    http: reqwest::Client,
    /// token fingerprint → session
    cache: Arc<RwLock<HashMap<String, SessionCache>>>,
}

#[derive(Debug, Deserialize)]
struct JwtClaims {
    #[allow(dead_code)]
    sub: Option<String>,
    exp: i64,
    typ: Option<String>,
    #[serde(default)]
    login_ip: Option<String>,
}

#[derive(Debug, Deserialize)]
struct NaVerifyResp {
    active: bool,
    #[serde(default)]
    exp: Option<i64>,
    #[serde(default)]
    login_ip: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

impl AuthState {
    pub fn from_env(master_key: String) -> Result<Self, anyhow::Error> {
        // Soft-fail invalid PEM so Cloud Run still binds PORT
        let jwt_key = match load_jwt_public_key() {
            Ok(k) => k,
            Err(e) => {
                tracing::error!("JWT_PUBLIC_KEY load failed (ignored at startup): {e}");
                None
            }
        };
        let mut jwt_validation = Validation::new(Algorithm::RS256);
        jwt_validation.validate_exp = true;
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
            jwt_validation.validate_aud = false;
        }

        // Where to ask North America "is this access token allowed?"
        // This is only the NA base address (trust target), not a second auth system.
        // Override with NA_VERIFY_URL if DNS/IP changes; default is production NA :8789.
        const DEFAULT_NA_VERIFY: &str = "http://gcp.nzysxc.com:8789/v1/auth/verify";
        let na_verify_url = env::var("NA_VERIFY_URL")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                // In Cloud Run / prod: always know where NA is (hard-coded default).
                // Local dev: still default so behavior matches; set NA_VERIFY_URL="" to disable
                // via DISABLE_NA_VERIFY=1.
                if env::var("DISABLE_NA_VERIFY").ok().as_deref() == Some("1") {
                    None
                } else {
                    Some(DEFAULT_NA_VERIFY.to_string())
                }
            });
        let na_secret = env::var("SERVERLESS_TO_NA_SECRET")
            .or_else(|_| env::var("NA_VERIFY_SECRET"))
            .ok()
            .filter(|s| !s.is_empty());

        // Do NOT bail here: Cloud Run health checks require the process to bind PORT.
        if master_key.is_empty() && jwt_key.is_none() && na_verify_url.is_none() {
            if is_prod() {
                tracing::error!(
                    "Auth not configured: no NA verify / master key / JWT — API will reject"
                );
            } else {
                tracing::warn!("Auth not configured (dev mode allows open access)");
            }
        } else {
            tracing::info!(
                master = !master_key.is_empty(),
                jwt = jwt_key.is_some(),
                na_verify = na_verify_url.as_deref().unwrap_or("-"),
                "auth ready (LLM trust path: client JWT → ask NA → Vertex)"
            );
        }

        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(8))
            .connect_timeout(Duration::from_secs(3))
            .build()?;

        Ok(Self {
            master_key,
            jwt_key,
            jwt_validation,
            na_verify_url,
            na_secret,
            http,
            cache: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    pub fn master_key_set(&self) -> bool {
        !self.master_key.is_empty()
    }

    pub fn jwt_enabled(&self) -> bool {
        self.jwt_key.is_some()
    }

    pub fn na_verify_enabled(&self) -> bool {
        self.na_verify_url.is_some()
    }

    fn token_fp(token: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut h = DefaultHasher::new();
        token.hash(&mut h);
        format!("{:016x}", h.finish())
    }

    /// Local RS256 only (no NA). Used when we still want master/JWT fallback.
    fn accept_local(&self, token: &str) -> Option<JwtClaims> {
        if !self.master_key.is_empty() && token == self.master_key {
            // synthetic "always ok" — not cached as JWT
            return None;
        }
        let key = self.jwt_key.as_ref()?;
        let data = decode::<JwtClaims>(token, key, &self.jwt_validation).ok()?;
        if let Some(typ) = data.claims.typ.as_deref() {
            if typ == "refresh" {
                return None;
            }
        }
        Some(data.claims)
    }

    fn is_master(&self, token: &str) -> bool {
        !self.master_key.is_empty() && token == self.master_key
    }

    async fn cache_get_allow(&self, fp: &str, client_ip: &str) -> bool {
        let now = chrono_now();
        let guard = self.cache.read().await;
        if let Some(e) = guard.get(fp) {
            if e.exp > now + 5 && ips_equal(client_ip, &e.login_ip) {
                return true;
            }
        }
        false
    }

    async fn cache_put(&self, fp: String, exp: i64, login_ip: String) {
        let mut guard = self.cache.write().await;
        // opportunistic prune
        if guard.len() > 10_000 {
            let now = chrono_now();
            guard.retain(|_, v| v.exp > now);
        }
        guard.insert(
            fp,
            SessionCache {
                exp,
                login_ip,
                cached_at: Instant::now(),
            },
        );
    }

    /// Returns true if allowed.
    async fn authorize_token(&self, token: &str, client_ip: &str) -> Result<bool, AppError> {
        if self.is_master(token) {
            return Ok(true);
        }

        let fp = Self::token_fp(token);

        // Fast path: JWT not expired (via cache) + same login IP → no NA call
        if self.cache_get_allow(&fp, client_ip).await {
            tracing::debug!(%client_ip, "auth cache hit (same login_ip, skip NA)");
            return Ok(true);
        }

        // If we have a local public key and can verify JWT:
        // same login_ip → allow without NA; different IP → must ask NA (if configured)
        if let Some(claims) = self.accept_local(token) {
            let lip = claims.login_ip.as_deref().unwrap_or("");
            if lip.is_empty() || ips_equal(client_ip, lip) {
                self.cache_put(fp, claims.exp, if lip.is_empty() { client_ip.to_string() } else { lip.to_string() })
                    .await;
                tracing::debug!(%client_ip, "local JWT ok, same/empty login_ip, skip NA");
                return Ok(true);
            }
            // IP mismatch → fall through to NA if available
            tracing::info!(
                %client_ip,
                login_ip = lip,
                "JWT login_ip mismatch → ask NA"
            );
        }

        // Ask NA: first use, cache miss, expired, or IP changed
        if let Some(url) = &self.na_verify_url {
            return self.ask_na(url, token, client_ip, fp).await;
        }

        // No NA and local JWT failed
        Ok(false)
    }

    async fn ask_na(
        &self,
        url: &str,
        token: &str,
        client_ip: &str,
        fp: String,
    ) -> Result<bool, AppError> {
        let mut req = self
            .http
            .post(url)
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header("x-client-ip", client_ip);

        if let Some(sec) = &self.na_secret {
            req = req.header("x-serverless-secret", sec);
        }

        let res = req.send().await.map_err(|e| {
            AppError::Unauthorized(format!("NA verify unreachable: {e}"))
        })?;

        let status = res.status();
        let body: NaVerifyResp = res.json().await.map_err(|e| {
            AppError::Unauthorized(format!("NA verify bad response: {e}"))
        })?;

        if !status.is_success() || !body.active {
            let msg = body
                .error
                .unwrap_or_else(|| format!("NA denied (HTTP {status})"));
            tracing::warn!(%msg, %client_ip, "NA verify denied");
            // drop cache entry if any
            self.cache.write().await.remove(&fp);
            return Ok(false);
        }

        let exp = body.exp.unwrap_or_else(|| chrono_now() + 3600);
        // Prefer JWT login_ip from NA; if empty, bind to current client IP for sticky cache
        let login_ip = body
            .login_ip
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| client_ip.to_string());

        // Only sticky-cache when current IP matches login_ip.
        // If IP differs but NA still allowed (non-strict), do NOT cache as same-IP fast path
        // for the wrong IP — next request with yet another IP must re-ask.
        // Cache under login_ip so only requests from login_ip skip NA.
        self.cache_put(fp, exp, login_ip.clone()).await;

        if !ips_equal(client_ip, &login_ip) && !login_ip.is_empty() {
            tracing::info!(
                %client_ip,
                %login_ip,
                "NA allowed despite IP mismatch (not sticky for this IP)"
            );
            // Allowed this once; subsequent same mismatched IP will ask NA again
            // (cache only helps when client_ip == login_ip)
        } else {
            tracing::debug!(%client_ip, %login_ip, "NA verify ok, cached");
        }

        Ok(true)
    }
}

fn chrono_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn ips_equal(a: &str, b: &str) -> bool {
    normalize_ip(a) == normalize_ip(b)
}

fn normalize_ip(s: &str) -> String {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix("::ffff:") {
        return rest.to_string();
    }
    s.to_string()
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
/// Protected: master key | local JWT (same IP) | NA verify (first / IP change)
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

    if auth.master_key.is_empty() && !auth.jwt_enabled() && !auth.na_verify_enabled() {
        if is_prod() {
            return Err(AppError::Internal(
                "No NA_VERIFY_URL / LITELLM_MASTER_KEY / JWT_PUBLIC_KEY configured".into(),
            ));
        }
        return Ok(next.run(req).await);
    }

    let Some(token) = extract_token(&req) else {
        return Err(AppError::Unauthorized(
            "Missing credentials. Use Authorization: Bearer <jwt> or x-api-key".into(),
        ));
    };

    let client_ip = client_ip_from_request(&req);

    match auth.authorize_token(&token, &client_ip).await {
        Ok(true) => Ok(next.run(req).await),
        Ok(false) => Err(AppError::Unauthorized(
            "Not allowed (JWT/NA verify failed or IP not permitted)".into(),
        )),
        Err(e) => Err(e),
    }
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

fn client_ip_from_request(req: &Request) -> String {
    // Cloud Run / proxies: first X-Forwarded-For hop is original client
    if let Some(xff) = req
        .headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
    {
        let first = xff.split(',').next().unwrap_or("").trim();
        if !first.is_empty() {
            return first.to_string();
        }
    }
    if let Some(ip) = req
        .headers()
        .get("x-real-ip")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        return ip.to_string();
    }
    // Fallback: unknown — cache key will force more NA checks
    "0.0.0.0".into()
}
