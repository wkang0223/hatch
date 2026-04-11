//! REST API for the dashboard and external integrations.

use super::AppState;
use anyhow::Result;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tower_http::cors::CorsLayer;
use tracing::info;
use uuid::Uuid;

pub async fn serve(state: AppState, addr: String) -> Result<()> {
    let app = Router::new()
        .route("/health",                    get(health))
        .route("/api/v1/providers",          get(list_providers))
        .route("/api/v1/providers/:id",      get(get_provider))
        .route("/api/v1/jobs",               get(list_jobs_rest))
        .route("/api/v1/stats",              get(network_stats))
        // ── Device-locked account endpoints ──────────────────────────────
        .route("/api/v1/account/register",      post(register_account))
        .route("/api/v1/account/:id",           get(get_account))
        .route("/api/v1/account/:id/verify",    post(verify_device))
        .route("/api/v1/account/:id/reregister", post(reregister_device))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!(addr = %addr, "REST API server listening");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok", "service": "neuralmesh-coordinator" }))
}

#[derive(Deserialize)]
struct ProviderQuery {
    runtime: Option<String>,
    min_ram: Option<i32>,
    region: Option<String>,
    limit: Option<i64>,
}

async fn list_providers(
    State(state): State<AppState>,
    Query(q): Query<ProviderQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let rows = sqlx::query!(
        r#"
        SELECT provider_id, chip_model, unified_memory_gb, gpu_cores,
               floor_price_nmc_per_hour, region, trust_score,
               jobs_completed, state, last_seen, installed_runtimes, max_job_ram_gb
        FROM providers
        WHERE state = 'available'
          AND ($1::int IS NULL OR max_job_ram_gb >= $1)
          AND ($2::text IS NULL OR region = $2)
        ORDER BY floor_price_nmc_per_hour ASC
        LIMIT $3
        "#,
        q.min_ram,
        q.region,
        q.limit.unwrap_or(50),
    )
    .fetch_all(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let providers: Vec<_> = rows.into_iter().map(|r| serde_json::json!({
        "id":                       r.provider_id,
        "chip_model":               r.chip_model,
        "unified_memory_gb":        r.unified_memory_gb,
        "gpu_cores":                r.gpu_cores,
        "floor_price_nmc_per_hour": r.floor_price_nmc_per_hour,
        "region":                   r.region,
        "trust_score":              r.trust_score,
        "jobs_completed":           r.jobs_completed,
        "state":                    r.state,
        "installed_runtimes":       r.installed_runtimes.unwrap_or_default(),
        "max_job_ram_gb":           r.max_job_ram_gb,
        "last_seen":                r.last_seen.map(|t| t.to_string()),
    })).collect();

    Ok(Json(serde_json::json!({ "providers": providers, "total": providers.len() })))
}

async fn get_provider(
    State(state): State<AppState>,
    Path(provider_id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let row = sqlx::query!(
        "SELECT * FROM providers WHERE provider_id = $1",
        provider_id,
    )
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::NOT_FOUND)?;

    Ok(Json(serde_json::json!({
        "provider_id": row.provider_id,
        "chip_model": row.chip_model,
        "unified_memory_gb": row.unified_memory_gb,
        "state": row.state,
        "floor_price_nmc_per_hour": row.floor_price_nmc_per_hour,
        "jobs_completed": row.jobs_completed,
        "trust_score": row.trust_score,
    })))
}

async fn list_jobs_rest(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let rows = sqlx::query!(
        "SELECT job_id, consumer_id, state, runtime, created_at FROM jobs ORDER BY created_at DESC LIMIT 50"
    )
    .fetch_all(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let jobs: Vec<_> = rows.into_iter().map(|r| serde_json::json!({
        "job_id":      r.job_id,
        "consumer_id": r.consumer_id,
        "state":       r.state,
        "runtime":     r.runtime,
        "created_at":  r.created_at.map(|t| t.to_string()),
    })).collect();

    Ok(Json(serde_json::json!({ "jobs": jobs })))
}

// ─── Device-locked account registration ──────────────────────────────────────

/// Body sent by the browser/CLI when creating a new account.
#[derive(Deserialize)]
struct RegisterAccountBody {
    /// ECDSA P-256 public key in SPKI format, hex-encoded (Web Crypto exportKey "spki")
    ecdsa_pubkey_hex: String,
    /// SHA-256(device fingerprint signals), hex-encoded
    device_fingerprint_hash: String,
    /// Optional human label, e.g. "Alice's MacBook Pro"
    device_label: Option<String>,
    /// "macos" | "linux" | "windows" | "browser"
    platform: Option<String>,
    /// BLAKE3(IOPlatformUUID + serial) — only sent by the CLI agent, empty for browser
    hardware_serial_hash: Option<String>,
}

/// Derive account_id = hex(SHA-256(pubkey_hex || fingerprint_hash))[..24]
fn derive_account_id(pubkey_hex: &str, fingerprint_hash: &str) -> String {
    let mut h = Sha256::new();
    h.update(pubkey_hex.as_bytes());
    h.update(b"||");
    h.update(fingerprint_hash.as_bytes());
    let result = h.finalize();
    hex::encode(&result[..12]) // 24 hex chars = 96 bits — collision-resistant for our scale
}

async fn register_account(
    State(state): State<AppState>,
    Json(body): Json<RegisterAccountBody>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Validate pubkey format (must be non-empty hex)
    if body.ecdsa_pubkey_hex.is_empty() || body.device_fingerprint_hash.len() != 64 {
        return Err(StatusCode::BAD_REQUEST);
    }

    let account_id = derive_account_id(&body.ecdsa_pubkey_hex, &body.device_fingerprint_hash);
    let platform = body.platform.unwrap_or_else(|| "browser".to_string());

    // Upsert — idempotent: same pubkey+fingerprint always yields the same account_id
    let result = sqlx::query!(
        r#"
        INSERT INTO accounts (
            account_id, ecdsa_pubkey_hex, device_fingerprint_hash,
            device_label, platform, hardware_serial_hash
        ) VALUES ($1, $2, $3, $4, $5, $6)
        ON CONFLICT (account_id) DO UPDATE
            SET last_seen    = now(),
                device_label = COALESCE($4, accounts.device_label)
        RETURNING account_id, created_at, last_seen
        "#,
        account_id,
        body.ecdsa_pubkey_hex,
        body.device_fingerprint_hash,
        body.device_label,
        platform,
        body.hardware_serial_hash,
    )
    .fetch_one(&state.db)
    .await;

    match result {
        Ok(row) => {
            // Also ensure a credit_accounts entry exists for this account
            let _ = sqlx::query!(
                "INSERT INTO credit_accounts (account_id) VALUES ($1) ON CONFLICT DO NOTHING",
                account_id,
            )
            .execute(&state.db)
            .await;

            info!(account_id = %account_id, platform = %platform, "Account registered/refreshed");
            Ok(Json(serde_json::json!({
                "ok": true,
                "account_id": row.account_id,
                "created_at": row.created_at.map(|t| t.to_string()),
                "last_seen":  row.last_seen.map(|t| t.to_string()),
            })))
        }
        Err(e) => {
            // Unique constraint on ecdsa_pubkey_hex — different fingerprint on same key
            if e.to_string().contains("unique") {
                return Ok(Json(serde_json::json!({
                    "ok": false,
                    "error": "device_mismatch",
                    "message": "This key is already registered to a different device fingerprint."
                })));
            }
            tracing::error!(error = %e, "Account registration failed");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn get_account(
    State(state): State<AppState>,
    Path(account_id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let acc = sqlx::query!(
        "SELECT account_id, device_label, platform, role, active, created_at, last_seen FROM accounts WHERE account_id = $1",
        account_id,
    )
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::NOT_FOUND)?;

    let bal = sqlx::query!(
        "SELECT available_nmc, escrowed_nmc, total_earned_nmc, total_spent_nmc FROM credit_accounts WHERE account_id = $1",
        account_id,
    )
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({
        "account_id":    acc.account_id,
        "device_label":  acc.device_label,
        "platform":      acc.platform,
        "role":          acc.role,
        "active":        acc.active,
        "created_at":    acc.created_at.map(|t| t.to_string()),
        "last_seen":     acc.last_seen.map(|t| t.to_string()),
        "balance": bal.map(|b| serde_json::json!({
            "available_nmc":    b.available_nmc,
            "escrowed_nmc":     b.escrowed_nmc,
            "total_earned_nmc": b.total_earned_nmc,
            "total_spent_nmc":  b.total_spent_nmc,
        })),
    })))
}

/// Verify that the requesting device matches the stored fingerprint for an account.
#[derive(Deserialize)]
struct VerifyDeviceBody {
    device_fingerprint_hash: String,
    ecdsa_pubkey_hex: String,
}

async fn verify_device(
    State(state): State<AppState>,
    Path(account_id): Path<String>,
    Json(body): Json<VerifyDeviceBody>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let acc = sqlx::query!(
        "SELECT ecdsa_pubkey_hex, device_fingerprint_hash FROM accounts WHERE account_id = $1 AND active = TRUE",
        account_id,
    )
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::NOT_FOUND)?;

    let pubkey_match = acc.ecdsa_pubkey_hex == body.ecdsa_pubkey_hex;
    let fingerprint_match = acc.device_fingerprint_hash == body.device_fingerprint_hash;

    if pubkey_match && fingerprint_match {
        // Update last_seen
        let _ = sqlx::query!(
            "UPDATE accounts SET last_seen = now() WHERE account_id = $1",
            account_id
        ).execute(&state.db).await;

        Ok(Json(serde_json::json!({ "ok": true, "verified": true })))
    } else {
        Ok(Json(serde_json::json!({
            "ok": true,
            "verified": false,
            "reason": if !pubkey_match { "key_mismatch" } else { "fingerprint_mismatch" }
        })))
    }
}

// ─────────────────────────────────────────────────────────────────────────────

async fn network_stats(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let stats = sqlx::query!(
        r#"
        SELECT
            COUNT(*) FILTER (WHERE state = 'available') AS available_providers,
            COUNT(*) FILTER (WHERE state = 'leased')    AS active_providers,
            SUM(unified_memory_gb) FILTER (WHERE state = 'available') AS total_available_ram_gb
        FROM providers
        "#
    )
    .fetch_one(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let job_stats = sqlx::query!(
        r#"
        SELECT
            COUNT(*) FILTER (WHERE state = 'running')  AS running_jobs,
            COUNT(*) FILTER (WHERE state = 'complete') AS completed_jobs
        FROM jobs
        "#
    )
    .fetch_one(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({
        "available_providers":  stats.available_providers,
        "active_providers":     stats.active_providers,
        "total_available_ram_gb": stats.total_available_ram_gb,
        "running_jobs":         job_stats.running_jobs,
        "completed_jobs":       job_stats.completed_jobs,
    })))
}

// ─── Re-register device (update fingerprint for existing account) ─────────────

/// Update the device fingerprint for an existing account.
/// Used when the browser/OS is updated and stable signals shift.
#[derive(Deserialize)]
struct ReregisterBody {
    ecdsa_pubkey_hex: String,
    new_device_fingerprint_hash: String,
    old_device_fingerprint_hash: String,
    device_label: Option<String>,
}

async fn reregister_device(
    State(state): State<AppState>,
    Path(account_id): Path<String>,
    Json(body): Json<ReregisterBody>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if body.new_device_fingerprint_hash.len() != 64 || body.old_device_fingerprint_hash.len() != 64 {
        return Err(StatusCode::BAD_REQUEST);
    }

    // Verify the pubkey matches what's stored — prevents hijacking
    let acc = sqlx::query!(
        "SELECT ecdsa_pubkey_hex FROM accounts WHERE account_id = $1 AND active = TRUE",
        account_id,
    )
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::NOT_FOUND)?;

    if acc.ecdsa_pubkey_hex != body.ecdsa_pubkey_hex {
        return Ok(Json(serde_json::json!({
            "ok": false,
            "error": "key_mismatch",
            "message": "Public key does not match the registered account."
        })));
    }

    // Update the fingerprint and last_seen
    let row = sqlx::query!(
        r#"
        UPDATE accounts
        SET device_fingerprint_hash = $1,
            last_seen               = now(),
            device_label            = COALESCE($2, device_label)
        WHERE account_id = $3
        RETURNING account_id, last_seen
        "#,
        body.new_device_fingerprint_hash,
        body.device_label,
        account_id,
    )
    .fetch_one(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    info!(account_id = %account_id, "Device fingerprint updated");
    Ok(Json(serde_json::json!({
        "ok": true,
        "account_id": row.account_id,
        "last_seen":  row.last_seen.map(|t| t.to_string()),
    })))
}
