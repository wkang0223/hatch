//! Intel Arc GPU detection via xpu-smi (Intel's management tool) or sysfs.
//!
//! Intel Arc (Alchemist / Battlemage) uses the xe/i915 kernel driver on Linux
//! and the Intel Graphics driver on Windows.
//! Compute: Intel oneAPI (SYCL), with OpenCL and LevelZero backends.
//!
//! Supported models: Arc A-series (A310, A380, A770, A750),
//!                   Arc B-series (Battlemage), Flex series (data centre).

use super::GpuBackend;
use crate::types::*;
use anyhow::Result;
use std::process::Command;

pub struct IntelArcBackend;

impl GpuBackend for IntelArcBackend {
    fn name(&self) -> &'static str { "intel-arc-oneapi" }

    fn is_available(&self) -> bool {
        // xpu-smi is Intel's management tool (oneAPI toolkit)
        Command::new("xpu-smi").arg("discovery").output()
            .map(|o| o.status.success())
            .unwrap_or(false)
            // fallback: check sysfs for Intel vendor (0x8086)
            || intel_present_in_sysfs()
    }

    fn enumerate(&self) -> Result<Vec<GpuInfo>> {
        if let Ok(gpus) = enumerate_via_xpu_smi() {
            if !gpus.is_empty() { return Ok(gpus); }
        }
        enumerate_via_sysfs()
    }
}

fn enumerate_via_xpu_smi() -> Result<Vec<GpuInfo>> {
    // xpu-smi discovery -j returns a JSON array of devices
    let out = Command::new("xpu-smi")
        .args(["discovery", "-j"])
        .output()?;

    anyhow::ensure!(out.status.success(), "xpu-smi failed");
    let v: serde_json::Value = serde_json::from_slice(&out.stdout)?;

    let mut gpus = Vec::new();
    if let Some(devices) = v["device_list"].as_array() {
        for (idx, dev) in devices.iter().enumerate() {
            let model = dev["device_name"].as_str().unwrap_or("Intel Arc").to_string();
            let vram_mb: u64 = dev["memory_physical_size"].as_u64().unwrap_or(0);
            let vram_gb = (vram_mb / 1024) as u32;
            let driver = dev["driver_version"].as_str().unwrap_or("").to_string();
            let device_id = dev["pci_bdf_address"].as_str().unwrap_or("").to_string();

            gpus.push(GpuInfo {
                vendor: GpuVendor::IntelArc,
                model: model.clone(),
                vram_gb,
                compute_cores: intel_eu_count(&model),
                compute_api: ComputeApi::OneApi,
                capability: GpuCapability::from_vram_gb(vram_gb),
                driver_version: driver,
                device_index: idx as u32,
                device_id,
                platform: Platform::Linux,
            });
        }
    }
    Ok(gpus)
}

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
            if vendor.trim() != "0x8086" { continue; } // Intel PCI vendor ID
        } else { continue; }

        // Only Arc / Xe class — check device class
        let model = std::fs::read_to_string(entry.path().join("device/product_name"))
            .unwrap_or_else(|_| "Intel Arc".to_string())
            .trim().to_string();

        // Intel sysfs exposes lmem (local memory) for discrete GPUs
        let vram_total = std::fs::read_to_string(
            entry.path().join("device/tile0/gt/gt0/lmem_max")
        ).ok()
            .and_then(|s| s.trim().parse::<u64>().ok())
            .unwrap_or(0);
        let vram_gb = (vram_total / 1024 / 1024 / 1024) as u32;

        // Skip integrated graphics (no local memory)
        if vram_gb == 0 { continue; }

        gpus.push(GpuInfo {
            vendor: GpuVendor::IntelArc,
            model,
            vram_gb,
            compute_cores: 0,
            compute_api: ComputeApi::OneApi,
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

/// EU (Execution Unit) count for known Arc SKUs.
fn intel_eu_count(model: &str) -> u32 {
    let m = model.to_lowercase();
    // Battlemage
    if m.contains("b580") { return 20 * 16; } // 20 Xe2 cores * 16 EUs
    if m.contains("b770") { return 32 * 16; }
    // Alchemist
    if m.contains("a770") { return 512; }
    if m.contains("a750") { return 448; }
    if m.contains("a580") { return 256; }
    if m.contains("a380") { return 128; }
    if m.contains("a310") { return 64;  }
    // Flex (data centre)
    if m.contains("flex 170") { return 512; }
    if m.contains("flex 140") { return 128; }
    0
}

fn intel_present_in_sysfs() -> bool {
    let drm = std::path::Path::new("/sys/class/drm");
    if !drm.exists() { return false; }
    std::fs::read_dir(drm).ok().map(|mut d| {
        d.any(|e| {
            e.ok().and_then(|e| {
                std::fs::read_to_string(e.path().join("device/vendor")).ok()
                    .map(|v| v.trim() == "0x8086")
            }).unwrap_or(false)
        })
    }).unwrap_or(false)
}
