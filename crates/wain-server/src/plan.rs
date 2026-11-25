//! Running the layout engine over HTTP.

use axum::Json;
use wain_core::{Plan, Project};

use crate::error::{ApiError, ApiJson};

/// Lays out a project. The browser normally does this itself in WebAssembly;
/// the server needs it to render plans and to re-check any project a client
/// sends.
pub async fn create_plan(ApiJson(project): ApiJson<Project>) -> Result<Json<Plan>, ApiError> {
    wain_core::plan(&project)
        .map(Json)
        .map_err(|invalid| ApiError::InvalidProject(invalid.issues))
}
