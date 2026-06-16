//! hatch-agent — provider-side daemon for Hatch.
//!
//! Runs as a launchd service on macOS, detects GPU idle state, registers
//! with the coordinator, accepts job assignments, and executes them in an
//! isolated macOS sandbox.

mod checkpoint;
mod config;
mod coordinator_client;
mod idle_monitor;
mod job_runner;
mod network;
mod runtime_check;
mod service;

use anyhow::Result;
use clap::Parser;
use hatch_common::config::AgentConfig;
use std::path::PathBuf;
use tracing::{error, info};

#[derive(Parser, Debug)]
#[command(
    name = "hatch-agent",
    about = "Hatch provider daemon — leases idle Apple Silicon GPU to the network",
    version
)]
struct Cli {
    /// Config file path (default: ~/.config/hatch/agent.toml)
    #[arg(long, short)]
    config: Option<PathBuf>,

    /// Run in foreground (don't daemonize). Useful for debugging.
    #[arg(long)]
    foreground: bool,

    /// Log level override (trace, debug, info, warn, error)
    #[arg(long)]
    log_level: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Load config
    let cfg_path = cli.config.unwrap_or_else(default_config_path);
    let cfg: AgentConfig = config::load_or_create(&cfg_path)?;

    // Init tracing
    let log_level = cli.log_level
        .as_deref()
        .unwrap_or(&cfg.log_level)
        .to_string();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| log_level.parse().unwrap_or_default()),
        )
        .json()
        .init();

    info!(
        version = env!("CARGO_PKG_VERSION"),
        config = %cfg_path.display(),
        "hatch-agent starting"
    );

    // Detect chip info
    let chip = hatch_macos::detect_mac_chip()?;
    info!(
        chip = %chip.chip_model,
        ram_gb = chip.unified_memory_gb,
        gpu_cores = chip.gpu_cores,
        "Apple Silicon detected"
    );

    // Load or generate Ed25519 keypair
    let key_path = cfg_path.parent().unwrap().join("identity.key");
    let keypair = if key_path.exists() {
        hatch_crypto::HatchKeypair::load_from_file(&key_path)?
    } else {
        let kp = hatch_crypto::HatchKeypair::generate();
        kp.save_to_file(&key_path)?;
        info!(pubkey = %kp.public_key_hex(), "Generated new provider identity keypair");
        kp
    };

    info!(provider_id = %keypair.public_key_hex(), "Provider identity loaded");

    // Check installed runtimes
    let available_runtimes = runtime_check::detect_installed_runtimes(&cfg.allowed_runtimes);
    if available_runtimes.is_empty() {
        error!("No ML runtimes installed! Run `nm provider install` first.");
        std::process::exit(1);
    }
    info!(runtimes = ?available_runtimes, "Available runtimes detected");

    // Run main agent loop
    if let Err(e) = run_agent(cfg, chip, keypair, available_runtimes).await {
        error!(error = %e, "Agent error");
        std::process::exit(1);
    }

    Ok(())
}

async fn run_agent(
    cfg: AgentConfig,
    chip: hatch_common::MacChipInfo,
    keypair: hatch_crypto::HatchKeypair,
    runtimes: Vec<String>,
) -> Result<()> {
    use idle_monitor::IdleMonitor;
    use coordinator_client::CoordinatorClient;
    use tracing::warn;

    // Connect with indefinite retry — a transient network blip at startup
    // shouldn't kill the agent permanently until launchd restarts it.
    let mut coordinator = loop {
        match CoordinatorClient::connect(&cfg.coordinator_endpoints).await {
            Ok(c) => { info!("Connected to coordinator"); break c; }
            Err(e) => {
                warn!(error = %e, "Coordinator connection failed — retrying in 5s");
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            }
        }
    };

    // Register with exponential backoff (2s, 4s, 8s … capped at 64s).
    let mut attempt = 0u32;
    loop {
        match coordinator.register(&keypair, &chip, &cfg, &runtimes).await {
            Ok(_) => {
                info!("Provider registered with coordinator");
                break;
            }
            Err(e) => {
                attempt += 1;
                let delay = tokio::time::Duration::from_secs(2u64.saturating_pow(attempt.min(6)));
                warn!(attempt, error = %e, delay_secs = delay.as_secs(), "Registration failed — retrying");
                tokio::time::sleep(delay).await;
            }
        }
    }

    let mut idle_monitor = IdleMonitor::new(
        cfg.idle_threshold_pct,
        cfg.idle_duration_minutes,
        chip.clone(),
        keypair,
        coordinator.clone(),
        cfg.clone(),
        runtimes,
    );

    idle_monitor.run().await
}

fn default_config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("hatch")
        .join("agent.toml")
}
