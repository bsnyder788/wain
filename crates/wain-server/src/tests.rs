//! API tests. Each runs against its own freshly migrated database, which
//! `#[sqlx::test]` creates from `DATABASE_URL` (see `scripts/dev-db.sh`).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::header::{CONTENT_TYPE, COOKIE, HOST, ORIGIN, SET_COOKIE};
use axum::http::{Request, StatusCode};
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;
use wain_core::{Length, Project, StyleKind, UnitSystem};

use crate::mail::{BoxFuture, Mailer};
use crate::suggest::{RawSuggestion, RawSuggestions, SuggestError, SuggestInput, Suggester};
use crate::{AppState, Settings, app, credits};

const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F'];

/// Records what would have been emailed.
#[derive(Default)]
struct Outbox(Mutex<Vec<(String, String)>>);

impl Mailer for Outbox {
    fn send_code<'a>(&'a self, email: &'a str, code: &'a str) -> BoxFuture<'a, Result<(), String>> {
        self.0
            .lock()
            .unwrap()
            .push((email.to_owned(), code.to_owned()));
        Box::pin(async { Ok(()) })
    }
}

/// What a [`FakeSuggester`] answers with.
type Answer = fn() -> Result<RawSuggestions, SuggestError>;

/// Answers every request the same way and remembers what it was asked.
struct FakeSuggester {
    answer: Answer,
    calls: AtomicUsize,
    last_brief: Mutex<String>,
}

impl FakeSuggester {
    fn new(answer: Answer) -> Arc<FakeSuggester> {
        Arc::new(FakeSuggester {
            answer,
            calls: AtomicUsize::new(0),
            last_brief: Mutex::default(),
        })
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl Suggester for FakeSuggester {
    fn suggest<'a>(
        &'a self,
        input: &'a SuggestInput,
    ) -> BoxFuture<'a, Result<RawSuggestions, SuggestError>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        *self.last_brief.lock().unwrap() = input.brief.clone();
        assert_eq!(input.media_type, "image/jpeg");
        Box::pin(async { (self.answer)() })
    }
}

fn suggestion(style: StyleKind, height_mm: f64) -> RawSuggestion {
    RawSuggestion {
        title: "A title".into(),
        rationale: "A reason.".into(),
        style,
        height_mm,
        target_spacing_mm: 400.0,
        member_width_mm: 65.0,
        rail_width_mm: 90.0,
    }
}

fn good_answer() -> Result<RawSuggestions, SuggestError> {
    Ok(RawSuggestions {
        observations: "A plain wall with a low sill.".into(),
        suggestions: vec![
            suggestion(StyleKind::BoardAndBatten, 1370.0),
            // Taller than the ceiling: the engine cannot build it.
            suggestion(StyleKind::ShakerPanel, 5000.0),
            suggestion(StyleKind::PictureFrame, 900.0),
        ],
    })
}

struct Harness {
    app: Router,
    db: PgPool,
    outbox: Arc<Outbox>,
}

fn harness(db: PgPool, dev: bool, suggester: Option<Arc<dyn Suggester>>) -> Harness {
    let outbox = Arc::new(Outbox::default());
    let state = AppState {
        db: db.clone(),
        settings: Arc::new(Settings {
            secret: b"a test secret that is long enough".to_vec(),
            dev,
        }),
        mailer: outbox.clone(),
        suggester,
    };
    Harness {
        app: app(state),
        db,
        outbox,
    }
}

struct Reply {
    status: StatusCode,
    cookie: Option<String>,
    body: Value,
}

impl Harness {
    async fn send(&self, request: Request<Body>) -> Reply {
        let response = self.app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let cookie = response
            .headers()
            .get(SET_COOKIE)
            .map(|value| value.to_str().unwrap().to_owned());
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        Reply {
            status,
            cookie,
            body,
        }
    }

    async fn post(&self, path: &str, body: Value, session: Option<&str>) -> Reply {
        let mut request = Request::post(format!("/api/v1{path}"))
            .header(HOST, "app.test")
            .header(ORIGIN, "https://app.test")
            .header(CONTENT_TYPE, "application/json");
        if let Some(session) = session {
            request = request.header(COOKIE, session);
        }
        self.send(request.body(Body::from(body.to_string())).unwrap())
            .await
    }

    async fn get(&self, path: &str, session: Option<&str>) -> Reply {
        let mut request = Request::get(format!("/api/v1{path}")).header(HOST, "app.test");
        if let Some(session) = session {
            request = request.header(COOKIE, session);
        }
        self.send(request.body(Body::empty()).unwrap()).await
    }

    /// Signs in and returns the `Cookie` header value for the session.
    async fn sign_in(&self, email: &str) -> String {
        let started = self
            .post("/auth/start", json!({ "email": email }), None)
            .await;
        assert_eq!(started.status, StatusCode::OK, "{}", started.body);
        let code = self.outbox.0.lock().unwrap().last().unwrap().1.clone();
        let verified = self
            .post(
                "/auth/verify",
                json!({ "challenge_id": started.body["challenge_id"], "code": code }),
                None,
            )
            .await;
        assert_eq!(verified.status, StatusCode::OK, "{}", verified.body);
        let cookie = verified.cookie.expect("a session cookie");
        cookie.split(';').next().unwrap().to_owned()
    }

    async fn credits(&self, session: &str) -> i64 {
        self.get("/me", Some(session)).await.body["credits"]
            .as_i64()
            .unwrap()
    }

    async fn suggest(&self, project: &Project, image: &[u8], session: Option<&str>) -> Reply {
        let boundary = "wain-test-boundary";
        let mut body = Vec::new();
        let mut part = |name: &str, filename: Option<&str>, content: &[u8]| {
            let file = filename
                .map(|f| format!("; filename=\"{f}\""))
                .unwrap_or_default();
            body.extend(
                format!(
                    "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"{file}\r\n\r\n"
                )
                .into_bytes(),
            );
            body.extend(content);
            body.extend(b"\r\n");
        };
        part(
            "project",
            None,
            serde_json::to_string(project).unwrap().as_bytes(),
        );
        part("wall", None, b"0");
        part("image", Some("wall.jpg"), image);
        body.extend(format!("--{boundary}--\r\n").into_bytes());

        let mut request = Request::post("/api/v1/suggestions")
            .header(HOST, "app.test")
            .header(ORIGIN, "https://app.test")
            .header(
                CONTENT_TYPE,
                format!("multipart/form-data; boundary={boundary}"),
            );
        if let Some(session) = session {
            request = request.header(COOKIE, session);
        }
        self.send(request.body(Body::from(body)).unwrap()).await
    }
}

fn project() -> Project {
    Project::starter(UnitSystem::Imperial)
}

// ---- Sign-in ----------------------------------------------------------------

#[sqlx::test]
async fn signing_in_creates_an_account_with_starting_credits(db: PgPool) {
    let h = harness(db, false, None);
    assert_eq!(h.get("/me", None).await.status, StatusCode::UNAUTHORIZED);

    let session = h.sign_in("Ada@Example.com").await;
    let me = h.get("/me", Some(&session)).await;
    assert_eq!(me.status, StatusCode::OK);
    assert_eq!(
        me.body,
        json!({ "email": "ada@example.com", "credits": credits::SIGNUP_GRANT })
    );
    assert_eq!(h.outbox.0.lock().unwrap()[0].0, "ada@example.com");

    // Signing in again, however the address is typed, is the same account
    // and earns no second grant.
    let again = h.sign_in("  ada@EXAMPLE.com ").await;
    assert_ne!(again, session);
    assert_eq!(h.credits(&again).await, i64::from(credits::SIGNUP_GRANT));
    let users: i64 = sqlx::query_scalar("select count(*) from users")
        .fetch_one(&h.db)
        .await
        .unwrap();
    assert_eq!(users, 1);
}

#[sqlx::test]
async fn production_mode_never_reveals_the_code_and_secures_the_cookie(db: PgPool) {
    let h = harness(db, false, None);
    let started = h
        .post("/auth/start", json!({ "email": "ada@example.com" }), None)
        .await;
    assert_eq!(started.body["dev_code"], Value::Null);

    let code = h.outbox.0.lock().unwrap()[0].1.clone();
    let verified = h
        .post(
            "/auth/verify",
            json!({ "challenge_id": started.body["challenge_id"], "code": code }),
            None,
        )
        .await;
    let cookie = verified.cookie.unwrap();
    for attribute in [
        "HttpOnly",
        "SameSite=Lax",
        "Secure",
        "Path=/",
        "Max-Age=2592000",
    ] {
        assert!(cookie.contains(attribute), "{cookie} lacks {attribute}");
    }
    // Only a hash of the token is stored.
    let token = cookie
        .split(';')
        .next()
        .unwrap()
        .split_once('=')
        .unwrap()
        .1
        .to_owned();
    let stored: Vec<u8> = sqlx::query_scalar("select token_hash from sessions")
        .fetch_one(&h.db)
        .await
        .unwrap();
    assert_ne!(stored, token.as_bytes());
    assert_eq!(stored.len(), 32);
}

#[sqlx::test]
async fn development_mode_returns_the_code_for_local_sign_in(db: PgPool) {
    let h = harness(db, true, None);
    let started = h
        .post("/auth/start", json!({ "email": "ada@example.com" }), None)
        .await;
    let code = started.body["dev_code"].as_str().unwrap().to_owned();
    assert_eq!(code, h.outbox.0.lock().unwrap()[0].1);
    let verified = h
        .post(
            "/auth/verify",
            json!({ "challenge_id": started.body["challenge_id"], "code": code }),
            None,
        )
        .await;
    assert_eq!(verified.status, StatusCode::OK);
    assert!(!verified.cookie.unwrap().contains("Secure"));
}

#[sqlx::test]
async fn a_wrong_code_fails_and_guesses_are_limited(db: PgPool) {
    let h = harness(db, false, None);
    let started = h
        .post("/auth/start", json!({ "email": "ada@example.com" }), None)
        .await;
    let challenge = started.body["challenge_id"].clone();
    let code = h.outbox.0.lock().unwrap()[0].1.clone();
    let wrong = if code == "000000" { "000001" } else { "000000" };

    for _ in 0..5 {
        let reply = h
            .post(
                "/auth/verify",
                json!({ "challenge_id": challenge, "code": wrong }),
                None,
            )
            .await;
        assert_eq!(reply.status, StatusCode::BAD_REQUEST);
        assert!(reply.cookie.is_none());
    }
    // After five misses even the right code no longer works.
    let reply = h
        .post(
            "/auth/verify",
            json!({ "challenge_id": challenge, "code": code }),
            None,
        )
        .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST);
    let users: i64 = sqlx::query_scalar("select count(*) from users")
        .fetch_one(&h.db)
        .await
        .unwrap();
    assert_eq!(users, 0);
}

#[sqlx::test]
async fn a_code_works_once_and_not_after_it_expires(db: PgPool) {
    let h = harness(db, false, None);
    let verify = |challenge: Value, code: String| {
        h.post(
            "/auth/verify",
            json!({ "challenge_id": challenge, "code": code }),
            None,
        )
    };

    let first = h
        .post("/auth/start", json!({ "email": "ada@example.com" }), None)
        .await;
    let code = h.outbox.0.lock().unwrap()[0].1.clone();
    let challenge = first.body["challenge_id"].clone();
    assert_eq!(
        verify(challenge.clone(), code.clone()).await.status,
        StatusCode::OK
    );
    assert_eq!(
        verify(challenge, code).await.status,
        StatusCode::BAD_REQUEST
    );

    let second = h
        .post("/auth/start", json!({ "email": "ada@example.com" }), None)
        .await;
    let code = h.outbox.0.lock().unwrap()[1].1.clone();
    sqlx::query("update login_challenges set expires_at = now() - interval '1 second' where consumed_at is null")
        .execute(&h.db)
        .await
        .unwrap();
    assert_eq!(
        verify(second.body["challenge_id"].clone(), code)
            .await
            .status,
        StatusCode::BAD_REQUEST
    );

    // A code is no use with someone else's challenge id.
    let third = h
        .post("/auth/start", json!({ "email": "ada@example.com" }), None)
        .await;
    assert_eq!(third.status, StatusCode::OK);
    let code = h.outbox.0.lock().unwrap()[2].1.clone();
    assert_eq!(
        verify(json!(Uuid::new_v4()), code).await.status,
        StatusCode::BAD_REQUEST
    );
}

#[sqlx::test]
async fn codes_are_rate_limited_per_address_and_bad_addresses_rejected(db: PgPool) {
    let h = harness(db, false, None);
    for _ in 0..5 {
        let reply = h
            .post("/auth/start", json!({ "email": "ada@example.com" }), None)
            .await;
        assert_eq!(reply.status, StatusCode::OK);
    }
    let reply = h
        .post("/auth/start", json!({ "email": "ada@example.com" }), None)
        .await;
    assert_eq!(reply.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(reply.body["error"], "too_many_requests");
    // Someone else is unaffected.
    let other = h
        .post("/auth/start", json!({ "email": "grace@example.com" }), None)
        .await;
    assert_eq!(other.status, StatusCode::OK);

    let bad = h
        .post("/auth/start", json!({ "email": "not an email" }), None)
        .await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);
    let malformed = h.post("/auth/start", json!({ "address": "x" }), None).await;
    assert_eq!(malformed.status, StatusCode::BAD_REQUEST);
    assert_eq!(malformed.body["error"], "bad_request");
}

#[sqlx::test]
async fn signing_out_ends_the_session(db: PgPool) {
    let h = harness(db, false, None);
    let session = h.sign_in("ada@example.com").await;
    assert_eq!(h.get("/me", Some(&session)).await.status, StatusCode::OK);

    let out = h.post("/auth/signout", json!({}), Some(&session)).await;
    assert_eq!(out.status, StatusCode::OK);
    assert!(out.cookie.unwrap().contains("Max-Age=0"));
    assert_eq!(
        h.get("/me", Some(&session)).await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        h.get("/me", Some("wain_session=made-up")).await.status,
        StatusCode::UNAUTHORIZED
    );
}

#[sqlx::test]
async fn an_expired_session_is_not_accepted(db: PgPool) {
    let h = harness(db, false, None);
    let session = h.sign_in("ada@example.com").await;
    sqlx::query("update sessions set expires_at = now() - interval '1 second'")
        .execute(&h.db)
        .await
        .unwrap();
    assert_eq!(
        h.get("/me", Some(&session)).await.status,
        StatusCode::UNAUTHORIZED
    );
}

#[sqlx::test]
async fn posts_from_another_site_are_refused(db: PgPool) {
    let h = harness(db, false, None);
    let request = |origin: Option<&str>| {
        let mut request = Request::post("/api/v1/auth/start")
            .header(HOST, "app.test:443")
            .header(CONTENT_TYPE, "application/json");
        if let Some(origin) = origin {
            request = request.header(ORIGIN, origin);
        }
        request
            .body(Body::from(
                json!({ "email": "ada@example.com" }).to_string(),
            ))
            .unwrap()
    };

    let foreign = h.send(request(Some("https://evil.example"))).await;
    assert_eq!(foreign.status, StatusCode::FORBIDDEN);
    assert_eq!(foreign.body["error"], "forbidden");
    assert_eq!(
        h.send(request(Some("null"))).await.status,
        StatusCode::FORBIDDEN
    );

    // The same host on any port or scheme is the app itself, and a client
    // that sends no Origin (a native app) is not a browser being tricked.
    assert_eq!(
        h.send(request(Some("https://APP.test"))).await.status,
        StatusCode::OK
    );
    assert_eq!(
        h.send(request(Some("http://app.test:5173"))).await.status,
        StatusCode::OK
    );
    assert_eq!(h.send(request(None)).await.status, StatusCode::OK);
}

// ---- Credits ----------------------------------------------------------------

async fn user_id(db: &PgPool) -> Uuid {
    sqlx::query_scalar("select id from users")
        .fetch_one(db)
        .await
        .unwrap()
}

#[sqlx::test]
async fn credits_are_spent_refunded_and_never_overdrawn(db: PgPool) {
    let h = harness(db, false, None);
    h.sign_in("ada@example.com").await;
    let user = user_id(&h.db).await;
    let grant = credits::SIGNUP_GRANT;

    let spend = credits::spend(&h.db, user, 4, "test").await.unwrap();
    assert_eq!(credits::balance(&h.db, user).await.unwrap(), grant - 4);

    // Refunding twice gives the credits back once.
    credits::refund(&h.db, spend).await.unwrap();
    credits::refund(&h.db, spend).await.unwrap();
    assert_eq!(credits::balance(&h.db, user).await.unwrap(), grant);

    // More than the balance is refused and takes nothing.
    let too_much = credits::spend(&h.db, user, grant + 1, "test").await;
    assert!(matches!(
        too_much,
        Err(crate::error::ApiError::InsufficientCredits)
    ));
    assert_eq!(credits::balance(&h.db, user).await.unwrap(), grant);

    // A grant cannot be "refunded" into extra credits.
    let grant_entry: Uuid =
        sqlx::query_scalar("select id from credit_entries where reason = 'signup_grant'")
            .fetch_one(&h.db)
            .await
            .unwrap();
    credits::refund(&h.db, grant_entry).await.unwrap();
    assert_eq!(credits::balance(&h.db, user).await.unwrap(), grant);
}

#[sqlx::test]
async fn simultaneous_spends_cannot_share_the_last_credit(db: PgPool) {
    let h = harness(db, false, None);
    h.sign_in("ada@example.com").await;
    let user = user_id(&h.db).await;
    credits::spend(&h.db, user, credits::SIGNUP_GRANT - 1, "test")
        .await
        .unwrap();

    let attempts = (0..8).map(|_| {
        let db = h.db.clone();
        tokio::spawn(async move { credits::spend(&db, user, 1, "test").await.is_ok() })
    });
    let mut succeeded = 0;
    for attempt in attempts.collect::<Vec<_>>() {
        succeeded += usize::from(attempt.await.unwrap());
    }
    assert_eq!(succeeded, 1);
    assert_eq!(credits::balance(&h.db, user).await.unwrap(), 0);
}

// ---- Suggestions ------------------------------------------------------------

#[sqlx::test]
async fn suggestions_cost_a_credit_and_return_buildable_designs(db: PgPool) {
    let fake = FakeSuggester::new(good_answer);
    let h = harness(db, false, Some(fake.clone()));
    let session = h.sign_in("ada@example.com").await;

    let reply = h.suggest(&project(), JPEG, Some(&session)).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(reply.body["observations"], "A plain wall with a low sill.");
    assert_eq!(
        reply.body["credits"],
        credits::SIGNUP_GRANT - credits::SUGGESTION_COST
    );

    // The suggestion taller than the ceiling was dropped; the rest are real
    // designs that plan cleanly on this project.
    let suggestions = reply.body["suggestions"].as_array().unwrap();
    let kinds: Vec<&str> = suggestions
        .iter()
        .map(|s| s["design"]["style"]["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["board_and_batten", "picture_frame"]);
    for suggestion in suggestions {
        let mut trial = project();
        trial.design = serde_json::from_value(suggestion["design"].clone()).unwrap();
        assert!(wain_core::plan(&trial).is_ok());
    }
    // 1370 mm is rounded to the nearest half inch for an imperial project.
    assert_eq!(
        suggestions[0]["design"]["style"]["height"],
        Length::inches(54).nm()
    );

    assert_eq!(fake.calls(), 1);
    assert!(fake.last_brief.lock().unwrap().contains("3658 mm wide"));
}

#[sqlx::test]
async fn a_failed_suggestion_is_refunded(db: PgPool) {
    let cases: [(Answer, &str); 3] = [
        (
            || Err(SuggestError::Failed("timeout".into())),
            "did not respond",
        ),
        (|| Err(SuggestError::Refused), "could not analyse"),
        (
            // Nothing the engine can build.
            || {
                Ok(RawSuggestions {
                    observations: String::new(),
                    suggestions: vec![suggestion(StyleKind::ShakerPanel, 5000.0)],
                })
            },
            "No workable designs",
        ),
    ];
    for (index, (answer, message)) in cases.into_iter().enumerate() {
        let fake = FakeSuggester::new(answer);
        let h = harness(db.clone(), false, Some(fake.clone()));
        let session = h.sign_in(&format!("user{index}@example.com")).await;

        let reply = h.suggest(&project(), JPEG, Some(&session)).await;
        assert_eq!(reply.status, StatusCode::BAD_GATEWAY);
        assert_eq!(reply.body["error"], "upstream");
        assert!(
            reply.body["message"].as_str().unwrap().contains(message),
            "{}",
            reply.body
        );
        assert_eq!(fake.calls(), 1);
        assert_eq!(h.credits(&session).await, i64::from(credits::SIGNUP_GRANT));
    }
}

#[sqlx::test]
async fn suggestions_are_refused_before_any_charge_or_ai_call(db: PgPool) {
    let fake = FakeSuggester::new(good_answer);
    let h = harness(db.clone(), false, Some(fake.clone()));

    // Not signed in.
    assert_eq!(
        h.suggest(&project(), JPEG, None).await.status,
        StatusCode::UNAUTHORIZED
    );

    let session = h.sign_in("ada@example.com").await;
    // Not an image, whatever it is called.
    let reply = h
        .suggest(&project(), b"<svg onload=alert(1)>", Some(&session))
        .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST);
    // A project the engine rejects.
    let mut invalid = project();
    invalid.walls[0].width = Length::ZERO;
    let reply = h.suggest(&invalid, JPEG, Some(&session)).await;
    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(reply.body["issues"][0]["path"], "walls[0].width");
    // A photo over the size limit.
    let mut huge = JPEG.to_vec();
    huge.resize(7 * 1024 * 1024, 0);
    assert_eq!(
        h.suggest(&project(), &huge, Some(&session)).await.status,
        StatusCode::BAD_REQUEST
    );

    // Out of credits.
    let user = user_id(&h.db).await;
    credits::spend(&h.db, user, credits::SIGNUP_GRANT, "test")
        .await
        .unwrap();
    let reply = h.suggest(&project(), JPEG, Some(&session)).await;
    assert_eq!(reply.status, StatusCode::PAYMENT_REQUIRED);
    assert_eq!(reply.body["error"], "insufficient_credits");

    assert_eq!(fake.calls(), 0);
    assert_eq!(h.credits(&session).await, 0);

    // And with no AI provider configured at all.
    let unconfigured = harness(db, false, None);
    let session = unconfigured.sign_in("grace@example.com").await;
    let reply = unconfigured.suggest(&project(), JPEG, Some(&session)).await;
    assert_eq!(reply.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        unconfigured.credits(&session).await,
        i64::from(credits::SIGNUP_GRANT)
    );
}

// ---- Plan -------------------------------------------------------------------

#[sqlx::test]
async fn the_plan_endpoint_needs_no_account(db: PgPool) {
    let h = harness(db, false, None);
    let health = h
        .send(Request::get("/healthz").body(Body::empty()).unwrap())
        .await;
    assert_eq!(health.status, StatusCode::OK);

    let planned = h
        .post("/plan", serde_json::to_value(project()).unwrap(), None)
        .await;
    assert_eq!(planned.status, StatusCode::OK);
    assert_eq!(planned.body["cut_list"][0]["kind"], "batten");
    assert_eq!(planned.body["cut_list"][0]["quantity"], 10);

    let mut invalid = project();
    invalid.walls[0].width = Length::ZERO;
    let rejected = h
        .post("/plan", serde_json::to_value(invalid).unwrap(), None)
        .await;
    assert_eq!(rejected.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(rejected.body["error"], "invalid_project");
    assert_eq!(rejected.body["issues"][0]["path"], "walls[0].width");

    let malformed = h.post("/plan", json!({ "name": 1 }), None).await;
    assert_eq!(malformed.status, StatusCode::BAD_REQUEST);
    assert_eq!(malformed.body["error"], "bad_request");
}
