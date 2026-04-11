pub mod handlers;

use sqlx::PgPool;

#[derive(Clone)]
pub struct LedgerState {
    pub db: PgPool,
}
