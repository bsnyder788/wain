//! The credit ledger.
//!
//! Credits meter the features that cost money to run. The ledger is
//! append-only: a balance is the sum of a user's entries. An action that can
//! fail is charged up front and refunded if it does not deliver, so a failed
//! AI call never costs the user anything.

use sqlx::PgPool;
use uuid::Uuid;

use crate::error::ApiError;

/// Credits given to every new account.
pub const SIGNUP_GRANT: i32 = 10;
/// What one set of AI design suggestions costs.
pub const SUGGESTION_COST: i32 = 1;

pub async fn balance(db: &PgPool, user_id: Uuid) -> Result<i32, sqlx::Error> {
    let total: i64 =
        sqlx::query_scalar("select coalesce(sum(delta), 0) from credit_entries where user_id = $1")
            .bind(user_id)
            .fetch_one(db)
            .await?;
    Ok(total.clamp(i32::MIN.into(), i32::MAX.into()) as i32)
}

/// Adds credits to an account, for example the signup grant.
pub async fn grant<'c, E>(
    db: E,
    user_id: Uuid,
    amount: i32,
    reason: &str,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'c, Database = sqlx::Postgres>,
{
    sqlx::query("insert into credit_entries (id, user_id, delta, reason) values ($1, $2, $3, $4)")
        .bind(Uuid::new_v4())
        .bind(user_id)
        .bind(amount)
        .bind(reason)
        .execute(db)
        .await?;
    Ok(())
}

/// Takes `cost` credits from an account, or fails with
/// [`ApiError::InsufficientCredits`] and takes nothing. Returns the id of the
/// ledger entry, which [`refund`] needs.
///
/// The user's row is locked for the duration, so two simultaneous requests
/// cannot both spend the same last credit.
pub async fn spend(db: &PgPool, user_id: Uuid, cost: i32, reason: &str) -> Result<Uuid, ApiError> {
    let mut tx = db.begin().await?;
    sqlx::query("select 1 from users where id = $1 for update")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    let available: i64 =
        sqlx::query_scalar("select coalesce(sum(delta), 0) from credit_entries where user_id = $1")
            .bind(user_id)
            .fetch_one(&mut *tx)
            .await?;
    if available < i64::from(cost) {
        return Err(ApiError::InsufficientCredits);
    }

    let entry = Uuid::new_v4();
    sqlx::query("insert into credit_entries (id, user_id, delta, reason) values ($1, $2, $3, $4)")
        .bind(entry)
        .bind(user_id)
        .bind(-cost)
        .bind(reason)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(entry)
}

/// Gives back what a [`spend`] took. Refunding the same spend twice has no
/// further effect.
pub async fn refund(db: &PgPool, spend_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        "insert into credit_entries (id, user_id, delta, reason, refunds)
         select $1, user_id, -delta, 'refund', id from credit_entries
         where id = $2 and delta < 0
         on conflict do nothing",
    )
    .bind(Uuid::new_v4())
    .bind(spend_id)
    .execute(db)
    .await?;
    Ok(())
}
