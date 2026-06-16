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
    pub floor_price_htc_per_hour: f64,
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
    // ── Apple Metal (macOS) ───────────────────────────────────────────────────
    Mlx,            // Apple MLX framework (Apple Silicon only)
    TorchMps,       // PyTorch with Metal Performance Shaders
    OnnxCoreml,     // ONNX Runtime with CoreML execution provider
    LlamaCpp,       // llama.cpp with Metal backend

    // ── NVIDIA CUDA (Linux / Windows) ────────────────────────────────────────
    TorchCuda,      // PyTorch with CUDA — supports Blackwell sm_120 (CUDA 12.8+)
    OnnxCuda,       // ONNX Runtime with CUDA execution provider
    TensorRt,       // TensorRT — requires NVIDIA GPU + TRT install
    LlamaCppCuda,   // llama.cpp with CUDA backend (cuBLAS)
    VllmCuda,       // vLLM serving engine (requires ≥ 16 GB VRAM)

    // ── AMD ROCm / HIP (Linux) ────────────────────────────────────────────────
    TorchRocm,      // PyTorch with ROCm — supports RDNA 3.5 / Strix Halo (ROCm 6.2+)
    OnnxRocm,       // ONNX Runtime with ROCm execution provider
    LlamaCppHip,    // llama.cpp with HIP (AMD) backend

    // ── Cross-platform ────────────────────────────────────────────────────────
    Shell,          // Plain shell script (always available, CPU fallback)
}

impl Runtime {
    pub fn as_str(&self) -> &'static str {
        match self {
            Runtime::Mlx          => "mlx",
            Runtime::TorchMps     => "torch-mps",
            Runtime::OnnxCoreml   => "onnx-coreml",
            Runtime::LlamaCpp     => "llama-cpp",
            Runtime::TorchCuda    => "torch-cuda",
            Runtime::OnnxCuda     => "onnx-cuda",
            Runtime::TensorRt     => "tensorrt",
            Runtime::LlamaCppCuda => "llama-cpp-cuda",
            Runtime::VllmCuda     => "vllm-cuda",
            Runtime::TorchRocm    => "torch-rocm",
            Runtime::OnnxRocm     => "onnx-rocm",
            Runtime::LlamaCppHip  => "llama-cpp-hip",
            Runtime::Shell        => "shell",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "mlx"            => Some(Runtime::Mlx),
            "torch-mps"      => Some(Runtime::TorchMps),
            "onnx-coreml"    => Some(Runtime::OnnxCoreml),
            "llama-cpp"      => Some(Runtime::LlamaCpp),
            "torch-cuda"     => Some(Runtime::TorchCuda),
            "onnx-cuda"      => Some(Runtime::OnnxCuda),
            "tensorrt"       => Some(Runtime::TensorRt),
            "llama-cpp-cuda" => Some(Runtime::LlamaCppCuda),
            "vllm-cuda"      => Some(Runtime::VllmCuda),
            "torch-rocm"     => Some(Runtime::TorchRocm),
            "onnx-rocm"      => Some(Runtime::OnnxRocm),
            "llama-cpp-hip"  => Some(Runtime::LlamaCppHip),
            "shell"          => Some(Runtime::Shell),
            _                => None,
        }
    }

    /// pip packages to install for this runtime
    pub fn pip_packages(&self) -> Vec<&'static str> {
        match self {
            Runtime::Mlx          => vec!["mlx", "mlx-lm", "numpy", "Pillow"],
            Runtime::TorchMps     => vec!["torch", "torchvision", "torchaudio"],
            Runtime::OnnxCoreml   => vec!["onnxruntime", "numpy"],
            Runtime::LlamaCpp     => vec![],  // needs CMake + Metal flags
            Runtime::TorchCuda    => vec!["torch", "torchvision", "torchaudio"],
            Runtime::OnnxCuda     => vec!["onnxruntime-gpu", "numpy"],
            Runtime::TensorRt     => vec!["tensorrt"],
            Runtime::LlamaCppCuda => vec![],  // needs CMAKE_ARGS=-DGGML_CUDA=on
            Runtime::VllmCuda     => vec!["vllm"],
            Runtime::TorchRocm    => vec!["torch", "torchvision", "torchaudio"],
            Runtime::OnnxRocm     => vec!["onnxruntime-rocm", "numpy"],
            Runtime::LlamaCppHip  => vec![],  // needs CMAKE_ARGS=-DGGML_HIPBLAS=on
            Runtime::Shell        => vec![],
        }
    }

    /// True if this runtime requires a CUDA-capable NVIDIA GPU.
    pub fn requires_cuda(&self) -> bool {
        matches!(self, Runtime::TorchCuda | Runtime::OnnxCuda | Runtime::TensorRt |
                       Runtime::LlamaCppCuda | Runtime::VllmCuda)
    }

    /// True if this runtime requires AMD ROCm.
    pub fn requires_rocm(&self) -> bool {
        matches!(self, Runtime::TorchRocm | Runtime::OnnxRocm | Runtime::LlamaCppHip)
    }

    /// True if this runtime requires Apple Metal.
    pub fn requires_metal(&self) -> bool {
        matches!(self, Runtime::Mlx | Runtime::TorchMps | Runtime::OnnxCoreml | Runtime::LlamaCpp)
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
    pub cost_so_far_htc: f64,
    pub wireguard_endpoint: Option<String>,
    pub ssh_port: Option<u16>,
}

// ──────────────────────────────────────────────
// Credit / wallet
// ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletBalance {
    pub account_id: String,
    pub available_htc: f64,
    pub escrowed_htc: f64,
}

impl WalletBalance {
    pub fn total(&self) -> f64 {
        self.available_htc + self.escrowed_htc
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
