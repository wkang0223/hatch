//! Main event loop for the agent: idle detection + job acceptance.

use crate::coordinator_client::CoordinatorClient;
use crate::job_runner::JobRunner;
use anyhow::Result;
use nm_common::{config::AgentConfig, MacChipInfo};
use nm_crypto::NmKeypair;
use nm_macos::idle::{IdleDetector, IdleState};
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tracing::{error, info, warn};

pub struct IdleMonitor {
    threshold_pct: f32,
    cool_down_minutes: u32,
    chip: MacChipInfo,
    keypair: NmKeypair,
    coordinator: CoordinatorClient,
    cfg: AgentConfig,
    runtimes: Vec<String>,
    start_time: Instant,
}

impl IdleMonitor {
    pub fn new(
        threshold_pct: f32,
        cool_down_minutes: u32,
        chip: MacChipInfo,
        keypair: NmKeypair,
        coordinator: CoordinatorClient,
        cfg: AgentConfig,
        runtimes: Vec<String>,
    ) -> Self {
        Self {
            threshold_pct,
            cool_down_minutes,
            chip,
            keypair,
            coordinator,
            cfg,
            runtimes,
            start_time: Instant::now(),
        }
    }

    pub async fn run(&mut self) -> Result<()> {
        let mut detector = IdleDetector::new(self.threshold_pct, self.cool_down_minutes);
        let mut current_state = IdleState::Busy;
        let heartbeat_interval = Duration::from_secs(30);
        let poll_interval      = Duration::from_secs(30);
        let mut last_heartbeat = Instant::now();

        info!("Idle monitor started — polling every {}s", poll_interval.as_secs());

        loop {
            // Poll idle state
            if let Some(new_state) = detector.poll() {
                info!(state = ?new_state, "Idle state changed");
                current_state = new_state.clone();

                match &current_state {
                    IdleState::Available => {
                        // Announce availability to coordinator
                        if let Err(e) = self.announce_available().await {
                            warn!(error = %e, "Failed to announce availability");
                        }
                    }
                    IdleState::Busy => {
                        // Let coordinator know we're busy again
                        let _ = self.send_heartbeat("busy", &current_state).await;
                    }
                    _ => {}
                }
            }

            // Send heartbeat every 30 seconds
            if last_heartbeat.elapsed() >= heartbeat_interval {
                let state_str = state_to_str(&current_state);
                let gpu_stats = nm_macos::gpu_detect::sample_gpu_utilization();
                let gpu_util  = gpu_stats.as_ref().map(|s| s.utilization_pct).unwrap_or(0.0);
                let ram_used  = gpu_stats.as_ref()
                    .map(|s| (s.memory_used_mb / 1024) as u32)
                    .unwrap_or(0);

                if let Err(e) = self.coordinator.heartbeat(
                    state_str,
                    gpu_util,
                    ram_used,
                    None,
                    self.start_time.elapsed().as_secs(),
                ).await {
                    warn!(error = %e, "Heartbeat failed");
                }
                last_heartbeat = Instant::now();
            }

            sleep(poll_interval).await;
        }
    }

    async fn announce_available(&self) -> Result<()> {
        self.coordinator.heartbeat(
            "available",
            0.0,
            0,
            None,
            self.start_time.elapsed().as_secs(),
        ).await?;
        info!("Announced AVAILABLE to coordinator");
        Ok(())
    }

    async fn send_heartbeat(&self, state: &str, _idle_state: &IdleState) -> Result<()> {
        self.coordinator.heartbeat(state, 0.0, 0, None, self.start_time.elapsed().as_secs()).await?;
        Ok(())
    }
}

fn state_to_str(state: &IdleState) -> &'static str {
    match state {
        IdleState::Busy        => "busy",
        IdleState::CoolingDown => "cooling_down",
        IdleState::Available   => "available",
        IdleState::Leased      => "leased",
        IdleState::Paused      => "paused",
    }
}
