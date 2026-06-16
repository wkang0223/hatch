//! Detect which ML runtimes are installed on this machine.
//! Works on macOS (Apple Silicon) and Linux (AMD ROCm, NVIDIA CUDA).

use std::process::Command;
use tracing::debug;

/// Return every runtime from `allowed` that is actually installed.
pub fn detect_installed_runtimes(allowed: &[String]) -> Vec<String> {
    let mut installed = Vec::new();
    for runtime in allowed {
        if is_runtime_installed(runtime) {
            debug!(runtime, "Runtime detected");
            installed.push(runtime.clone());
        } else {
            debug!(runtime, "Runtime NOT installed");
        }
    }
    installed
}

fn is_runtime_installed(runtime: &str) -> bool {
    match runtime {
        // ── Apple Metal ───────────────────────────────────────────────────────
        "mlx"         => python_has("mlx"),
        "torch-mps"   => python_has("torch") && cfg!(target_os = "macos"),
        "onnx-coreml" => python_has("onnxruntime") && cfg!(target_os = "macos"),
        "llama-cpp"   => python_has("llama_cpp")
            || cmd_exists("llama-cli"),

        // ── NVIDIA CUDA ───────────────────────────────────────────────────────
        "torch-cuda" => {
            // Require CUDA-capable hardware AND PyTorch with CUDA support
            nvidia_smi_ok() && python_has("torch") && python_eval("import torch; assert torch.cuda.is_available()")
        }
        "onnx-cuda"     => nvidia_smi_ok() && python_has("onnxruntime"),
        "tensorrt"      => nvidia_smi_ok() && python_has("tensorrt"),
        "llama-cpp-cuda"=> nvidia_smi_ok() && python_has("llama_cpp"),
        "vllm-cuda"     => nvidia_smi_ok() && python_has("vllm"),

        // ── AMD ROCm ──────────────────────────────────────────────────────────
        "torch-rocm" => {
            // Require ROCm-capable hardware AND PyTorch compiled with ROCm
            rocm_ok() && python_has("torch") && python_eval("import torch; assert torch.cuda.is_available()")
        }
        "onnx-rocm"      => rocm_ok() && python_has("onnxruntime"),
        "llama-cpp-hip"  => rocm_ok() && python_has("llama_cpp"),

        // ── Cross-platform ────────────────────────────────────────────────────
        "shell" => true,

        _ => false,
    }
}

// ── Hardware presence checks ──────────────────────────────────────────────────

fn nvidia_smi_ok() -> bool {
    Command::new("nvidia-smi")
        .arg("--query-gpu=name")
        .arg("--format=csv,noheader")
        .output()
        .map(|o| o.status.success() && !o.stdout.is_empty())
        .unwrap_or(false)
}

fn rocm_ok() -> bool {
    // Primary: rocm-smi
    if Command::new("rocm-smi").arg("--version").output()
        .map(|o| o.status.success()).unwrap_or(false) {
        return true;
    }
    // Fallback: KFD device node (AMD Compute Device, present even for iGPUs)
    std::path::Path::new("/dev/kfd").exists()
}

// ── Python checks ─────────────────────────────────────────────────────────────

fn python_has(package: &str) -> bool {
    python_eval(&format!("import {}", package))
}

fn python_eval(expr: &str) -> bool {
    for interp in &["python3.12", "python3.13", "python3.11", "python3"] {
        if let Ok(out) = Command::new(interp).args(["-c", expr]).output() {
            if out.status.success() { return true; }
        }
    }
    false
}

fn cmd_exists(cmd: &str) -> bool {
    Command::new("which").arg(cmd).output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

// ── Install helpers (called by `hatch provider install`) ──────────────────────

pub fn install_runtime(runtime: &str) -> anyhow::Result<()> {
    match runtime {
        "mlx"   => pip_install(&["mlx", "mlx-lm", "numpy", "Pillow"]),
        "torch-mps"    => pip_install(&["torch", "torchvision", "torchaudio"]),
        "onnx-coreml"  => pip_install(&["onnxruntime", "numpy"]),
        "llama-cpp"    => install_llama_cpp_metal(),
        "torch-cuda"   => pip_install_index(
            &["torch", "torchvision", "torchaudio"],
            "https://download.pytorch.org/whl/cu128",
        ),
        "torch-rocm"   => pip_install_index(
            &["torch", "torchvision", "torchaudio"],
            "https://download.pytorch.org/whl/rocm6.2",
        ),
        "onnx-cuda"    => pip_install(&["onnxruntime-gpu", "numpy"]),
        "onnx-rocm"    => pip_install(&["onnxruntime-rocm", "numpy"]),
        "tensorrt"     => pip_install(&["tensorrt"]),
        "vllm-cuda"    => pip_install(&["vllm"]),
        "llama-cpp-cuda" => install_llama_cpp_cuda(),
        "llama-cpp-hip"  => install_llama_cpp_hip(),
        "shell"          => Ok(()),
        other            => anyhow::bail!("Unknown runtime: {}", other),
    }
}

fn pip_install(packages: &[&str]) -> anyhow::Result<()> {
    let status = Command::new("pip3").arg("install").args(packages).status()?;
    anyhow::ensure!(status.success(), "pip3 install failed");
    Ok(())
}

fn pip_install_index(packages: &[&str], index_url: &str) -> anyhow::Result<()> {
    let status = Command::new("pip3")
        .arg("install").args(packages)
        .args(["--index-url", index_url])
        .status()?;
    anyhow::ensure!(status.success(), "pip3 install failed");
    Ok(())
}

fn install_llama_cpp_metal() -> anyhow::Result<()> {
    let status = Command::new("pip3")
        .args(["install", "llama-cpp-python"])
        .env("CMAKE_ARGS", "-DGGML_METAL=on")
        .env("FORCE_CMAKE", "1")
        .status()?;
    anyhow::ensure!(status.success(), "llama-cpp-python (Metal) install failed");
    Ok(())
}

fn install_llama_cpp_cuda() -> anyhow::Result<()> {
    let status = Command::new("pip3")
        .args(["install", "llama-cpp-python"])
        .env("CMAKE_ARGS", "-DGGML_CUDA=on")
        .env("FORCE_CMAKE", "1")
        .status()?;
    anyhow::ensure!(status.success(), "llama-cpp-python (CUDA) install failed");
    Ok(())
}

fn install_llama_cpp_hip() -> anyhow::Result<()> {
    let status = Command::new("pip3")
        .args(["install", "llama-cpp-python"])
        .env("CMAKE_ARGS", "-DGGML_HIPBLAS=on")
        .env("FORCE_CMAKE", "1")
        .status()?;
    anyhow::ensure!(status.success(), "llama-cpp-python (HIP/ROCm) install failed");
    Ok(())
}
