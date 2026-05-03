//! Provider reputation scoring.
//!
//! Note: `update_trust_score` (per-job success/failure) lives inline in
//! `api/rest.rs` where it has direct DB access. This module provides
//! background maintenance tasks (stale provider eviction).

use sqlx::PgPool;
use anyhow::Result;

/// Evict providers that haven't sent a heartbeat in 90 seconds.
pub async fn evict_stale_providers(db: &PgPool) -> Result<u64> {
    let result = sqlx::query!(
        r#"
        UPDATE providers
        SET state = 'offline', active_job_id = NULL
        WHERE state IN ('available', 'leased')
          AND last_seen < now() - INTERVAL '90 seconds'
        "#
    )
    .execute(db)
    .await?;

    Ok(result.rows_affected())
}
