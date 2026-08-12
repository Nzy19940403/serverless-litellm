mod auth;
mod config;
mod error;
mod providers;
mod routes;

use axum::middleware;
use axum::routing::{get, post};
use axum::Router;
use std::net::SocketAddr;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::auth::master_key_auth;
use crate::config::{port, AppConfig};
use crate::routes::{chat_completions, health, healthz, list_models, root, ui_index, AppState};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "serverless_litellm=info,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = AppConfig::load()?;
    tracing::info!(
        config = %config.config_path.display(),
        models = ?config.model_names,
        master_key_set = !config.master_key.is_empty(),
        "loaded config"
    );

    let state = AppState::new(config);
    let cfg_for_auth = state.config.clone();

    let app = Router::new()
        .route("/", get(root))
        .route("/ui", get(ui_index))
        .route("/ui/", get(ui_index))
        .route("/health", get(health))
        .route("/healthz", get(healthz))
        .route("/v1/models", get(list_models))
        .route("/v1/chat/completions", post(chat_completions))
        .layer(middleware::from_fn_with_state(
            cfg_for_auth,
            master_key_auth,
        ))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port()));
    tracing::info!("serverless-litellm listening on {addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
