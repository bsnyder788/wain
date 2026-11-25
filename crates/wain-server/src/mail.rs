//! Sending sign-in codes.

use std::future::Future;
use std::pin::Pin;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Delivers a sign-in code to an email address.
pub trait Mailer: Send + Sync {
    fn send_code<'a>(&'a self, email: &'a str, code: &'a str) -> BoxFuture<'a, Result<(), String>>;
}

/// Development only: writes the code to the server log instead of sending it.
pub struct LogMailer;

impl Mailer for LogMailer {
    fn send_code<'a>(&'a self, email: &'a str, code: &'a str) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            tracing::info!("sign-in code for {email}: {code}");
            Ok(())
        })
    }
}
