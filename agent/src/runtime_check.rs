//! Detect which ML runtimes are installed on this Mac.

use std::process::Command;
use tracing::debug;

/// Check each allowed runtime and return a list of installed ones.
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
        "mlx" => python_package_available("mlx"),
        "torch-mps" => python_package_available("torch"),
        "onnx-coreml" => python_package_available("onnxruntime"),
        "llama-cpp" => {
            // Check for llama-cpp-python or the llama.cpp binary
            python_package_available("llama_cpp")
                || Command::new("which").arg("llama-cli").status()
                    .map(|s| s.success()).unwrap_or(false)
        }
        "shell" => true, // Always available
        _ => false,
    }
}

fn python_package_available(package: &str) -> bool {
    // Try several interpreters — homebrew may install packages under a versioned python
    let candidates = ["python3.12", "python3.13", "python3.11", "python3"];
    for interp in &candidates {
        if let Ok(out) = Command::new(interp)
            .args(["-c", &format!("import {}", package)])
            .output()
        {
            if out.status.success() {
                return true;
            }
        }
    }
    false
}

/// Install runtimes using pip. Called by `nm provider install`.
pub fn install_runtime(runtime: &str) -> anyhow::Result<()> {
    use nm_common::Runtime;
    let rt = Runtime::from_str(runtime)
        .ok_or_else(|| anyhow::anyhow!("Unknown runtime: {}", runtime))?;

    let packages = rt.pip_packages();
    if packages.is_empty() {
        if runtime == "llama-cpp" {
            // llama-cpp-python needs special install with Metal support
            install_llama_cpp()?;
        }
        return Ok(());
    }

    let mut cmd = Command::new("pip3");
    cmd.arg("install").args(packages);
    let status = cmd.status()?;
    if !status.success() {
        anyhow::bail!("pip3 install failed for runtime: {}", runtime);
    }
    Ok(())
}

fn install_llama_cpp() -> anyhow::Result<()> {
    // Install llama-cpp-python with Metal support
    let status = Command::new("pip3")
        .args(["install", "llama-cpp-python"])
        .env("CMAKE_ARGS", "-DGGML_METAL=on")
        .env("FORCE_CMAKE", "1")
        .status()?;
    if !status.success() {
        anyhow::bail!("Failed to install llama-cpp-python with Metal support");
    }
    Ok(())
}
