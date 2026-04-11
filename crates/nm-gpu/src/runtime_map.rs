//! Maps GPU vendor + capability to supported NeuralMesh job runtimes.
//!
//! This is the central compatibility matrix that lets the coordinator
//! know which job runtimes a provider can execute, purely based on
//! their detected GPU.

use crate::types::{GpuVendor, GpuCapability, GpuInfo};
use serde::{Deserialize, Serialize};

// ─── Runtime enum (extends nm-common::Runtime) ───────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupportedRuntime {
    // Apple Silicon (existing)
    Mlx,            // MLX (Apple ML framework)
    TorchMps,       // PyTorch with Metal Performance Shaders
    OnnxCoreml,     // ONNX Runtime with CoreML EP
    LlamaCpp,       // llama.cpp with Metal backend
    // NVIDIA CUDA
    TorchCuda,      // PyTorch with CUDA
    OnnxCuda,       // ONNX Runtime with CUDA EP
    TensorRt,       // TensorRT (NVIDIA-only, requires TRT install)
    LlamaCppCuda,   // llama.cpp with CUDA backend
    VllmCuda,       // vLLM (serving, CUDA)
    // AMD ROCm
    TorchRocm,      // PyTorch with ROCm/HIP
    OnnxRocm,       // ONNX Runtime with ROCm EP
    LlamaCppHip,    // llama.cpp with HIP backend
    // Intel Arc (oneAPI / OpenCL)
    TorchXpu,       // PyTorch with XPU (Intel Arc)
    OnnxOpenVino,   // ONNX Runtime with OpenVINO EP
    LlamaCppSycl,   // llama.cpp with SYCL backend
    OpenVinoGenAi,  // OpenVINO GenAI pipeline
    // Cross-vendor
    Shell,          // Shell scripts (CPU fallback, always available)
}

// ─── Compatibility matrix ─────────────────────────────────────────────────────

pub struct RuntimeMap;

impl RuntimeMap {
    /// Return all runtimes supported by a given GPU.
    pub fn for_gpu(gpu: &GpuInfo) -> Vec<SupportedRuntime> {
        match gpu.vendor {
            GpuVendor::Apple    => Self::apple(&gpu.capability),
            GpuVendor::Nvidia   => Self::nvidia(&gpu.capability),
            GpuVendor::Amd      => Self::amd(&gpu.capability),
            GpuVendor::IntelArc => Self::intel_arc(&gpu.capability),
            GpuVendor::Unknown  => vec![SupportedRuntime::Shell],
        }
    }

    fn apple(cap: &GpuCapability) -> Vec<SupportedRuntime> {
        let mut rts = vec![
            SupportedRuntime::LlamaCpp,
            SupportedRuntime::OnnxCoreml,
            SupportedRuntime::Shell,
        ];
        // MPS / MLX require at least 8 GB (InferenceSmall)
        rts.push(SupportedRuntime::TorchMps);
        rts.push(SupportedRuntime::Mlx);
        rts
    }

    fn nvidia(cap: &GpuCapability) -> Vec<SupportedRuntime> {
        let mut rts = vec![
            SupportedRuntime::TorchCuda,
            SupportedRuntime::OnnxCuda,
            SupportedRuntime::LlamaCppCuda,
            SupportedRuntime::Shell,
        ];
        // TensorRT and vLLM need >= InferenceMid (16 GB+)
        if *cap >= GpuCapability::InferenceMid {
            rts.push(SupportedRuntime::TensorRt);
            rts.push(SupportedRuntime::VllmCuda);
        }
        rts
    }

    fn amd(cap: &GpuCapability) -> Vec<SupportedRuntime> {
        vec![
            SupportedRuntime::TorchRocm,
            SupportedRuntime::OnnxRocm,
            SupportedRuntime::LlamaCppHip,
            SupportedRuntime::Shell,
        ]
    }

    fn intel_arc(_cap: &GpuCapability) -> Vec<SupportedRuntime> {
        vec![
            SupportedRuntime::TorchXpu,
            SupportedRuntime::OnnxOpenVino,
            SupportedRuntime::OpenVinoGenAi,
            SupportedRuntime::LlamaCppSycl,
            SupportedRuntime::Shell,
        ]
    }

    /// Check if a specific runtime is supported by a GPU.
    pub fn supports(gpu: &GpuInfo, runtime: &SupportedRuntime) -> bool {
        Self::for_gpu(gpu).contains(runtime)
    }

    /// Human-readable vendor + API label for the dashboard.
    pub fn vendor_label(gpu: &GpuInfo) -> String {
        match gpu.vendor {
            GpuVendor::Apple    => format!("Apple Metal · {}", gpu.model),
            GpuVendor::Nvidia   => format!("NVIDIA CUDA · {}", gpu.model),
            GpuVendor::Amd      => format!("AMD ROCm · {}", gpu.model),
            GpuVendor::IntelArc => format!("Intel Arc oneAPI · {}", gpu.model),
            GpuVendor::Unknown  => format!("Unknown GPU · {}", gpu.model),
        }
    }
}

// ─── Phase roadmap notes ──────────────────────────────────────────────────────
//
// Phase 1 (current): Mac-only, Metal stack
// Phase 2: Linux NVIDIA (CUDA), Linux AMD (ROCm), Linux Intel Arc (oneAPI)
//   - Agent binary compiled per-platform with feature flags
//   - Coordinator uses RuntimeMap to filter providers by job runtime
//   - Provider registers supported runtimes on heartbeat
// Phase 3: Windows support (CUDA + DirectML), multi-GPU scheduling
//   - Windows agent uses CUDA (NVIDIA) or DirectML (AMD/Intel fallback)
//   - DirectML runtime added as SupportedRuntime::DirectMl
