//! Accounts, sign-in by emailed code, and sessions.
//!
//! Signing in takes two calls. `POST /auth/start` emails a six-digit code and
//! returns an unguessable challenge id; `POST /auth/verify` exchanges the id
//! and the code for a session cookie. A code works in the mobile apps as well
//! as the browser, which a link in an email would not without deep linking.
//!
//! A six-digit code is guessable, so each challenge allows only a few
//! attempts, expires quickly, and is useless without its challenge id.

use axum::extract::{FromRequestParts, Request, State};
use axum::http::header::{COOKIE, HOST, ORIGIN, SET_COOKIE};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, Method};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::{Json, Router, routing::get, routing::post};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use uuid::Uuid;

use crate::AppState;
use crate::credits;
use crate::error::{ApiError, ApiJson};

const SESSION_COOKIE: &str = "wain_session";
const SESSION_DAYS: i64 = 30;
const CODE_MINUTES: i32 = 10;
const MAX_ATTEMPTS: i32 = 5;
/// Codes one address can request per quarter hour.
const MAX_CODES_PER_WINDOW: i64 = 5;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/auth/start", post(start))
        .route("/auth/verify", post(verify))
        .route("/auth/signout", post(sign_out))
        .route("/me", get(me))
}

/// The signed-in user, for handlers that require one.
pub struct CurrentUser {
    pub id: Uuid,
    pub email: String,
}

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let token = session_token(&parts.headers).ok_or(ApiError::Unauthorized)?;
        let row: Option<(Uuid, String)> = sqlx::query_as(
            "select users.id, users.email from sessions
             join users on users.id = sessions.user_id
             where sessions.token_hash = $1 and sessions.expires_at > now()",
        )
        .bind(hash(&[token.as_bytes()]))
        .fetch_optional(&state.db)
        .await?;
        let (id, email) = row.ok_or(ApiError::Unauthorized)?;
        Ok(CurrentUser { id, email })
    }
}

/// What the client knows about the signed-in account.
#[derive(Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Account {
    pub email: String,
    pub credits: i32,
}

#[derive(Deserialize)]
struct StartRequest {
    email: String,
}

#[derive(Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct StartResponse {
    pub challenge_id: Uuid,
    /// The code itself, only when the server runs in development mode, so
    /// that signing in does not need a mail provider.
    pub dev_code: Option<String>,
}

async fn start(
    State(state): State<AppState>,
    ApiJson(request): ApiJson<StartRequest>,
) -> Result<Json<StartResponse>, ApiError> {
    let email = normalize_email(&request.email)
        .ok_or_else(|| ApiError::BadRequest("Enter a valid email address.".to_owned()))?;

    let recent: i64 = sqlx::query_scalar(
        "select count(*) from login_challenges
         where email = $1 and created_at > now() - interval '15 minutes'",
    )
    .bind(&email)
    .fetch_one(&state.db)
    .await?;
    if recent >= MAX_CODES_PER_WINDOW {
        return Err(ApiError::TooManyRequests(
            "Too many codes requested. Wait a few minutes and try again.",
        ));
    }

    let challenge_id = Uuid::new_v4();
    let code = new_code()?;
    sqlx::query(
        "insert into login_challenges (id, email, code_hash, expires_at)
         values ($1, $2, $3, now() + make_interval(mins => $4))",
    )
    .bind(challenge_id)
    .bind(&email)
    .bind(code_hash(&state, challenge_id, &code))
    .bind(CODE_MINUTES)
    .execute(&state.db)
    .await?;

    state
        .mailer
        .send_code(&email, &code)
        .await
        .map_err(|error| {
            tracing::error!(%error, "could not send sign-in code");
            ApiError::Upstream("We could not send the email. Try again shortly.")
        })?;

    // The response is the same whether or not the address has an account.
    Ok(Json(StartResponse {
        challenge_id,
        dev_code: state.settings.dev.then_some(code),
    }))
}

#[derive(Deserialize)]
struct VerifyRequest {
    challenge_id: Uuid,
    code: String,
}

async fn verify(
    State(state): State<AppState>,
    ApiJson(request): ApiJson<VerifyRequest>,
) -> Result<Response, ApiError> {
    let invalid = || ApiError::BadRequest("That code is wrong or has expired.".to_owned());

    // Count the attempt before checking it, in one statement, so parallel
    // guesses cannot exceed the limit.
    let challenge: Option<(String, Vec<u8>)> = sqlx::query_as(
        "update login_challenges set attempts = attempts + 1
         where id = $1 and consumed_at is null and expires_at > now() and attempts < $2
         returning email, code_hash",
    )
    .bind(request.challenge_id)
    .bind(MAX_ATTEMPTS)
    .fetch_optional(&state.db)
    .await?;
    let (email, expected) = challenge.ok_or_else(invalid)?;

    let given = code_hash(&state, request.challenge_id, request.code.trim());
    if !bool::from(given.ct_eq(&expected)) {
        return Err(invalid());
    }

    let mut tx = state.db.begin().await?;
    // Only one request can consume a challenge.
    let consumed = sqlx::query(
        "update login_challenges set consumed_at = now() where id = $1 and consumed_at is null",
    )
    .bind(request.challenge_id)
    .execute(&mut *tx)
    .await?;
    if consumed.rows_affected() == 0 {
        return Err(invalid());
    }

    let created: Option<Uuid> = sqlx::query_scalar(
        "insert into users (id, email) values ($1, $2) on conflict (email) do nothing returning id",
    )
    .bind(Uuid::new_v4())
    .bind(&email)
    .fetch_optional(&mut *tx)
    .await?;
    let user_id = match created {
        Some(id) => {
            credits::grant(&mut *tx, id, credits::SIGNUP_GRANT, "signup_grant").await?;
            id
        }
        None => {
            sqlx::query_scalar("select id from users where email = $1")
                .bind(&email)
                .fetch_one(&mut *tx)
                .await?
        }
    };

    let token = new_token()?;
    sqlx::query(
        "insert into sessions (token_hash, user_id, expires_at)
         values ($1, $2, now() + make_interval(days => $3))",
    )
    .bind(hash(&[token.as_bytes()]))
    .bind(user_id)
    .bind(SESSION_DAYS as i32)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    let account = Account {
        email,
        credits: credits::balance(&state.db, user_id).await?,
    };
    let cookie = session_cookie(&token, SESSION_DAYS * 24 * 60 * 60, state.settings.dev);
    Ok(([(SET_COOKIE, cookie)], Json(account)).into_response())
}

async fn sign_out(State(state): State<AppState>, headers: HeaderMap) -> Result<Response, ApiError> {
    if let Some(token) = session_token(&headers) {
        sqlx::query("delete from sessions where token_hash = $1")
            .bind(hash(&[token.as_bytes()]))
            .execute(&state.db)
            .await?;
    }
    let cookie = session_cookie("", 0, state.settings.dev);
    Ok(([(SET_COOKIE, cookie)], Json(serde_json::json!({}))).into_response())
}

async fn me(State(state): State<AppState>, user: CurrentUser) -> Result<Json<Account>, ApiError> {
    Ok(Json(Account {
        credits: credits::balance(&state.db, user.id).await?,
        email: user.email,
    }))
}

/// Rejects state-changing requests that a browser sent from another site.
///
/// The session cookie is `SameSite=Lax`, which already keeps it off
/// cross-site POSTs; this is a second, independent check. A browser always
/// sends `Origin` on such requests, and it must name the host being talked
/// to. Requests without the header (native apps, scripts) carry no ambient
/// browser credentials and are let through.
pub async fn same_origin(request: Request, next: Next) -> Response {
    let safe = matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    );
    if !safe && let Some(origin) = request.headers().get(ORIGIN) {
        let origin_host = origin
            .to_str()
            .ok()
            .and_then(|origin| origin.split_once("://"))
            .map(|(_, authority)| hostname(authority));
        let host = request
            .headers()
            .get(HOST)
            .and_then(|host| host.to_str().ok())
            .map(hostname);
        if origin_host.is_none() || origin_host != host {
            return ApiError::Forbidden("This request came from another site.").into_response();
        }
    }
    next.run(request).await
}

/// The host of an authority, lowercased and without any port.
fn hostname(authority: &str) -> String {
    let host = match authority.strip_prefix('[') {
        // An IPv6 literal: keep what is inside the brackets.
        Some(rest) => rest.split(']').next().unwrap_or(rest),
        None => authority.split(':').next().unwrap_or(authority),
    };
    host.to_ascii_lowercase()
}

/// Trims and lowercases an address, and checks it has the basic shape of
/// one. Whether it really exists is proved by the code arriving.
fn normalize_email(input: &str) -> Option<String> {
    let email = input.trim().to_lowercase();
    let (local, domain) = email.split_once('@')?;
    let plausible = !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !domain.contains('@')
        && email.len() <= 254
        && !email.chars().any(|c| c.is_whitespace() || c.is_control());
    plausible.then_some(email)
}

fn session_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, _)| *name == SESSION_COOKIE)
        .map(|(_, value)| value.to_owned())
        .filter(|value| !value.is_empty())
}

fn session_cookie(token: &str, max_age_seconds: i64, dev: bool) -> HeaderValue {
    // Plain HTTP on localhost cannot store a `Secure` cookie in every browser.
    let secure = if dev { "" } else { "; Secure" };
    let cookie = format!(
        "{SESSION_COOKIE}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age_seconds}{secure}"
    );
    HeaderValue::from_str(&cookie).expect("session tokens are hex")
}

fn hash(parts: &[&[u8]]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    for part in parts {
        // Length-prefix each part so different splits cannot collide.
        hasher.update((part.len() as u64).to_be_bytes());
        hasher.update(part);
    }
    hasher.finalize().to_vec()
}

fn code_hash(state: &AppState, challenge_id: Uuid, code: &str) -> Vec<u8> {
    hash(&[
        &state.settings.secret,
        challenge_id.as_bytes(),
        code.as_bytes(),
    ])
}

fn random_error(error: getrandom::Error) -> ApiError {
    tracing::error!(%error, "the system random number generator failed");
    ApiError::Internal
}

/// 256 bits from the operating system, as hex.
fn new_token() -> Result<String, ApiError> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(random_error)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// A uniformly random six-digit code.
fn new_code() -> Result<String, ApiError> {
    // Reject the top sliver of the range so every code is equally likely.
    const LIMIT: u32 = u32::MAX - u32::MAX % 1_000_000;
    loop {
        let value = getrandom::u32().map_err(random_error)?;
        if value < LIMIT {
            return Ok(format!("{:06}", value % 1_000_000));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emails_are_normalized_and_sanity_checked() {
        assert_eq!(
            normalize_email("  Ada@Example.COM "),
            Some("ada@example.com".into())
        );
        for bad in [
            "",
            "ada",
            "ada@",
            "@example.com",
            "ada@example",
            "a da@example.com",
            "ada@.com",
            "ada@example.",
            "a@b@c.com",
        ] {
            assert_eq!(normalize_email(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn hostnames_drop_ports_and_case() {
        assert_eq!(hostname("Example.com:5173"), "example.com");
        assert_eq!(hostname("localhost"), "localhost");
        assert_eq!(hostname("[::1]:8787"), "::1");
    }

    #[test]
    fn the_session_cookie_is_found_among_others() {
        let mut headers = HeaderMap::new();
        headers.insert(
            COOKIE,
            HeaderValue::from_static("theme=dark; wain_session=abc123; x=1"),
        );
        assert_eq!(session_token(&headers), Some("abc123".into()));
        headers.insert(COOKIE, HeaderValue::from_static("wain_session="));
        assert_eq!(session_token(&headers), None);
        headers.insert(COOKIE, HeaderValue::from_static("not_wain_session=abc"));
        assert_eq!(session_token(&headers), None);
    }

    #[test]
    fn codes_are_six_digits_and_tokens_are_long() {
        for _ in 0..50 {
            let code = new_code().unwrap();
            assert!(
                code.len() == 6 && code.bytes().all(|b| b.is_ascii_digit()),
                "{code}"
            );
        }
        assert_eq!(new_token().unwrap().len(), 64);
        assert_ne!(new_token().unwrap(), new_token().unwrap());
    }

    #[test]
    fn hash_parts_do_not_run_together() {
        assert_ne!(hash(&[b"ab", b"c"]), hash(&[b"a", b"bc"]));
    }
}
