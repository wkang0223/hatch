//! CPU idle detection on Linux via /proc/stat.
//! GPU idle detection delegates to nvidia-smi / rocm-smi through nm-gpu.

use std::time::{Duration, Instant};

/// Mirrors nm-macos::idle::IdleState so the agent can use either path.
#[derive(Debug, Clone, PartialEq)]
pub enum IdleState {
    Busy,
    CoolingDown,
    Available,
    Leased,
    Paused,
}

pub struct LinuxIdleDetector {
    /// CPU idle threshold (0.0 = all-busy, 100.0 = fully idle)
    idle_threshold_pct: f32,
    /// How many consecutive idle polls before we declare Available
    cool_down_polls:    u32,
    prev_cpu_times:     Option<CpuTimes>,
    idle_run:           u32,
    state:              IdleState,
}

#[derive(Clone)]
struct CpuTimes {
    idle:  u64,
    total: u64,
}

impl LinuxIdleDetector {
    pub fn new(threshold_pct: f32, cool_down_minutes: u32) -> Self {
        Self {
            idle_threshold_pct: threshold_pct,
            // Convert minutes to poll counts (poll interval ≈ 5s → 12 polls/min)
            cool_down_polls:    cool_down_minutes * 12,
            prev_cpu_times:     None,
            idle_run:           0,
            state:              IdleState::Busy,
        }
    }

    pub fn current_state(&self) -> &IdleState { &self.state }

    /// Call every poll tick. Returns Some(new_state) when the state changes.
    pub fn poll(&mut self) -> Option<IdleState> {
        let current = read_cpu_times().unwrap_or(CpuTimes { idle: 0, total: 1 });
        let idle_pct = if let Some(prev) = &self.prev_cpu_times {
            let d_idle  = current.idle.saturating_sub(prev.idle);
            let d_total = current.total.saturating_sub(prev.total);
            if d_total == 0 { 100.0 } else { d_idle as f32 / d_total as f32 * 100.0 }
        } else {
            0.0 // First reading — be conservative
        };
        self.prev_cpu_times = Some(current);

        let is_idle = idle_pct >= self.idle_threshold_pct
            && self.gpu_is_idle();

        if is_idle {
            self.idle_run += 1;
        } else {
            self.idle_run = 0;
        }

        let new_state = if self.idle_run >= self.cool_down_polls {
            IdleState::Available
        } else if is_idle {
            IdleState::CoolingDown
        } else {
            IdleState::Busy
        };

        if new_state != self.state {
            self.state = new_state.clone();
            Some(new_state)
        } else {
            None
        }
    }

    fn gpu_is_idle(&self) -> bool {
        let stats = hatch_gpu::sample_gpu_stats();
        if stats.is_empty() {
            return true; // No GPU, or stats unavailable — assume idle
        }
        // Consider idle if ALL GPUs are < 20% utilisation
        stats.iter().all(|s| s.utilisation_pct < 20.0)
    }
}

fn read_cpu_times() -> Option<CpuTimes> {
    let content = std::fs::read_to_string("/proc/stat").ok()?;
    let line = content.lines().find(|l| l.starts_with("cpu "))?;
    // Format: "cpu  user nice system idle iowait irq softirq steal guest guest_nice"
    let nums: Vec<u64> = line.split_whitespace()
        .skip(1)  // skip "cpu"
        .filter_map(|v| v.parse().ok())
        .collect();

    if nums.len() < 4 { return None; }
    let idle  = nums[3] + nums.get(4).copied().unwrap_or(0); // idle + iowait
    let total = nums.iter().sum();
    Some(CpuTimes { idle, total })
}
