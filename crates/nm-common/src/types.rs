use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ──────────────────────────────────────────────
// Apple Silicon chip info
// ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MacChipInfo {
    /// e.g. "M4 Pro", "M3 Ultra", "M2"
    pub chip_model: String,
    /// Total unified memory in gigabytes
    pub unified_memory_gb: u32,
    /// Number of GPU cores
    pub gpu_cores: u32,
    /// Number of CPU cores (performance + efficiency)
    pub cpu_cores: u32,
    /// Metal version string e.g. "3.2"
    pub metal_version: String,
    /// IOPlatformSerialNumber — used for hardware attestation
    pub serial_number: String,
    /// IOPlatformUUID
    pub platform_uuid: String,
    /// macOS version e.g. "14.5"
    pub macos_version: String,
}

impl MacChipInfo {
    /// Returns the GPU family tier for capability matching
    pub fn capability_class(&self) -> &'static str {
        match self.unified_memory_gb {
            0..=15  => "metal-gpu-small",   // M1/M2 base, 8-16GB
            16..=31 => "metal-gpu-mid",     // M2/M3/M4 base, 16-24GB
            32..=63 => "metal-gpu-high",    // M4 Pro, M3 Max, 32-48GB
            64..=127 => "metal-gpu-ultra",  // M4 Max, M3 Ultra, 64-128GB
            _        => "metal-gpu-pro",    // M4 Ultra, M3 Ultra 192GB
        }
    }

    /// Returns true if the chip can serve a job requiring `ram_gb`
    pub fn can_serve(&self, ram_gb: u32) -> bool {
        self.unified_memory_gb >= ram_gb + 4 // Reserve 4GB for macOS
    }
}

// ──────────────────────────────────────────────
// Provider
// ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ProviderState {
    Offline,
    Idle,          // Online but not yet reached idle threshold
    Available,     // GPU idle threshold met — ready to accept jobs
    Leased,        // Running a job
    Paused,        // Provider manually paused
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderInfo {
    pub provider_id: String,       // libp2p PeerId (base58)
    pub chip: MacChipInfo,
    pub installed_runtimes: Vec<Runtime>,
    pub max_job_ram_gb: u32,
    pub bandwidth_mbps: u32,
    pub region: String,
    pub floor_price_nmc_per_hour: f64,
    pub wireguard_public_key: String,
    pub state: ProviderState,
    pub trust_score: f32,          // 0.0–5.0
    pub jobs_completed: u32,
    pub success_rate: f32,
}

// ──────────────────────────────────────────────
// Runtime environments
// ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum Runtime {
    Mlx,        // Apple MLX framework
    TorchMps,   // PyTorch with Metal Performance Shaders
    OnnxCoreml, // ONNX Runtime with CoreML execution provider
    LlamaCpp,   // llama.cpp with Metal backend
    Shell,      // Plain shell script (restricted)
}

impl Runtime {
    pub fn as_str(&self) -> &'static str {
        match self {
            Runtime::Mlx        => "mlx",
            Runtime::TorchMps   => "torch-mps",
            Runtime::OnnxCoreml => "onnx-coreml",
            Runtime::LlamaCpp   => "llama-cpp",
            Runtime::Shell      => "shell",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "mlx"          => Some(Runtime::Mlx),
            "torch-mps"    => Some(Runtime::TorchMps),
            "onnx-coreml"  => Some(Runtime::OnnxCoreml),
            "llama-cpp"    => Some(Runtime::LlamaCpp),
            "shell"        => Some(Runtime::Shell),
            _              => None,
        }
    }

    /// Python executable and base packages for this runtime
    pub fn pip_packages(&self) -> Vec<&'static str> {
        match self {
            Runtime::Mlx        => vec!["mlx", "mlx-lm", "numpy", "Pillow"],
            Runtime::TorchMps   => vec!["torch", "torchvision", "torchaudio"],
            Runtime::OnnxCoreml => vec!["onnxruntime", "numpy"],
            Runtime::LlamaCpp   => vec![],  // llama-cpp-python installed separately
            Runtime::Shell      => vec![],
        }
    }
}

// ──────────────────────────────────────────────
// Job specification & status
// ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum JobState {
    Queued,
    Matching,
    Assigned,
    Running,
    Migrating,  // Provider dropped, finding replacement
    Complete,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobSpec {
    pub job_id: Uuid,
    pub consumer_id: String,
    pub runtime: Runtime,
    pub min_ram_gb: u32,
    pub max_duration_secs: u32,
    pub max_price_per_hour: f64,
    pub bundle_hash: String,   // SHA256 of job tar.gz
    pub bundle_url: String,    // Pre-signed S3 URL
    pub consumer_ssh_pubkey: String,
    pub consumer_wg_pubkey: String,
    pub preferred_region: Option<String>,
    pub env_vars: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobStatus {
    pub job_id: Uuid,
    pub state: JobState,
    pub provider_id: Option<String>,
    pub provider_chip: Option<String>,
    pub price_per_hour: f64,
    pub elapsed_secs: u64,
    pub gpu_util_pct: f32,
    pub ram_used_gb: u32,
    pub cost_so_far_nmc: f64,
    pub wireguard_endpoint: Option<String>,
    pub ssh_port: Option<u16>,
}

// ──────────────────────────────────────────────
// Credit / wallet
// ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletBalance {
    pub account_id: String,
    pub available_nmc: f64,
    pub escrowed_nmc: f64,
}

impl WalletBalance {
    pub fn total(&self) -> f64 {
        self.available_nmc + self.escrowed_nmc
    }
}

// ──────────────────────────────────────────────
// Matching bid (provider → coordinator)
// ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderBid {
    pub job_id: Uuid,
    pub provider_id: String,
    pub chip_model: String,
    pub unified_ram_gb: u32,
    pub bid_price_per_hour: f64,
    pub latency_ms: u32,         // Round-trip to consumer (estimated)
    pub attestation_sig: Vec<u8>, // Ed25519 sig over (job_id || provider_id || chip_serial)
}

/// Score a bid for matching. Higher is better.
pub fn score_bid(bid: &ProviderBid, trust_score: f32, uptime_ratio: f32, max_price: f64) -> f64 {
    let price_norm  = 1.0 - (bid.bid_price_per_hour / max_price).min(1.0);
    let latency_norm = 1.0 - (bid.latency_ms as f64 / 500.0).min(1.0);
    let trust_norm  = (trust_score / 5.0) as f64;
    let uptime_norm = uptime_ratio as f64;

    0.40 * price_norm + 0.30 * latency_norm + 0.20 * trust_norm + 0.10 * uptime_norm
}
