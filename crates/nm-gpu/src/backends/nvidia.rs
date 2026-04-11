//! NVIDIA GPU detection via nvidia-smi subprocess.
//!
//! We use subprocess rather than the NVML Rust binding to avoid a hard
//! compile-time dependency on the NVIDIA driver headers. The NVML crate
//! (feature-gated) can be dropped in for production environments where the
//! driver is guaranteed to be present.

use super::GpuBackend;
use crate::types::*;
use anyhow::Result;
use std::process::Command;

pub struct NvidiaBackend;

impl GpuBackend for NvidiaBackend {
    fn name(&self) -> &'static str { "nvidia-cuda" }

    fn is_available(&self) -> bool {
        Command::new("nvidia-smi")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn enumerate(&self) -> Result<Vec<GpuInfo>> {
        // Query all fields we need in one call
        let out = Command::new("nvidia-smi")
            .args([
                "--query-gpu=index,name,memory.total,driver_version,pci.bus_id",
                "--format=csv,noheader,nounits",
            ])
            .output()?;

        anyhow::ensure!(out.status.success(), "nvidia-smi failed");

        let stdout = String::from_utf8_lossy(&out.stdout);
        let mut gpus = Vec::new();

        for line in stdout.lines() {
            let parts: Vec<&str> = line.split(',').map(str::trim).collect();
            if parts.len() < 5 { continue; }

            let index: u32 = parts[0].parse().unwrap_or(0);
            let model = parts[1].to_string();
            let vram_mb: u64 = parts[2].parse().unwrap_or(0);
            let vram_gb = (vram_mb / 1024) as u32;
            let driver = parts[3].to_string();
            let device_id = parts[4].to_string();

            gpus.push(GpuInfo {
                vendor: GpuVendor::Nvidia,
                model: model.clone(),
                vram_gb,
                compute_cores: nvidia_cuda_cores(&model),
                compute_api: ComputeApi::Cuda,
                capability: GpuCapability::from_vram_gb(vram_gb),
                driver_version: driver,
                device_index: index,
                device_id,
                platform: current_platform(),
            });
        }
        Ok(gpus)
    }
}

/// Heuristic CUDA core count for common consumer/pro GPUs.
fn nvidia_cuda_cores(model: &str) -> u32 {
    let m = model.to_lowercase();
    // RTX 50xx series
    if m.contains("5090")   { return 21_760; }
    if m.contains("5080")   { return 10_752; }
    if m.contains("5070 ti") { return 8_960; }
    if m.contains("5070")   { return 6_144; }
    // RTX 40xx series
    if m.contains("4090")   { return 16_384; }
    if m.contains("4080 super") { return 10_240; }
    if m.contains("4080")   { return 9_728;  }
    if m.contains("4070 ti super") { return 8_448; }
    if m.contains("4070 ti") { return 7_680; }
    if m.contains("4070 super") { return 7_168; }
    if m.contains("4070")   { return 5_888;  }
    if m.contains("4060 ti") { return 4_352; }
    if m.contains("4060")   { return 3_072;  }
    // H/A series (data center)
    if m.contains("h100")   { return 16_896; }
    if m.contains("h200")   { return 16_896; }
    if m.contains("a100")   { return 6_912;  }
    if m.contains("a10")    { return 9_216;  }
    // RTX 30xx
    if m.contains("3090")   { return 10_496; }
    if m.contains("3080 ti") { return 10_240; }
    if m.contains("3080")   { return 8_704;  }
    0
}

fn current_platform() -> Platform {
    #[cfg(target_os = "linux")]   { Platform::Linux }
    #[cfg(target_os = "windows")] { Platform::Windows }
    #[cfg(target_os = "macos")]   { Platform::Macos }
}
