//! Detect Apple Silicon GPU/chip info via IOKit and system_profiler.

use anyhow::{bail, Context, Result};
use nm_common::MacChipInfo;
use std::process::Command;
use tracing::debug;

#[derive(Debug, Clone)]
pub struct GpuStats {
    /// Current GPU utilization 0–100
    pub utilization_pct: f32,
    /// GPU VRAM / unified memory used in MB
    pub memory_used_mb: u64,
    /// Total unified memory in MB
    pub memory_total_mb: u64,
}

/// Detect chip information by parsing `sysctl` and `system_profiler`.
/// This works on all Apple Silicon Macs without any special entitlements.
pub fn detect_mac_chip() -> Result<MacChipInfo> {
    let chip_model   = sysctl_str("machdep.cpu.brand_string")
        .or_else(|_| sysctl_str("hw.model"))
        .unwrap_or_else(|_| "Apple Silicon".into());
    let chip_model   = normalize_chip_name(&chip_model);

    let memory_bytes: u64 = sysctl_u64("hw.memsize").context("hw.memsize")?;
    let unified_memory_gb  = (memory_bytes / (1024 * 1024 * 1024)) as u32;

    let cpu_cores: u32 = sysctl_u64("hw.logicalcpu")
        .unwrap_or(8) as u32;

    let gpu_cores = detect_gpu_cores(&chip_model);

    let serial_number = ioreg_value("IOPlatformSerialNumber")
        .unwrap_or_else(|_| "UNKNOWN".into());

    let platform_uuid = ioreg_value("IOPlatformUUID")
        .unwrap_or_else(|_| "UNKNOWN".into());

    let metal_version = detect_metal_version();
    let macos_version = detect_macos_version();

    debug!(
        chip = %chip_model,
        ram_gb = unified_memory_gb,
        gpu_cores = gpu_cores,
        serial = %serial_number,
        "Detected Mac chip"
    );

    Ok(MacChipInfo {
        chip_model,
        unified_memory_gb,
        gpu_cores,
        cpu_cores,
        metal_version,
        serial_number,
        platform_uuid,
        macos_version,
    })
}

/// Sample current GPU utilization using `ioreg`.
/// Returns None if unable to read (non-fatal — fall back to 0%).
pub fn sample_gpu_utilization() -> Option<GpuStats> {
    // Use `sudo powermetrics` for real GPU util, but that requires root.
    // Without root, use the IOAccelerator registry for a best-effort stat.
    let output = Command::new("ioreg")
        .args(["-r", "-d", "1", "-n", "AGXAccelerator"])
        .output()
        .ok()?;

    let text = String::from_utf8_lossy(&output.stdout);

    // Parse "PerformanceStatistics" dict from ioreg output
    let util_pct = parse_ioreg_float(&text, "Device Utilization %")
        .or_else(|| parse_ioreg_float(&text, "GPU Activity(%)"))
        .unwrap_or(0.0);

    let memory_used_mb = parse_ioreg_u64(&text, "IOGPUOutstandingBufferMB")
        .unwrap_or(0);

    let memory_total_mb = {
        let bytes = sysctl_u64("hw.memsize").unwrap_or(0);
        bytes / (1024 * 1024)
    };

    Some(GpuStats {
        utilization_pct: util_pct,
        memory_used_mb,
        memory_total_mb,
    })
}

// ──────────────────────────────────────────────
// Private helpers
// ──────────────────────────────────────────────

fn sysctl_str(key: &str) -> Result<String> {
    let out = Command::new("sysctl")
        .arg("-n").arg(key)
        .output()?;
    if !out.status.success() {
        bail!("sysctl {} failed", key);
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn sysctl_u64(key: &str) -> Result<u64> {
    let s = sysctl_str(key)?;
    s.trim().parse::<u64>().context("parse sysctl u64")
}

fn ioreg_value(key: &str) -> Result<String> {
    let out = Command::new("ioreg")
        .args(["-rd1", "-c", "IOPlatformExpertDevice"])
        .output()?;
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        if line.contains(key) {
            // Line format: `  "IOPlatformSerialNumber" = "C02XXXXXX"`
            if let Some(val) = line.split('=').nth(1) {
                return Ok(val.trim().trim_matches('"').to_string());
            }
        }
    }
    bail!("ioreg key {} not found", key)
}

fn detect_gpu_cores(chip_model: &str) -> u32 {
    // Best-effort lookup by chip name substring
    let m = chip_model.to_lowercase();
    if m.contains("m4 ultra")   { return 80; }
    if m.contains("m3 ultra")   { return 80; }
    if m.contains("m2 ultra")   { return 76; }
    if m.contains("m1 ultra")   { return 64; }
    if m.contains("m4 max")     { return 40; }
    if m.contains("m3 max")     { return 40; }
    if m.contains("m2 max")     { return 38; }
    if m.contains("m1 max")     { return 32; }
    if m.contains("m4 pro")     { return 20; }
    if m.contains("m3 pro")     { return 18; }
    if m.contains("m2 pro")     { return 19; }
    if m.contains("m1 pro")     { return 16; }
    if m.contains("m4")         { return 10; }
    if m.contains("m3")         { return 10; }
    if m.contains("m2")         { return 10; }
    if m.contains("m1")         { return 8;  }
    10 // fallback
}

fn normalize_chip_name(raw: &str) -> String {
    // `sysctl machdep.cpu.brand_string` on Apple Silicon returns e.g. "Apple M4 Pro"
    // `hw.model` returns e.g. "Mac15,3"
    if raw.starts_with("Apple M") {
        return raw.to_string();
    }
    // Fallback: try system_profiler
    if let Ok(out) = Command::new("system_profiler")
        .args(["SPHardwareDataType"])
        .output()
    {
        let text = String::from_utf8_lossy(&out.stdout);
        for line in text.lines() {
            if line.trim_start().starts_with("Chip:") {
                if let Some(val) = line.split(':').nth(1) {
                    return val.trim().to_string();
                }
            }
        }
    }
    raw.to_string()
}

fn detect_metal_version() -> String {
    // system_profiler SPDisplaysDataType contains "Metal: Supported, feature set..."
    if let Ok(out) = Command::new("system_profiler")
        .args(["SPDisplaysDataType"])
        .output()
    {
        let text = String::from_utf8_lossy(&out.stdout);
        for line in text.lines() {
            if line.contains("Metal") && line.contains("GPUFamily") {
                // e.g. "Metal: Supported, feature set macOS GPUFamily2 v1"
                return line.trim().to_string();
            }
        }
    }
    "Metal (version unknown)".into()
}

fn detect_macos_version() -> String {
    Command::new("sw_vers")
        .arg("-productVersion")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "unknown".into())
}

fn parse_ioreg_float(text: &str, key: &str) -> Option<f32> {
    for line in text.lines() {
        if line.contains(key) {
            let parts: Vec<&str> = line.splitn(2, '=').collect();
            if parts.len() == 2 {
                return parts[1].trim().trim_matches('"').parse::<f32>().ok();
            }
        }
    }
    None
}

fn parse_ioreg_u64(text: &str, key: &str) -> Option<u64> {
    for line in text.lines() {
        if line.contains(key) {
            let parts: Vec<&str> = line.splitn(2, '=').collect();
            if parts.len() == 2 {
                return parts[1].trim().trim_matches('"').parse::<u64>().ok();
            }
        }
    }
    None
}
