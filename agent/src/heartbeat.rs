//! Background heartbeat task — keeps the coordinator updated every 30s.

use crate::coordinator_client::CoordinatorClient;
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tracing::warn;

pub async fn run_heartbeat(
    coordinator: CoordinatorClient,
    state_rx: tokio::sync::watch::Receiver<String>,
    start_time: Instant,
) {
    let interval = Duration::from_secs(30);
    loop {
        sleep(interval).await;
        let state = state_rx.borrow().clone();
        let uptime = start_time.elapsed().as_secs();
        let gpu_stats = nm_macos::gpu_detect::sample_gpu_utilization();
        let gpu_util  = gpu_stats.as_ref().map(|s| s.utilization_pct).unwrap_or(0.0);
        let ram_used  = gpu_stats.as_ref()
            .map(|s| (s.memory_used_mb / 1024) as u32)
            .unwrap_or(0);

        if let Err(e) = coordinator.heartbeat(&state, gpu_util, ram_used, None, uptime).await {
            warn!(error = %e, "Heartbeat failed");
        }
    }
}
