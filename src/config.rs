use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub master_key: String,
    pub drop_params: bool,
    pub timeout: Duration,
    pub models: HashMap<String, ModelRoute>,
    pub model_names: Vec<String>,
    pub config_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ModelRoute {
    pub model_name: String,
    pub provider: ProviderKind,
    pub upstream_model: String,
    pub api_base: String,
    pub api_key_env: String,
    pub api_key: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    OpenAiCompatible,
    Anthropic,
}

impl ProviderKind {
    fn parse(s: Option<&str>) -> Self {
        match s.map(|x| x.to_ascii_lowercase()).as_deref() {
            Some("anthropic") => Self::Anthropic,
            _ => Self::OpenAiCompatible,
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawConfig {
    general_settings: Option<RawGeneral>,
    litellm_settings: Option<RawLite>,
    model_list: Vec<RawModel>,
}

#[derive(Debug, Deserialize)]
struct RawGeneral {
    master_key_env: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawLite {
    drop_params: Option<bool>,
    request_timeout_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct RawModel {
    model_name: String,
    litellm_params: RawParams,
}

#[derive(Debug, Deserialize)]
struct RawParams {
    provider: Option<String>,
    model: String,
    api_base: String,
    api_key_env: Option<String>,
    api_key: Option<String>,
}

impl AppConfig {
    pub fn load() -> Result<Self> {
        let path = env::var("CONFIG_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("config.yaml"));
        Self::from_path(&path)
    }

    pub fn from_path(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("read config {}", path.display()))?;
        let raw: RawConfig = serde_yaml::from_str(&text).context("parse config.yaml")?;

        let master_env = raw
            .general_settings
            .as_ref()
            .and_then(|g| g.master_key_env.clone())
            .unwrap_or_else(|| "LITELLM_MASTER_KEY".into());
        let master_key = env::var(&master_env)
            .or_else(|_| env::var("LITELLM_MASTER_KEY"))
            .unwrap_or_default();

        let drop_params = raw
            .litellm_settings
            .as_ref()
            .and_then(|s| s.drop_params)
            .unwrap_or(true);

        let timeout_ms = raw
            .litellm_settings
            .as_ref()
            .and_then(|s| s.request_timeout_ms)
            .or_else(|| {
                env::var("REQUEST_TIMEOUT_MS")
                    .ok()
                    .and_then(|v| v.parse().ok())
            })
            .unwrap_or(300_000);

        let mut models = HashMap::new();
        let mut model_names = Vec::new();

        for entry in raw.model_list {
            let key_env = entry
                .litellm_params
                .api_key_env
                .clone()
                .unwrap_or_default();
            let api_key = if !key_env.is_empty() {
                env::var(&key_env).unwrap_or_default()
            } else {
                entry.litellm_params.api_key.clone().unwrap_or_default()
            };

            let api_base = entry.litellm_params.api_base.trim_end_matches('/').to_string();
            if api_base.is_empty() {
                bail!("model {} missing api_base", entry.model_name);
            }

            let route = ModelRoute {
                model_name: entry.model_name.clone(),
                provider: ProviderKind::parse(entry.litellm_params.provider.as_deref()),
                upstream_model: entry.litellm_params.model,
                api_base,
                api_key_env: key_env,
                api_key,
            };
            model_names.push(entry.model_name.clone());
            models.insert(entry.model_name, route);
        }

        if models.is_empty() {
            bail!("config model_list is empty");
        }

        Ok(Self {
            master_key,
            drop_params,
            timeout: Duration::from_millis(timeout_ms),
            models,
            model_names,
            config_path: path.to_path_buf(),
        })
    }

    pub fn resolve(&self, name: &str) -> Option<&ModelRoute> {
        self.models.get(name)
    }
}

pub fn port() -> u16 {
    env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(4000)
}

pub fn is_cloud_run() -> bool {
    env::var("K_SERVICE").is_ok()
}

pub fn is_prod() -> bool {
    is_cloud_run() || env::var("NODE_ENV").as_deref() == Ok("production")
        || env::var("RUST_ENV").as_deref() == Ok("production")
}
