use serde::{Deserialize, Serialize};

// ─── Vendor ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GpuVendor {
    Apple,
    Nvidia,
    Amd,
    IntelArc,
    Unknown,
}

impl GpuVendor {
    pub fn as_str(&self) -> &'static str {
        match self {
            GpuVendor::Apple    => "apple",
            GpuVendor::Nvidia   => "nvidia",
            GpuVendor::Amd      => "amd",
            GpuVendor::IntelArc => "intel_arc",
            GpuVendor::Unknown  => "unknown",
        }
    }
}

impl std::fmt::Display for GpuVendor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// ─── Capability tier ──────────────────────────────────────────────────────────

/// Normalised capability tier — used for job matching regardless of vendor.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GpuCapability {
    /// Entry-level inference (8 GB VRAM or Apple M-entry)
    InferenceSmall,
    /// Mid-range inference (16–24 GB VRAM or Apple M-Pro)
    InferenceMid,
    /// High-end inference + fine-tune (40–48 GB or Apple M-Max)
    InferenceLarge,
    /// Training-grade (80+ GB or Apple M-Ultra)
    Training,
}

impl GpuCapability {
    pub fn from_vram_gb(vram_gb: u32) -> Self {
        match vram_gb {
            0..=15  => Self::InferenceSmall,
            16..=31 => Self::InferenceMid,
            32..=79 => Self::InferenceLarge,
            _       => Self::Training,
        }
    }
}

// ─── Core GPU record ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuInfo {
    /// Vendor enum
    pub vendor: GpuVendor,
    /// Driver-reported model name (e.g. "RTX 4090", "RX 7900 XTX", "Arc A770", "M4 Max")
    pub model: String,
    /// VRAM or unified memory in GB
    pub vram_gb: u32,
    /// Shader/compute cores (best-effort; 0 if not queryable)
    pub compute_cores: u32,
    /// Compute API available on this device
    pub compute_api: ComputeApi,
    /// Normalised capability tier
    pub capability: GpuCapability,
    /// Driver version string
    pub driver_version: String,
    /// Device index (for multi-GPU hosts)
    pub device_index: u32,
    /// PCIE bus ID or Metal device id (for disambiguation)
    pub device_id: String,
    /// Platform (macOS / Linux / Windows)
    pub platform: Platform,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComputeApi {
    /// Apple Metal (macOS)
    Metal,
    /// NVIDIA CUDA
    Cuda,
    /// AMD ROCm/HIP (Linux)
    Rocm,
    /// Intel oneAPI / OpenCL (Linux/Windows)
    OneApi,
    /// OpenCL fallback
    OpenCl,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Macos,
    Linux,
    Windows,
}

// ─── Live stats ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuStats {
    pub device_index: u32,
    /// GPU utilisation 0–100
    pub utilisation_pct: f32,
    /// VRAM used in MB
    pub vram_used_mb: u64,
    /// VRAM total in MB
    pub vram_total_mb: u64,
    /// Temperature in Celsius (0 if not available)
    pub temp_celsius: f32,
    /// Power draw in Watts (0 if not available)
    pub power_draw_w: f32,
    /// Clock speed in MHz (0 if not available)
    pub clock_mhz: u32,
}
