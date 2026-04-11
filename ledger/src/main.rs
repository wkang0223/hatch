//! neuralmesh-ledger — off-chain credit accounting (Phase 1).

mod off_chain;

use anyhow::Result;
use axum::{Router, routing::{get, post}};
use clap::Parser;
use sqlx::postgres::PgPoolOptions;
use tower_http::cors::CorsLayer;
use tracing::info;

#[derive(Parser, Debug)]
#[command(name = "neuralmesh-ledger", version)]
struct Cli {
    #[arg(long, env = "NM_DATABASE_URL")]
    database_url: Option<String>,
    #[arg(long, default_value = "0.0.0.0:8082")]
    listen_addr: String,
    #[arg(long, default_value = "info")]
    log_level: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    tracing_subscriber::fmt().with_env_filter(&cli.log_level).json().init();
    info!(version = env!("CARGO_PKG_VERSION"), "neuralmesh-ledger starting");

    let db_url = cli.database_url
        .or_else(|| std::env::var("DATABASE_URL").ok())
        .unwrap_or_else(|| "postgresql://neuralmesh:neuralmesh@localhost/neuralmesh".into());

    let db = PgPoolOptions::new()
        .max_connections(10)
        .connect(&db_url)
        .await?;

    let state = off_chain::LedgerState { db };

    let app = Router::new()
        .route("/health",                        get(health))
        .route("/api/v1/wallet/:id/balance",     get(off_chain::handlers::get_balance))
        .route("/api/v1/wallet/:id/deposit",     post(off_chain::handlers::deposit))
        .route("/api/v1/wallet/:id/withdraw",    post(off_chain::handlers::withdraw))
        .route("/api/v1/wallet/:id/transactions", get(off_chain::handlers::list_transactions))
        .route("/api/v1/escrow/lock",            post(off_chain::handlers::lock_escrow))
        .route("/api/v1/escrow/release",         post(off_chain::handlers::release_escrow))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&cli.listen_addr).await?;
    info!(addr = %cli.listen_addr, "Ledger REST API listening");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> axum::response::Json<serde_json::Value> {
    axum::response::Json(serde_json::json!({ "status": "ok", "service": "neuralmesh-ledger" }))
}
