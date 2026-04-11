//! Reverse-auction matching engine — runs every 30 seconds.
//!
//! Algorithm:
//!   1. Pull new jobs from NATS "nm.jobs.new" subject
//!   2. For each job, query available providers meeting hard constraints
//!   3. Score each provider: price(40%) + latency(30%) + trust(20%) + uptime(10%)
//!   4. Assign job to highest-scoring provider
//!   5. Notify both parties via NATS and update DB

use anyhow::Result;
use nm_common::score_bid;
use sqlx::PgPool;
use tokio::time::{interval, Duration};
use tracing::{error, info, warn};

/// Run the matching auction loop. Never returns under normal operation.
pub async fn run_auction_loop(db: PgPool, nats: Option<async_nats::Client>) -> Result<()> {
    let mut tick = interval(Duration::from_secs(30));
    info!("Matching engine started (30s auction windows)");

    loop {
        tick.tick().await;
        if let Err(e) = run_auction_cycle(&db, nats.as_ref()).await {
            error!(error = %e, "Auction cycle error");
        }
    }
}

async fn run_auction_cycle(db: &PgPool, nats: Option<&async_nats::Client>) -> Result<()> {
    // Fetch all queued jobs
    let queued_jobs = sqlx::query!(
        r#"
        SELECT job_id, runtime, min_ram_gb, max_price_per_hour,
               preferred_region, consumer_ssh_pubkey, consumer_wg_pubkey,
               bundle_url, bundle_hash
        FROM jobs
        WHERE state = 'queued'
        ORDER BY created_at ASC
        LIMIT 50
        "#
    )
    .fetch_all(db)
    .await?;

    for job in queued_jobs {
        let job_id = job.job_id.as_str();
        match find_best_provider(db, job_id, job.min_ram_gb.unwrap_or(0) as u32, job.max_price_per_hour.unwrap_or(f64::MAX)).await {
            Ok(Some(provider_id)) => {
                assign_job(db, nats, job_id, &provider_id, job.max_price_per_hour.unwrap_or(0.1)).await?;
            }
            Ok(None) => {
                warn!(job_id, "No matching provider available — will retry next cycle");
            }
            Err(e) => {
                error!(job_id, error = %e, "Error finding provider for job");
            }
        }
    }

    Ok(())
}

async fn find_best_provider(
    db: &PgPool,
    job_id: &str,
    min_ram_gb: u32,
    max_price: f64,
) -> Result<Option<String>> {
    let candidates = sqlx::query!(
        r#"
        SELECT provider_id, floor_price_nmc_per_hour, trust_score, jobs_completed
        FROM providers
        WHERE state = 'available'
          AND max_job_ram_gb >= $1
          AND floor_price_nmc_per_hour <= $2
        ORDER BY floor_price_nmc_per_hour ASC
        LIMIT 20
        "#,
        min_ram_gb as i32,
        max_price,
    )
    .fetch_all(db)
    .await?;

    if candidates.is_empty() {
        return Ok(None);
    }

    // Score candidates and pick the best
    let best = candidates.into_iter().max_by(|a, b| {
        let score_a = simple_score(
            a.floor_price_nmc_per_hour.unwrap_or(0.1),
            a.trust_score.unwrap_or(3.0),
            max_price,
        );
        let score_b = simple_score(
            b.floor_price_nmc_per_hour.unwrap_or(0.1),
            b.trust_score.unwrap_or(3.0),
            max_price,
        );
        score_a.partial_cmp(&score_b).unwrap_or(std::cmp::Ordering::Equal)
    });

    Ok(best.map(|r| r.provider_id))
}

fn simple_score(price: f64, trust: f64, max_price: f64) -> f64 {
    let price_norm = if max_price > 0.0 { 1.0 - (price / max_price).min(1.0) } else { 0.5 };
    let trust_norm = trust / 5.0;
    0.6 * price_norm + 0.4 * trust_norm
}

async fn assign_job(
    db: &PgPool,
    nats: Option<&async_nats::Client>,
    job_id: &str,
    provider_id: &str,
    price_per_hour: f64,
) -> Result<()> {
    // Update job state
    sqlx::query!(
        "UPDATE jobs SET state = 'assigned', provider_id = $1, price_per_hour = $2, started_at = now() WHERE job_id = $3",
        provider_id, price_per_hour, job_id,
    )
    .execute(db)
    .await?;

    // Update provider state
    sqlx::query!(
        "UPDATE providers SET state = 'leased', active_job_id = $1 WHERE provider_id = $2",
        job_id, provider_id,
    )
    .execute(db)
    .await?;

    // Notify provider via NATS (optional — DB polling fallback handles delivery if absent)
    if let Some(nc) = nats {
        let payload = serde_json::json!({
            "type": "job_assigned",
            "job_id": job_id,
            "provider_id": provider_id,
        }).to_string();
        let _ = nc.publish(
            format!("nm.provider.{}", provider_id),
            payload.into_bytes().into(),
        ).await;
    }

    info!(job_id, provider_id, price_per_hour, "Job assigned to provider");
    Ok(())
}

/// Calculate how much a provider earned for a completed job.
pub async fn calculate_credits(db: &PgPool, job_id: &str) -> Result<f64> {
    let row = sqlx::query!(
        "SELECT price_per_hour, actual_runtime_s FROM jobs WHERE job_id = $1",
        job_id,
    )
    .fetch_optional(db)
    .await?;

    if let Some(r) = row {
        let hours = r.actual_runtime_s.unwrap_or(0) as f64 / 3600.0;
        let gross = r.price_per_hour.unwrap_or(0.0) * hours;
        let fee = gross * 0.08; // 8% platform fee; provider receives 92%
        Ok(gross - fee)
    } else {
        Ok(0.0)
    }
}
