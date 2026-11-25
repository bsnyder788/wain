//! Buying credits.
//!
//! Credits are sold in packs through Stripe Checkout. The browser is sent to
//! a Stripe-hosted page, so card details never touch this server. Nothing is
//! granted when the user comes back: the credits are added only when Stripe
//! tells us, in a signed webhook, that the payment went through.

use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::http::header::ORIGIN;
use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::Sha256;
use sqlx::PgPool;
use uuid::Uuid;

use crate::AppState;
use crate::auth::CurrentUser;
use crate::credits;
use crate::error::{ApiError, ApiJson};
use crate::mail::BoxFuture;

const STRIPE_API: &str = "https://api.stripe.com/v1";
/// How old a webhook's timestamp may be, which limits replaying a captured one.
const WEBHOOK_TOLERANCE_SECONDS: i64 = 300;

/// A bundle of credits for sale.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Pack {
    /// The Stripe price id, which also identifies the pack to the client.
    pub id: String,
    pub credits: i32,
    /// The price in the currency's smallest unit (cents for USD).
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub amount: i64,
    /// Lowercase ISO currency code, as Stripe reports it.
    pub currency: String,
}

/// What a payment provider needs to start a checkout.
pub struct CheckoutRequest<'a> {
    pub pack: &'a Pack,
    pub user_id: Uuid,
    pub email: &'a str,
    /// Where Stripe sends the browser afterwards.
    pub success_url: String,
    pub cancel_url: String,
}

/// Starts checkouts. A trait so tests need no payment provider.
pub trait Payments: Send + Sync {
    /// Returns the URL of the hosted checkout page.
    fn create_checkout<'a>(
        &'a self,
        request: &'a CheckoutRequest<'a>,
    ) -> BoxFuture<'a, Result<String, String>>;
}

/// Everything billing needs; absent when the server is not set up to sell.
pub struct Billing {
    pub packs: Vec<Pack>,
    pub payments: Box<dyn Payments>,
    /// Stripe's signing secret for the webhook endpoint.
    pub webhook_secret: Vec<u8>,
}

#[derive(Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct PacksResponse {
    /// Empty when this server does not sell credits.
    pub packs: Vec<Pack>,
}

/// `GET /billing/packs`
pub async fn packs(State(state): State<AppState>) -> Json<PacksResponse> {
    let packs = state
        .billing
        .as_ref()
        .map(|billing| billing.packs.clone())
        .unwrap_or_default();
    Json(PacksResponse { packs })
}

#[derive(Deserialize)]
pub struct CheckoutBody {
    pack: String,
}

#[derive(Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct CheckoutResponse {
    /// The Stripe-hosted page to send the browser to.
    pub url: String,
}

/// `POST /billing/checkout`: starts a purchase for the signed-in user.
pub async fn checkout(
    State(state): State<AppState>,
    user: CurrentUser,
    headers: HeaderMap,
    ApiJson(body): ApiJson<CheckoutBody>,
) -> Result<Json<CheckoutResponse>, ApiError> {
    let billing = state.billing.as_ref().ok_or(ApiError::Unavailable(
        "Buying credits is not set up on this server.",
    ))?;
    let pack = billing
        .packs
        .iter()
        .find(|pack| pack.id == body.pack)
        .ok_or_else(|| ApiError::BadRequest("That credit pack does not exist.".to_owned()))?;

    // Send the browser back to wherever it is using the app from. The
    // same-origin check has already tied this header to our own host.
    let origin = headers
        .get(ORIGIN)
        .and_then(|origin| origin.to_str().ok())
        .filter(|origin| origin.starts_with("http://") || origin.starts_with("https://"))
        .ok_or_else(|| {
            ApiError::BadRequest("Purchases must be started from the web app.".to_owned())
        })?;

    let request = CheckoutRequest {
        pack,
        user_id: user.id,
        email: &user.email,
        success_url: format!("{origin}/account?purchase=success"),
        cancel_url: format!("{origin}/account?purchase=cancelled"),
    };
    let url = billing
        .payments
        .create_checkout(&request)
        .await
        .map_err(|error| {
            tracing::error!(%error, "could not start checkout");
            ApiError::Upstream("The payment page could not be opened. Try again shortly.")
        })?;
    Ok(Json(CheckoutResponse { url }))
}

/// `POST /billing/webhook`: Stripe reporting what happened to a checkout.
///
/// Anyone can post here, so nothing in the body is believed until its
/// signature checks out. A verified event we have no use for is still
/// acknowledged, or Stripe would keep retrying it.
pub async fn webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    let billing = state.billing.as_ref().ok_or(ApiError::Unavailable(
        "Billing is not set up on this server.",
    ))?;
    let signature = headers
        .get("stripe-signature")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or_default();
    if !signature_is_valid(&billing.webhook_secret, signature, &body, now) {
        return Err(ApiError::BadRequest(
            "The webhook signature is not valid.".to_owned(),
        ));
    }

    let event: Value = serde_json::from_slice(&body)
        .map_err(|_| ApiError::BadRequest("The webhook body is not JSON.".to_owned()))?;
    let completed = matches!(
        event["type"].as_str(),
        Some("checkout.session.completed" | "checkout.session.async_payment_succeeded")
    );
    if completed {
        record_purchase(&state.db, &event["data"]["object"]).await?;
    }
    Ok(Json(serde_json::json!({ "received": true })))
}

/// Grants the credits for a paid checkout session, once.
async fn record_purchase(db: &PgPool, session: &Value) -> Result<(), ApiError> {
    // A session can complete before a delayed payment method has paid; the
    // credits wait for the later `async_payment_succeeded` event.
    if session["payment_status"] != "paid" {
        return Ok(());
    }
    let details = (|| {
        let session_id = session["id"].as_str()?;
        let user_id: Uuid = session["client_reference_id"].as_str()?.parse().ok()?;
        let credits: i32 = session["metadata"]["credits"].as_str()?.parse().ok()?;
        (credits > 0).then_some((session_id, user_id, credits))
    })();
    let Some((session_id, user_id, credits)) = details else {
        // Signed by Stripe but not one of ours, or malformed. Retrying will
        // not fix it, so acknowledge it and leave a trace.
        tracing::error!(session = %session["id"], "paid checkout session could not be matched to a purchase");
        return Ok(());
    };

    let mut tx = db.begin().await?;
    let known: Option<i32> = sqlx::query_scalar("select 1 from users where id = $1")
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await?;
    if known.is_none() {
        tracing::error!(%user_id, session_id, "paid checkout session is for an account that no longer exists");
        return Ok(());
    }
    let inserted = sqlx::query(
        "insert into purchases (id, user_id, stripe_session_id, credits, amount_total, currency)
         values ($1, $2, $3, $4, $5, $6)
         on conflict (stripe_session_id) do nothing",
    )
    .bind(Uuid::new_v4())
    .bind(user_id)
    .bind(session_id)
    .bind(credits)
    .bind(session["amount_total"].as_i64())
    .bind(session["currency"].as_str())
    .execute(&mut *tx)
    .await?;
    if inserted.rows_affected() == 1 {
        credits::grant(&mut *tx, user_id, credits, "purchase").await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Checks a `Stripe-Signature` header: `t=<unix time>,v1=<hex hmac>[,v1=...]`,
/// where the HMAC-SHA256 is over `<t>.<body>` keyed with the endpoint's
/// signing secret. Several `v1` values appear while a secret is being rolled.
fn signature_is_valid(secret: &[u8], header: &str, body: &[u8], now: i64) -> bool {
    let mut timestamp = None;
    let mut candidates = Vec::new();
    for (key, value) in header
        .split(',')
        .filter_map(|pair| pair.trim().split_once('='))
    {
        match key {
            "t" => timestamp = value.parse::<i64>().ok(),
            "v1" => candidates.extend(decode_hex(value)),
            _ => {}
        }
    }
    let Some(timestamp) = timestamp else {
        return false;
    };
    if (now - timestamp).abs() > WEBHOOK_TOLERANCE_SECONDS {
        return false;
    }

    candidates.iter().any(|candidate| {
        let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(secret) else {
            return false;
        };
        mac.update(timestamp.to_string().as_bytes());
        mac.update(b".");
        mac.update(body);
        // Compares in constant time.
        mac.verify_slice(candidate).is_ok()
    })
}

fn decode_hex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&text[index..index + 2], 16).ok())
        .collect()
}

/// Reads `WAIN_CREDIT_PACKS`, e.g. `25:price_abc,100:price_def`: the credits
/// in each pack and the Stripe price that sells it.
pub fn parse_packs(setting: &str) -> Result<Vec<(i32, String)>, String> {
    let packs: Vec<(i32, String)> = setting
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            let (credits, price) = entry.split_once(':').ok_or_else(|| {
                format!("WAIN_CREDIT_PACKS entry `{entry}` is not `credits:price_id`")
            })?;
            let credits: i32 = credits
                .trim()
                .parse()
                .ok()
                .filter(|credits| *credits > 0)
                .ok_or_else(|| {
                    format!("WAIN_CREDIT_PACKS entry `{entry}` needs a positive credit count")
                })?;
            Ok((credits, price.trim().to_owned()))
        })
        .collect::<Result<_, String>>()?;
    if packs.is_empty() {
        return Err("WAIN_CREDIT_PACKS lists no packs".to_owned());
    }
    Ok(packs)
}

/// Talks to Stripe's REST API directly; two calls do not justify an SDK.
pub struct StripeClient {
    http: reqwest::Client,
    secret_key: String,
}

impl StripeClient {
    pub fn new(secret_key: String) -> Result<StripeClient, reqwest::Error> {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?;
        Ok(StripeClient { http, secret_key })
    }

    /// Looks up what a price charges, so the app shows exactly what Stripe
    /// will bill and the amount lives in one place.
    pub async fn pack(&self, credits: i32, price_id: &str) -> Result<Pack, String> {
        let response = self
            .http
            .get(format!("{STRIPE_API}/prices/{price_id}"))
            .bearer_auth(&self.secret_key)
            .send()
            .await
            .map_err(|error| error.to_string())?;
        let price = read(response).await?;
        match (price["unit_amount"].as_i64(), price["currency"].as_str()) {
            (Some(amount), Some(currency)) if price["type"] == "one_time" => Ok(Pack {
                id: price_id.to_owned(),
                credits,
                amount,
                currency: currency.to_owned(),
            }),
            _ => Err(format!(
                "Stripe price {price_id} is not a fixed one-time price"
            )),
        }
    }
}

impl Payments for StripeClient {
    fn create_checkout<'a>(
        &'a self,
        request: &'a CheckoutRequest<'a>,
    ) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async move {
            let user_id = request.user_id.to_string();
            let credits = request.pack.credits.to_string();
            let form = [
                ("mode", "payment"),
                ("line_items[0][price]", request.pack.id.as_str()),
                ("line_items[0][quantity]", "1"),
                ("success_url", request.success_url.as_str()),
                ("cancel_url", request.cancel_url.as_str()),
                ("customer_email", request.email),
                // Both come back in the webhook and say who to credit, and
                // with how much.
                ("client_reference_id", user_id.as_str()),
                ("metadata[credits]", credits.as_str()),
            ];
            let response = self
                .http
                .post(format!("{STRIPE_API}/checkout/sessions"))
                .bearer_auth(&self.secret_key)
                .form(&form)
                .send()
                .await
                .map_err(|error| error.to_string())?;
            let session = read(response).await?;
            session["url"]
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| "Stripe returned a checkout session without a URL".to_owned())
        })
    }
}

async fn read(response: reqwest::Response) -> Result<Value, String> {
    let status = response.status();
    let body: Value = response.json().await.map_err(|error| error.to_string())?;
    if status.is_success() {
        Ok(body)
    } else {
        // Stripe's error object describes the problem and holds no secrets.
        Err(format!("{status}: {}", body["error"]["message"]))
    }
}

/// Development only: sells one sample pack and "charges" nothing. Checkout
/// grants the credits on the spot and sends the browser straight back, so
/// the purchase flow can be tried without a Stripe account.
pub struct FakePayments {
    pub db: PgPool,
}

impl FakePayments {
    pub fn packs() -> Vec<Pack> {
        vec![Pack {
            id: "sample_pack".to_owned(),
            credits: 25,
            amount: 500,
            currency: "usd".to_owned(),
        }]
    }
}

impl Payments for FakePayments {
    fn create_checkout<'a>(
        &'a self,
        request: &'a CheckoutRequest<'a>,
    ) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async move {
            credits::grant(
                &self.db,
                request.user_id,
                request.pack.credits,
                "sample_purchase",
            )
            .await
            .map_err(|error| error.to_string())?;
            Ok(request.success_url.clone())
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Signs a body the way Stripe does.
    pub(crate) fn sign(secret: &[u8], body: &[u8], timestamp: i64) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(secret).unwrap();
        mac.update(format!("{timestamp}.").as_bytes());
        mac.update(body);
        let hex: String = mac
            .finalize()
            .into_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        format!("t={timestamp},v1={hex}")
    }

    const SECRET: &[u8] = b"whsec_test";
    const NOW: i64 = 1_800_000_000;

    #[test]
    fn accepts_only_a_fresh_signature_over_this_exact_body() {
        let body = br#"{"type":"checkout.session.completed"}"#;
        let header = sign(SECRET, body, NOW);
        assert!(signature_is_valid(SECRET, &header, body, NOW));
        assert!(signature_is_valid(SECRET, &header, body, NOW + 299));

        // A different body, secret or time all fail.
        assert!(!signature_is_valid(
            SECRET,
            &header,
            br#"{"type":"other"}"#,
            NOW
        ));
        assert!(!signature_is_valid(b"whsec_other", &header, body, NOW));
        assert!(!signature_is_valid(SECRET, &header, body, NOW + 301));
        assert!(!signature_is_valid(SECRET, &header, body, NOW - 301));
        // Changing the timestamp in the header breaks the signature too.
        let moved = header.replace(&format!("t={NOW}"), &format!("t={}", NOW + 10));
        assert!(!signature_is_valid(SECRET, &moved, body, NOW + 10));
    }

    #[test]
    fn copes_with_rolled_secrets_and_junk_headers() {
        let body = b"{}";
        let good = sign(SECRET, body, NOW);
        let v1 = good.split_once(",v1=").unwrap().1;
        // During a secret roll Stripe sends one signature per secret.
        let rolled = format!("t={NOW},v1={},v1={v1},v0=ignored", "00".repeat(32));
        assert!(signature_is_valid(SECRET, &rolled, body, NOW));

        for junk in [
            "",
            "v1=abcd",
            "t=notanumber,v1=abcd",
            &format!("t={NOW}"),
            &format!("t={NOW},v1=zz"),
            &format!("t={NOW},v1=abc"),
        ] {
            assert!(!signature_is_valid(SECRET, junk, body, NOW), "{junk:?}");
        }
    }

    #[test]
    fn reads_the_pack_setting() {
        assert_eq!(
            parse_packs("25:price_a, 100 : price_b ,"),
            Ok(vec![
                (25, "price_a".to_owned()),
                (100, "price_b".to_owned())
            ])
        );
        for bad in ["", "price_a", "0:price_a", "-5:price_a", "many:price_a"] {
            assert!(parse_packs(bad).is_err(), "{bad:?}");
        }
    }
}
