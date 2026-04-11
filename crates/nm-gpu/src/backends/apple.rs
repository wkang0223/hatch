//! Apple Silicon GPU detection via system_profiler and sysctl.
//! Mirrors the existing nm-macos/gpu_detect.rs but wrapped in the unified API.

use super::GpuBackend;
use crate::types::*;
use anyhow::Result;
use std::process::Command;

pub struct AppleBackend;

impl GpuBackend for AppleBackend {
    fn name(&self) -> &'static str { "apple-metal" }

    fn is_available(&self) -> bool {
        cfg!(target_os = "macos")
    }

    fn enumerate(&self) -> Result<Vec<GpuInfo>> {
        #[cfg(not(target_os = "macos"))]
        return Ok(vec![]);

        #[cfg(target_os = "macos")]
        {
            let model = detect_chip_model();
            let vram_gb = detect_unified_memory_gb();
            let cores = detect_gpu_cores(&model);

            Ok(vec![GpuInfo {
                vendor: GpuVendor::Apple,
                model: model.clone(),
                vram_gb,
                compute_cores: cores,
                compute_api: ComputeApi::Metal,
                capability: capability_for_apple(&model),
                driver_version: macos_version(),
                device_index: 0,
                device_id: platform_uuid(),
                platform: Platform::Macos,
            }])
        }
    }
}

#[cfg(target_os = "macos")]
fn detect_chip_model() -> String {
    let out = Command::new("sysctl")
        .args(["-n", "machdep.cpu.brand_string"])
        .output();
    if let Ok(o) = out {
        let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
        if !s.is_empty() { return s; }
    }
    // Fallback: system_profiler
    let out = Command::new("system_profiler")
        .args(["SPHardwareDataType", "-json"])
        .output();
    if let Ok(o) = out {
        if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&o.stdout) {
            if let Some(chip) = v["SPHardwareDataType"][0]["chip_type"].as_str() {
                return chip.to_string();
            }
        }
    }
    "Apple Silicon".to_string()
}

#[cfg(target_os = "macos")]
fn detect_unified_memory_gb() -> u32 {
    let out = Command::new("sysctl")
        .args(["-n", "hw.memsize"])
        .output();
    if let Ok(o) = out {
        if let Ok(bytes) = String::from_utf8_lossy(&o.stdout).trim().parse::<u64>() {
            return (bytes / 1024 / 1024 / 1024) as u32;
        }
    }
    0
}

#[cfg(target_os = "macos")]
fn detect_gpu_cores(model: &str) -> u32 {
    // Heuristic map of known Apple Silicon GPU core counts
    let model_lower = model.to_lowercase();
    if model_lower.contains("m4 ultra")  { return 80; }
    if model_lower.contains("m4 max")    { return 40; }
    if model_lower.contains("m4 pro")    { return 20; }
    if model_lower.contains("m4")        { return 10; }
    if model_lower.contains("m3 ultra")  { return 60; }
    if model_lower.contains("m3 max")    { return 40; }
    if model_lower.contains("m3 pro")    { return 18; }
    if model_lower.contains("m3")        { return 10; }
    if model_lower.contains("m2 ultra")  { return 60; }
    if model_lower.contains("m2 max")    { return 38; }
    if model_lower.contains("m2 pro")    { return 19; }
    if model_lower.contains("m2")        { return 10; }
    if model_lower.contains("m1 ultra")  { return 64; }
    if model_lower.contains("m1 max")    { return 32; }
    if model_lower.contains("m1 pro")    { return 16; }
    if model_lower.contains("m1")        { return 8;  }
    0
}

#[cfg(target_os = "macos")]
fn capability_for_apple(model: &str) -> GpuCapability {
    let m = model.to_lowercase();
    if m.contains("ultra") { return GpuCapability::Training; }
    if m.contains("max")   { return GpuCapability::InferenceLarge; }
    if m.contains("pro")   { return GpuCapability::InferenceMid; }
    GpuCapability::InferenceSmall
}

#[cfg(target_os = "macos")]
fn macos_version() -> String {
    let out = Command::new("sw_vers").args(["-productVersion"]).output();
    out.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

#[cfg(target_os = "macos")]
fn platform_uuid() -> String {
    let out = Command::new("ioreg")
        .args(["-rd1", "-c", "IOPlatformExpertDevice"])
        .output();
    if let Ok(o) = out {
        let s = String::from_utf8_lossy(&o.stdout);
        for line in s.lines() {
            if line.contains("IOPlatformUUID") {
                if let Some(uuid) = line.split('"').nth(3) {
                    return uuid.to_string();
                }
            }
        }
    }
    "unknown".to_string()
}
