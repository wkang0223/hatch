//! Provider reputation scoring.
//!
//! Note: `update_trust_score` (per-job success/failure) lives inline in
//! `api/rest.rs` where it has direct DB access. This module provides
//! background maintenance tasks (stale provider eviction).

use sqlx::PgPool;
use anyhow::Result;

/// Evict AVAILABLE providers that haven't sent a heartbeat in 90 seconds.
/// Intentionally skips 'leased' providers — their timeout and job migration
/// is handled by matching::run_heartbeat_watcher (at 120 s) which also
/// refunds consumers and re-queues checkpointed jobs.  Evicting a leased
/// provider here would orphan their active job without any cleanup.
pub async fn evict_stale_providers(db: &PgPool) -> Result<u64> {
    let result = sqlx::query!(
        r#"
        UPDATE providers
        SET state = 'offline'
        WHERE state = 'available'
          AND last_seen < now() - INTERVAL '90 seconds'
        "#
    )
    .execute(db)
    .await?;

    Ok(result.rows_affected())
}
