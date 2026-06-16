//! nm-gpu — cross-vendor GPU abstraction for Hatch.
//!
//! Supported vendors (Phase 1 = detect + report; Phase 2 = job dispatch):
//!   Apple Silicon  — Metal, macOS only, via sysctl + ioreg (current phase)
//!   NVIDIA         — CUDA, Linux/Windows, via NVML or nvidia-smi
//!   AMD            — ROCm/HIP, Linux, via rocm-smi or sysfs
//!   Intel Arc      — oneAPI/OpenCL, Linux/Windows, via xpu-smi or sysfs
//!
//! Architecture:
//!   GpuInfo          — vendor-agnostic GPU record (common fields)
//!   GpuDetector      — trait each backend implements
//!   detect_gpus()    — platform dispatch, returns Vec<GpuInfo>
//!   RuntimeMap       — maps GpuInfo to supported Hatch runtimes
//!
//! The coordinator uses this to validate provider hardware claims and
//! to map job runtimes to GPU backends.

pub mod types;
pub mod detect;
pub mod runtime_map;
pub mod backends;

pub use types::{GpuInfo, GpuVendor, GpuCapability, GpuStats, ComputeApi};
pub use detect::{detect_gpus, detect_primary_gpu};
pub use runtime_map::{RuntimeMap, SupportedRuntime};

/// Sample live utilisation stats for all GPUs on this host.
pub fn sample_gpu_stats() -> Vec<GpuStats> {
    let mut all = Vec::new();
    #[cfg(target_os = "linux")]
    {
        all.extend(backends::nvidia::sample_stats());
        all.extend(backends::amd::sample_stats());
    }
    all
}
