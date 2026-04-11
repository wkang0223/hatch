//! Provider reputation scoring.

use sqlx::PgPool;
use anyhow::Result;

/// Update provider trust score after a job outcome.
pub async fn update_score(db: &PgPool, provider_id: &str, success: bool) -> Result<()> {
    let delta: f64 = if success { 0.01 } else { -0.10 };

    sqlx::query!(
        r#"
        UPDATE providers SET
            trust_score  = GREATEST(0.0, LEAST(5.0, trust_score + $1)),
            jobs_completed = jobs_completed + 1,
            success_rate = (jobs_completed * success_rate + $2::float8) / (jobs_completed + 1),
            state = 'available'
        WHERE provider_id = $3
        "#,
        delta,
        if success { 1.0f64 } else { 0.0f64 },
        provider_id,
    )
    .execute(db)
    .await?;

    Ok(())
}

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
