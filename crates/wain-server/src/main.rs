use std::net::SocketAddr;
use std::sync::Arc;

use sqlx::postgres::PgPoolOptions;
use wain_server::config::Config;
use wain_server::mail::LogMailer;
use wain_server::suggest::{CannedSuggester, ClaudeSuggester, Suggester};
use wain_server::{AppState, MIGRATIONS, Settings, app};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // A `.env` file is a convenience for local development; real deployments
    // set the environment directly.
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "wain_server=info".into()),
        )
        .init();

    let config = Config::from_env()?;
    if config.dev {
        tracing::warn!("development mode: sign-in codes are logged and returned to the browser");
    }

    let db = PgPoolOptions::new()
        .max_connections(10)
        .connect(&config.database_url)
        .await?;
    MIGRATIONS.run(&db).await?;

    let suggester: Option<Arc<dyn Suggester>> = match config.anthropic_api_key {
        _ if config.fake_ai => {
            tracing::warn!("WAIN_FAKE_AI is set: suggestions are fixed samples, not from a model");
            Some(Arc::new(CannedSuggester))
        }
        Some(key) => Some(Arc::new(ClaudeSuggester::new(key)?)),
        None => {
            tracing::warn!("ANTHROPIC_API_KEY is not set: AI suggestions are disabled");
            None
        }
    };

    let state = AppState {
        db,
        settings: Arc::new(Settings {
            secret: config.secret,
            dev: config.dev,
        }),
        mailer: Arc::new(LogMailer),
        suggester,
    };

    let address = SocketAddr::from(([127, 0, 0, 1], config.port));
    let listener = tokio::net::TcpListener::bind(address).await?;
    tracing::info!("listening on http://{address}");
    axum::serve(listener, app(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
