//! nm-linux — Linux platform support for the Hatch agent.
//!
//! Provides:
//!   - `LinuxSandbox`    — cgroups v2 resource limits + optional bwrap filesystem isolation
//!   - `LinuxIdle`       — CPU idle detection via /proc/stat
//!   - `sample_gpu_stats()` — thin wrapper around nm-gpu's Linux stats

pub mod sandbox;
pub mod idle;

pub use sandbox::{LinuxSandbox, LinuxSandboxProfile};
pub use idle::{LinuxIdleDetector, IdleState};

/// Sample GPU utilisation across all detected vendors (AMD + NVIDIA).
pub fn sample_gpu_utilization() -> Option<GpuSampleResult> {
    let stats = hatch_gpu::sample_gpu_stats();
    let first = stats.into_iter().next()?;
    Some(GpuSampleResult {
        utilization_pct: first.utilisation_pct,
        memory_used_mb:  first.vram_used_mb,
    })
}

pub struct GpuSampleResult {
    pub utilization_pct: f32,
    pub memory_used_mb:  u64,
}
