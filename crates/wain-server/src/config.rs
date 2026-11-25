//! Settings read from the environment at startup.

/// Used to key code hashes when `WAIN_DEV=1` and no secret is set. Never
/// accepted outside dev mode.
const DEV_SECRET: &str = "wain-development-secret-not-for-production";

pub struct Config {
    pub database_url: String,
    pub port: u16,
    /// Keys the hashes of sign-in codes.
    pub secret: Vec<u8>,
    /// Local development mode: sign-in codes are returned to the browser and
    /// logged instead of emailed, and cookies are not marked `Secure`.
    pub dev: bool,
    /// Enables AI suggestions when present.
    pub anthropic_api_key: Option<String>,
    /// Development only: answer suggestion requests with fixed samples
    /// instead of calling a model.
    pub fake_ai: bool,
    /// Selling credits through Stripe, when all three settings are present.
    pub stripe: Option<StripeConfig>,
    /// Development only: a sample credit pack that costs nothing.
    pub fake_billing: bool,
}

pub struct StripeConfig {
    pub secret_key: String,
    pub webhook_secret: String,
    /// `(credits, Stripe price id)` for each pack on sale.
    pub packs: Vec<(i32, String)>,
}

impl Config {
    pub fn from_env() -> Result<Config, String> {
        let var = |name: &str| std::env::var(name).ok().filter(|value| !value.is_empty());

        let dev = var("WAIN_DEV").is_some_and(|value| value == "1" || value == "true");
        let database_url =
            var("DATABASE_URL").ok_or("DATABASE_URL is not set (see scripts/dev-db.sh)")?;
        let port = match var("PORT") {
            Some(port) => port
                .parse()
                .map_err(|_| "PORT is not a valid port number")?,
            None => 8787,
        };
        let secret = match (var("WAIN_SECRET"), dev) {
            (Some(secret), _) if secret.len() >= 32 => secret,
            (Some(_), _) => return Err("WAIN_SECRET must be at least 32 characters".into()),
            (None, true) => DEV_SECRET.to_owned(),
            (None, false) => return Err("WAIN_SECRET is not set".into()),
        };
        if !dev {
            // There is no email provider yet, and codes must never be logged
            // or returned in production.
            return Err("no email provider is configured; set WAIN_DEV=1 to run locally".into());
        }

        let stripe = match (
            var("STRIPE_SECRET_KEY"),
            var("STRIPE_WEBHOOK_SECRET"),
            var("WAIN_CREDIT_PACKS"),
        ) {
            (Some(secret_key), Some(webhook_secret), Some(packs)) => Some(StripeConfig {
                secret_key,
                webhook_secret,
                packs: crate::billing::parse_packs(&packs)?,
            }),
            (None, None, None) => None,
            _ => {
                return Err(
                    "to sell credits, set all of STRIPE_SECRET_KEY, STRIPE_WEBHOOK_SECRET and \
                     WAIN_CREDIT_PACKS"
                        .into(),
                );
            }
        };
        let flag = |name: &str| var(name).is_some_and(|value| value == "1" || value == "true");

        Ok(Config {
            database_url,
            port,
            secret: secret.into_bytes(),
            dev,
            anthropic_api_key: var("ANTHROPIC_API_KEY"),
            fake_ai: dev && flag("WAIN_FAKE_AI"),
            stripe,
            fake_billing: dev && flag("WAIN_FAKE_BILLING"),
        })
    }
}
