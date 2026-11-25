//! The wain HTTP API.
//!
//! For now this serves a health check and the layout engine. Accounts,
//! project storage, AI suggestions and billing are added in later phases.

use std::net::SocketAddr;

use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Serialize;
use wain_core::{Issue, Project};

#[tokio::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "wain_server=info".into()),
        )
        .init();

    let port = std::env::var("PORT")
        .ok()
        .and_then(|port| port.parse().ok())
        .unwrap_or(8080);
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(address).await?;
    tracing::info!("listening on http://{address}");

    axum::serve(listener, app())
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
}

fn app() -> Router {
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/api/v1/plan", post(create_plan))
}

/// The error body for every failed API request.
#[derive(Serialize)]
struct ApiError {
    error: &'static str,
    message: String,
    /// Per-field problems, for `invalid_project`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    issues: Vec<Issue>,
}

/// Runs the layout engine on a project. The browser normally does this itself
/// in WebAssembly; the server needs it to render plans and to re-check any
/// project a client sends.
async fn create_plan(body: Result<Json<Project>, JsonRejection>) -> Response {
    let Json(project) = match body {
        Ok(body) => body,
        Err(rejection) => {
            let error = ApiError {
                error: "bad_request",
                message: rejection.body_text(),
                issues: Vec::new(),
            };
            return (rejection.status(), Json(error)).into_response();
        }
    };
    match wain_core::plan(&project) {
        Ok(plan) => Json(plan).into_response(),
        Err(invalid) => {
            let error = ApiError {
                error: "invalid_project",
                message: "the project has problems that must be fixed first".to_owned(),
                issues: invalid.issues,
            };
            (StatusCode::UNPROCESSABLE_ENTITY, Json(error)).into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use axum::body::{Body, to_bytes};
    use axum::http::{Request, header};
    use serde_json::Value;
    use tower::ServiceExt;
    use wain_core::{Length, UnitSystem};

    use super::*;

    async fn post_plan(body: String) -> (StatusCode, Value) {
        let request = Request::post("/api/v1/plan")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .unwrap();
        let response = app().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    #[tokio::test]
    async fn health_check_responds() {
        let request = Request::get("/healthz").body(Body::empty()).unwrap();
        let response = app().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn plans_a_valid_project() {
        let project = Project::starter(UnitSystem::Imperial);
        let (status, body) = post_plan(serde_json::to_string(&project).unwrap()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["cut_list"][0]["kind"], "batten");
        assert_eq!(body["cut_list"][0]["quantity"], 10);
    }

    #[tokio::test]
    async fn reports_issues_for_an_invalid_project() {
        let mut project = Project::starter(UnitSystem::Imperial);
        project.walls[0].width = Length::ZERO;
        let (status, body) = post_plan(serde_json::to_string(&project).unwrap()).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["error"], "invalid_project");
        assert_eq!(body["issues"][0]["path"], "walls[0].width");
    }

    #[tokio::test]
    async fn rejects_a_malformed_body_as_json() {
        let (status, body) = post_plan("{\"name\": 1}".to_owned()).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["error"], "bad_request");
    }
}
