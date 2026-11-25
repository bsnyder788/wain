//! The one error type every handler returns.

use axum::Json;
use axum::extract::{FromRequest, Request};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use serde::de::DeserializeOwned;
use wain_core::Issue;

#[derive(Debug)]
pub enum ApiError {
    BadRequest(String),
    /// No valid session.
    Unauthorized,
    Forbidden(&'static str),
    TooManyRequests(&'static str),
    /// The project failed the engine's validation.
    InvalidProject(Vec<Issue>),
    /// The account has too few credits for the action.
    InsufficientCredits,
    /// A feature this deployment has not been configured for.
    Unavailable(&'static str),
    /// A service we depend on failed; the user was not charged.
    Upstream(&'static str),
    /// A bug or an infrastructure failure. Details go to the log, not the client.
    Internal,
}

/// The JSON body of every error response.
#[derive(Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ApiErrorBody {
    /// A stable machine-readable code, e.g. `insufficient_credits`.
    pub error: String,
    /// A sentence that can be shown to the user.
    pub message: String,
    /// Per-field problems, for `invalid_project`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(feature = "ts", ts(optional, as = "Option<Vec<Issue>>"))]
    pub issues: Vec<Issue>,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code, message, issues) = match self {
            ApiError::BadRequest(message) => {
                (StatusCode::BAD_REQUEST, "bad_request", message, Vec::new())
            }
            ApiError::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "Sign in to continue.".to_owned(),
                Vec::new(),
            ),
            ApiError::Forbidden(message) => (
                StatusCode::FORBIDDEN,
                "forbidden",
                message.to_owned(),
                Vec::new(),
            ),
            ApiError::TooManyRequests(message) => (
                StatusCode::TOO_MANY_REQUESTS,
                "too_many_requests",
                message.to_owned(),
                Vec::new(),
            ),
            ApiError::InvalidProject(issues) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid_project",
                "The project has problems that must be fixed first.".to_owned(),
                issues,
            ),
            ApiError::InsufficientCredits => (
                StatusCode::PAYMENT_REQUIRED,
                "insufficient_credits",
                "You are out of credits.".to_owned(),
                Vec::new(),
            ),
            ApiError::Unavailable(message) => (
                StatusCode::SERVICE_UNAVAILABLE,
                "unavailable",
                message.to_owned(),
                Vec::new(),
            ),
            ApiError::Upstream(message) => (
                StatusCode::BAD_GATEWAY,
                "upstream",
                message.to_owned(),
                Vec::new(),
            ),
            ApiError::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "Something went wrong on our side.".to_owned(),
                Vec::new(),
            ),
        };
        let body = ApiErrorBody {
            error: code.to_owned(),
            message,
            issues,
        };
        (status, Json(body)).into_response()
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(error: sqlx::Error) -> ApiError {
        tracing::error!(%error, "database error");
        ApiError::Internal
    }
}

/// `Json`, but a malformed body becomes an [`ApiError`] like everything else.
pub struct ApiJson<T>(pub T);

impl<S, T> FromRequest<S> for ApiJson<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = ApiError;

    async fn from_request(request: Request, state: &S) -> Result<Self, ApiError> {
        match Json::<T>::from_request(request, state).await {
            Ok(Json(value)) => Ok(ApiJson(value)),
            Err(rejection) => Err(ApiError::BadRequest(rejection.body_text())),
        }
    }
}
