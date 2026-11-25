//! The wain HTTP API.
//!
//! Accounts and sign-in, the credit ledger that meters paid features, AI
//! design suggestions, and the layout engine over HTTP. Everything is served
//! under `/api/v1`.

pub mod auth;
pub mod billing;
pub mod config;
pub mod credits;
pub mod error;
pub mod mail;
pub mod plan;
pub mod suggest;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::routing::{get, post};
use sqlx::PgPool;

use mail::Mailer;
use suggest::Suggester;

/// Embedded database migrations, applied at startup.
pub static MIGRATIONS: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Largest request body for the suggestions upload: a photo plus a project.
const UPLOAD_LIMIT: usize = 8 * 1024 * 1024;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub settings: Arc<Settings>,
    pub mailer: Arc<dyn Mailer>,
    /// `None` when no AI provider is configured.
    pub suggester: Option<Arc<dyn Suggester>>,
    /// `None` when the server is not set up to sell credits.
    pub billing: Option<Arc<billing::Billing>>,
}

pub struct Settings {
    /// Keys the hashes of sign-in codes.
    pub secret: Vec<u8>,
    /// Development mode; see [`config::Config::dev`].
    pub dev: bool,
}

pub fn app(state: AppState) -> Router {
    let api = Router::new()
        .route("/plan", post(plan::create_plan))
        .route(
            "/suggestions",
            post(suggest::create_suggestions).layer(DefaultBodyLimit::max(UPLOAD_LIMIT)),
        )
        .route("/billing/packs", get(billing::packs))
        .route("/billing/checkout", post(billing::checkout))
        .route("/billing/webhook", post(billing::webhook))
        .merge(auth::routes())
        .layer(axum::middleware::from_fn(auth::same_origin));

    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .nest("/api/v1", api)
        .with_state(state)
}
