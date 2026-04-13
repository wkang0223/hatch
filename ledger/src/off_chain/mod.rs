pub mod handlers;

use sqlx::PgPool;
use std::sync::Arc;

#[derive(Clone)]
pub struct LedgerState {
    pub db:     PgPool,
    /// Phase 3 on-chain oracle — disabled (no-op) when `NM_ESCROW_ADDRESS` is absent.
    pub oracle: Arc<crate::on_chain::ArbitrumOracle>,
}
