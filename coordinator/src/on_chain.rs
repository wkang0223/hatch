//! Coordinator-side on-chain settlement bridge.
//!
//! Instead of calling Arbitrum directly (which requires alloy in the
//! coordinator binary), the coordinator POSTs to the ledger service's
//! `/api/v1/on_chain/release_escrow` endpoint.  The ledger service owns
//! the oracle private key and the alloy dependency.
//!
//! If `NM_LEDGER_URL` is absent the calls are silently skipped —
//! Phase 1/2 deployments run without a ledger service.

use anyhow::Result;
use tracing::{info, warn};

/// HTTP client that forwards settlement requests to `neuralmesh-ledger`.
///
/// Thread-safe — `Arc<SettlementOracle>` is cheaply cloned.
#[derive(Clone, Debug)]
pub struct SettlementOracle {
    ledger_url: String,
    secret:     String,
    client:     reqwest::Client,
    pub enabled: bool,
}

impl SettlementOracle {
    /// Initialise from environment variables:
    /// - `NM_LEDGER_URL`   — base URL of the ledger service (e.g. `http://ledger:8082`)
    /// - `INTERNAL_SECRET` — shared secret for ledger→coordinator auth
    pub fn from_env() -> Self {
        let ledger_url = std::env::var("NM_LEDGER_URL").unwrap_or_default();
        if ledger_url.is_empty() {
            info!("NM_LEDGER_URL not set — on-chain settlement forwarding disabled");
            return Self::disabled();
        }
        let secret = std::env::var("INTERNAL_SECRET").unwrap_or_default();
        if secret.is_empty() {
            warn!("INTERNAL_SECRET not set — on-chain settlement disabled (auth required)");
            return Self::disabled();
        }

        info!(ledger_url = %ledger_url, "On-chain settlement forwarding enabled → ledger service");
        Self {
            ledger_url,
            secret,
            client: reqwest::Client::new(),
            enabled: true,
        }
    }

    pub fn disabled() -> Self {
        Self {
            ledger_url: String::new(),
            secret:     String::new(),
            client:     reqwest::Client::new(),
            enabled:    false,
        }
    }

    /// Forward `releaseEscrow(job_id, actual_cost)` to the ledger service.
    /// Returns the Arbitrum tx hash string, or `None` if disabled / ledger down.
    pub async fn release_escrow(
        &self,
        job_id:          &str,
        actual_cost_nmc: f64,
    ) -> Result<Option<String>> {
        if !self.enabled {
            return Ok(None);
        }

        let url = format!("{}/api/v1/on_chain/release_escrow", self.ledger_url);
        let body = serde_json::json!({
            "job_id":          job_id,
            "actual_cost_nmc": actual_cost_nmc,
            "secret":          self.secret,
        });

        let res = self.client
            .post(&url)
            .json(&body)
            .send()
            .await?;

        if !res.status().is_success() {
            anyhow::bail!(
                "Ledger releaseEscrow returned {}: {}",
                res.status(),
                res.text().await.unwrap_or_default()
            );
        }

        let json: serde_json::Value = res.json().await?;
        let tx_hash = json["tx_hash"].as_str().map(String::from);

        if let Some(ref hash) = tx_hash {
            info!(job_id, tx_hash = %hash, "Escrow released on-chain via ledger");
        }

        Ok(tx_hash)
    }

    /// Forward `cancelEscrow(job_id)` to the ledger service.
    pub async fn cancel_escrow(&self, job_id: &str) -> Result<Option<String>> {
        if !self.enabled {
            return Ok(None);
        }

        let url = format!("{}/api/v1/on_chain/cancel_escrow", self.ledger_url);
        let body = serde_json::json!({
            "job_id": job_id,
            "secret": self.secret,
        });

        let res = self.client
            .post(&url)
            .json(&body)
            .send()
            .await?;

        if !res.status().is_success() {
            anyhow::bail!(
                "Ledger cancelEscrow returned {}: {}",
                res.status(),
                res.text().await.unwrap_or_default()
            );
        }

        let json: serde_json::Value = res.json().await?;
        let tx_hash = json["tx_hash"].as_str().map(String::from);

        if let Some(ref hash) = tx_hash {
            info!(job_id, tx_hash = %hash, "Escrow cancelled on-chain via ledger");
        }

        Ok(tx_hash)
    }
}
