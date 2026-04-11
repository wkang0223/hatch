//! AMD GPU detection via rocm-smi (ROCm) or sysfs fallback.
//!
//! ROCm supports: RDNA 2+ (RX 6000+), CDNA (MI series) on Linux.
//! Windows support uses HIP (via rocm-smi64 or WMI fallback).

use super::GpuBackend;
use crate::types::*;
use anyhow::Result;
use std::process::Command;

pub struct AmdBackend;

impl GpuBackend for AmdBackend {
    fn name(&self) -> &'static str { "amd-rocm" }

    fn is_available(&self) -> bool {
        // Try rocm-smi first, then amdgpu sysfs
        Command::new("rocm-smi").arg("--version").output()
            .map(|o| o.status.success())
            .unwrap_or(false)
            || std::path::Path::new("/sys/class/drm").exists()
    }

    fn enumerate(&self) -> Result<Vec<GpuInfo>> {
        // Try rocm-smi JSON output
        if let Ok(gpus) = enumerate_via_rocm_smi() {
            if !gpus.is_empty() { return Ok(gpus); }
        }
        // Fallback: sysfs
        enumerate_via_sysfs()
    }
}

fn enumerate_via_rocm_smi() -> Result<Vec<GpuInfo>> {
    let out = Command::new("rocm-smi")
        .args(["--showproductname", "--showmeminfo", "vram", "--json"])
        .output()?;

    anyhow::ensure!(out.status.success(), "rocm-smi failed");
    let v: serde_json::Value = serde_json::from_slice(&out.stdout)?;

    let mut gpus = Vec::new();
    if let Some(cards) = v.as_object() {
        for (key, card) in cards {
            if !key.starts_with("card") { continue; }
            let idx: u32 = key.trim_start_matches("card").parse().unwrap_or(0);
            let model = card["Card series"].as_str().unwrap_or("AMD GPU").to_string();
            let vram_bytes: u64 = card["VRAM Total Memory (B)"]
                .as_str().unwrap_or("0").parse().unwrap_or(0);
            let vram_gb = (vram_bytes / 1024 / 1024 / 1024) as u32;

            gpus.push(GpuInfo {
                vendor: GpuVendor::Amd,
                model,
                vram_gb,
                compute_cores: 0, // rocm-smi doesn't report CU count directly
                compute_api: ComputeApi::Rocm,
                capability: GpuCapability::from_vram_gb(vram_gb),
                driver_version: rocm_version(),
                device_index: idx,
                device_id: format!("card{}", idx),
                platform: Platform::Linux,
            });
        }
    }
    Ok(gpus)
}

/// Sysfs fallback: reads /sys/class/drm/card*/device/vendor (0x1002 = AMD).
fn enumerate_via_sysfs() -> Result<Vec<GpuInfo>> {
    let mut gpus = Vec::new();
    let drm = std::path::Path::new("/sys/class/drm");
    if !drm.exists() { return Ok(gpus); }

    let mut idx = 0u32;
    for entry in std::fs::read_dir(drm)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if !name_str.starts_with("card") || name_str.contains('-') { continue; }

        let vendor_path = entry.path().join("device/vendor");
        if let Ok(vendor) = std::fs::read_to_string(&vendor_path) {
            if vendor.trim() != "0x1002" { continue; } // AMD PCI vendor ID
        } else { continue; }

        let model = std::fs::read_to_string(entry.path().join("device/product_name"))
            .unwrap_or_else(|_| "AMD GPU".to_string())
            .trim().to_string();

        // Try to read VRAM from sysfs mem_info
        let vram_total = std::fs::read_to_string(
            entry.path().join("device/mem_info_vram_total")
        ).ok()
            .and_then(|s| s.trim().parse::<u64>().ok())
            .unwrap_or(0);
        let vram_gb = (vram_total / 1024 / 1024 / 1024) as u32;

        gpus.push(GpuInfo {
            vendor: GpuVendor::Amd,
            model,
            vram_gb,
            compute_cores: 0,
            compute_api: ComputeApi::Rocm,
            capability: GpuCapability::from_vram_gb(vram_gb),
            driver_version: String::new(),
            device_index: idx,
            device_id: name_str.to_string(),
            platform: Platform::Linux,
        });
        idx += 1;
    }
    Ok(gpus)
}

fn rocm_version() -> String {
    Command::new("rocm-smi").arg("--version")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}
