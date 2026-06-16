//! AMD GPU detection — discrete (ROCm) and integrated APU (Strix Halo / AI Max+).
//!
//! Detection sources, tried in order:
//!   1. `rocm-smi --json`    — best for discrete RDNA/CDNA cards
//!   2. KFD topology sysfs   — works for both discrete and APU (Strix Halo, Strix Point)
//!   3. DRM sysfs fallback   — broad amdgpu kernel driver detection
//!
//! AMD Ryzen AI Max+ 395 (Strix Halo, gfx1152):
//!   - 40 Compute Units (RDNA 3.5)
//!   - Unified memory — shares LPDDR5X with CPU (up to 128 GB)
//!   - Exposed via /sys/class/kfd/kfd/topology/nodes/<n>/properties
//!   - ROCm 6.2+ required for full compute support

use super::GpuBackend;
use crate::types::*;
use anyhow::Result;
use std::process::Command;

pub struct AmdBackend;

impl GpuBackend for AmdBackend {
    fn name(&self) -> &'static str { "amd-rocm" }

    fn is_available(&self) -> bool {
        Command::new("rocm-smi").arg("--version").output()
            .map(|o| o.status.success())
            .unwrap_or(false)
            || std::path::Path::new("/sys/class/kfd/kfd/topology/nodes").exists()
            || std::path::Path::new("/sys/class/drm").exists()
    }

    fn enumerate(&self) -> Result<Vec<GpuInfo>> {
        // 1. Try rocm-smi (best metadata for discrete GPUs)
        if let Ok(gpus) = enumerate_via_rocm_smi() {
            if !gpus.is_empty() { return Ok(gpus); }
        }
        // 2. KFD topology — works for APUs too (Strix Halo)
        if let Ok(gpus) = enumerate_via_kfd_topology() {
            if !gpus.is_empty() { return Ok(gpus); }
        }
        // 3. DRM sysfs fallback
        enumerate_via_sysfs()
    }
}

// ── rocm-smi path ─────────────────────────────────────────────────────────────

fn enumerate_via_rocm_smi() -> Result<Vec<GpuInfo>> {
    let out = Command::new("rocm-smi")
        .args(["--showproductname", "--showmeminfo", "vram", "--json"])
        .output()?;

    anyhow::ensure!(out.status.success(), "rocm-smi failed");
    let v: serde_json::Value = serde_json::from_slice(&out.stdout)?;

    let rocm_ver = rocm_version();
    let mut gpus = Vec::new();

    if let Some(cards) = v.as_object() {
        for (key, card) in cards {
            if !key.starts_with("card") { continue; }
            let idx: u32 = key.trim_start_matches("card").parse().unwrap_or(0);
            let model  = card["Card series"].as_str()
                .or_else(|| card["Card model"].as_str())
                .unwrap_or("AMD GPU")
                .to_string();

            // Integrated GPUs (APU) report 0 or a very small "VRAM" because they
            // share system memory. Detect this and substitute system RAM.
            let vram_bytes: u64 = card["VRAM Total Memory (B)"]
                .as_str().unwrap_or("0").parse().unwrap_or(0);
            let (vram_gb, is_apu) = if vram_bytes < 512 * 1024 * 1024 {
                (system_memory_gb(), true)
            } else {
                ((vram_bytes / 1024 / 1024 / 1024) as u32, false)
            };

            let cu_count = detect_cu_count_for_model(&model);
            let gfx_ver  = detect_gfx_target_from_rocm_smi(card);
            let extra    = build_extra_string(is_apu, &gfx_ver, &rocm_ver);

            gpus.push(GpuInfo {
                vendor: GpuVendor::Amd,
                model,
                vram_gb,
                compute_cores: cu_count,
                compute_api: ComputeApi::Rocm,
                capability: capability_for_amd(vram_gb, &gfx_ver, is_apu),
                driver_version: rocm_ver.clone(),
                device_index: idx,
                device_id: format!("card{}", idx),
                platform: Platform::Linux,
                extra: Some(extra),
            });
        }
    }
    Ok(gpus)
}

// ── KFD topology path (APU-aware) ─────────────────────────────────────────────

/// KFD topology exposes every HSA node — CPU nodes have cpu_cores_count > 0,
/// GPU nodes have simd_count > 0 and cpu_cores_count == 0.
fn enumerate_via_kfd_topology() -> Result<Vec<GpuInfo>> {
    let kfd_nodes = std::path::Path::new("/sys/class/kfd/kfd/topology/nodes");
    if !kfd_nodes.exists() { return Ok(vec![]); }

    let rocm_ver = rocm_version();
    let mut gpus  = Vec::new();
    let mut idx   = 0u32;

    let mut entries: Vec<_> = std::fs::read_dir(kfd_nodes)?
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let props_path = entry.path().join("properties");
        let Ok(props) = std::fs::read_to_string(&props_path) else { continue };

        let props_map = parse_kfd_properties(&props);

        // Skip CPU nodes
        let simd_count: u32 = props_map.get("simd_count")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        if simd_count == 0 { continue; }

        let cpu_cores: u32 = props_map.get("cpu_cores_count")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        // A node with both CPU cores and SIMDs is an APU GPU node
        let is_apu = cpu_cores > 0
            || props_map.get("is_apu").map(|v| v == "1").unwrap_or(false);

        // RDNA/CDNA: SIMDs per CU depends on architecture generation.
        // RDNA 3.x (gfx11xx): 4 SIMDs per CU × 32 threads = 128 threads/CU
        // CDNA (gfx9xx): 4 SIMDs per CU as well.
        // KFD reports simd_count = total SIMD units.
        let gfx_target = props_map.get("gfx_target_version")
            .cloned()
            .unwrap_or_default();
        let cu_count = simd_count / simds_per_cu(&gfx_target);

        // Memory: APU uses system RAM; discrete GPU reads local_mem_size.
        let local_mem_kb: u64 = props_map.get("local_mem_size")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let (vram_gb, is_apu_final) = if local_mem_kb < 512 * 1024 || is_apu {
            (system_memory_gb(), true)
        } else {
            ((local_mem_kb / 1024 / 1024) as u32, false)
        };

        // Read model name from marketing_name file if present
        let model = std::fs::read_to_string(entry.path().join("name"))
            .unwrap_or_else(|_| model_from_gfx_target(&gfx_target));

        let extra = build_extra_string(is_apu_final, &gfx_target, &rocm_ver);

        gpus.push(GpuInfo {
            vendor: GpuVendor::Amd,
            model,
            vram_gb,
            compute_cores: cu_count,
            compute_api: ComputeApi::Rocm,
            capability: capability_for_amd(vram_gb, &gfx_target, is_apu_final),
            driver_version: rocm_ver.clone(),
            device_index: idx,
            device_id: format!("kfd-node-{}", entry.file_name().to_string_lossy()),
            platform: Platform::Linux,
            extra: Some(extra),
        });
        idx += 1;
    }
    Ok(gpus)
}

// ── DRM sysfs fallback ────────────────────────────────────────────────────────

fn enumerate_via_sysfs() -> Result<Vec<GpuInfo>> {
    let mut gpus = Vec::new();
    let drm = std::path::Path::new("/sys/class/drm");
    if !drm.exists() { return Ok(gpus); }

    let mut idx = 0u32;
    let mut entries: Vec<_> = std::fs::read_dir(drm)?
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        // Only primary render nodes (card0, card1, …) not connectors (card0-DP-1)
        if !name_str.starts_with("card") || name_str.contains('-') { continue; }

        let vendor_path = entry.path().join("device/vendor");
        let Ok(vendor) = std::fs::read_to_string(&vendor_path) else { continue };
        if vendor.trim() != "0x1002" { continue; } // AMD PCI vendor ID

        let model = std::fs::read_to_string(entry.path().join("device/product_name"))
            .unwrap_or_else(|_| "AMD GPU".to_string())
            .trim().to_string();

        let vram_total = std::fs::read_to_string(
            entry.path().join("device/mem_info_vram_total")
        ).ok()
            .and_then(|s| s.trim().parse::<u64>().ok())
            .unwrap_or(0);

        let (vram_gb, is_apu) = if vram_total < 512 * 1024 * 1024 {
            (system_memory_gb(), true)
        } else {
            ((vram_total / 1024 / 1024 / 1024) as u32, false)
        };

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
            extra: if is_apu { Some("is_apu=true".into()) } else { None },
        });
        idx += 1;
    }
    Ok(gpus)
}

// ── Live stats ─────────────────────────────────────────────────────────────────

pub fn sample_stats() -> Vec<GpuStats> {
    let out = match Command::new("rocm-smi")
        .args(["--showuse", "--showmeminfo", "vram", "--showtemp", "--showpower", "--json"])
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return vec![],
    };

    let v: serde_json::Value = match serde_json::from_slice(&out.stdout) {
        Ok(v) => v,
        Err(_) => return vec![],
    };

    let mut stats = Vec::new();
    if let Some(cards) = v.as_object() {
        for (key, card) in cards {
            if !key.starts_with("card") { continue; }
            let idx: u32 = key.trim_start_matches("card").parse().unwrap_or(0);

            let util_pct: f32 = card["GPU use (%)"]
                .as_str().unwrap_or("0").trim_end_matches('%').parse().unwrap_or(0.0);
            let vram_used_mb: u64 = card["VRAM Total Used Memory (B)"]
                .as_str().unwrap_or("0").parse::<u64>().unwrap_or(0) / 1024 / 1024;
            let vram_total_mb: u64 = card["VRAM Total Memory (B)"]
                .as_str().unwrap_or("0").parse::<u64>().unwrap_or(0) / 1024 / 1024;
            let temp: f32 = card["Temperature (Sensor edge) (C)"]
                .as_str().unwrap_or("0").parse().unwrap_or(0.0);
            let power: f32 = card["Average Graphics Package Power (W)"]
                .as_str().unwrap_or("0").parse().unwrap_or(0.0);

            stats.push(GpuStats {
                device_index: idx,
                utilisation_pct: util_pct,
                vram_used_mb,
                vram_total_mb,
                temp_celsius: temp,
                power_draw_w: power,
                clock_mhz: 0, // rocm-smi clock requires separate call
            });
        }
    }
    stats
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn parse_kfd_properties(content: &str) -> std::collections::HashMap<String, String> {
    content.lines()
        .filter_map(|line| {
            let mut parts = line.splitn(2, ' ');
            let key = parts.next()?.trim().to_string();
            let val = parts.next()?.trim().to_string();
            Some((key, val))
        })
        .collect()
}

/// Number of SIMD units per Compute Unit for each GFX family.
/// KFD topology reports simd_count = total SIMDs; divide to get CU count.
fn simds_per_cu(gfx_target: &str) -> u32 {
    match gfx_target {
        // RDNA 3.x (gfx11xx) — includes Strix Halo (1152), Strix Point (1151), Navi31 (1100)
        t if t.starts_with("11") => 4,
        // RDNA 2 (gfx10xx, Navi2x)
        t if t.starts_with("10") => 4,
        // CDNA (gfx90x, MI200/MI300)
        t if t.starts_with("9")  => 4,
        // Default
        _                        => 4,
    }
}

/// Human-readable model name derived from GFX target version string.
fn model_from_gfx_target(gfx: &str) -> String {
    match gfx {
        "1152" => "AMD Radeon 890M/895M (Strix Halo, AI Max+ 395)".to_string(),
        "1151" => "AMD Radeon 880M (Strix Point, AI 370)".to_string(),
        "1100" => "AMD Radeon RX 7900 XTX (Navi31)".to_string(),
        "1101" => "AMD Radeon RX 7900 XT (Navi31)".to_string(),
        "1102" => "AMD Radeon RX 7800 XT (Navi32)".to_string(),
        "1103" => "AMD Radeon RX 7600 (Navi33)".to_string(),
        "942"  => "AMD Instinct MI300X (CDNA3)".to_string(),
        "940"  => "AMD Instinct MI250X (CDNA2)".to_string(),
        _      => format!("AMD GPU (gfx{})", gfx),
    }
}

fn detect_gfx_target_from_rocm_smi(card: &serde_json::Value) -> String {
    // rocm-smi may not expose gfx target directly; we can read it from sysfs
    // as a post-process. For now return empty and let capability_for_amd handle it.
    card.get("GFX Version")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

/// CU count heuristic for model name strings (used when KFD sysfs is unavailable).
fn detect_cu_count_for_model(model: &str) -> u32 {
    let m = model.to_lowercase();
    // Strix Halo (AI Max+ 395)
    if m.contains("895m") || m.contains("890m") || m.contains("strix halo") { return 40; }
    // Strix Point (AI 370/375)
    if m.contains("880m") || m.contains("strix point") { return 12; }
    // RDNA 3 discrete
    if m.contains("7900 xtx") || m.contains("7900 gre") { return 96; }
    if m.contains("7900 xt")  { return 84; }
    if m.contains("7800 xt")  { return 60; }
    if m.contains("7700 xt")  { return 54; }
    if m.contains("7600")     { return 32; }
    // CDNA (MI series)
    if m.contains("mi300x")   { return 304; }
    if m.contains("mi300a")   { return 228; }
    if m.contains("mi250x")   { return 220; }
    if m.contains("mi210")    { return 104; }
    0
}

fn capability_for_amd(vram_gb: u32, gfx_target: &str, is_apu: bool) -> GpuCapability {
    // MI300X (CDNA3, 192GB HBM3) — training grade
    if gfx_target == "942" { return GpuCapability::Training; }
    // MI250X (CDNA2, 128GB HBM2e) — training grade
    if gfx_target == "940" { return GpuCapability::Training; }
    // Strix Halo APU with 64+ GB system RAM — can handle large inference
    if is_apu && vram_gb >= 64 { return GpuCapability::InferenceLarge; }
    if is_apu && vram_gb >= 32 { return GpuCapability::InferenceMid; }
    GpuCapability::from_vram_gb(vram_gb)
}

fn build_extra_string(is_apu: bool, gfx_target: &str, rocm_ver: &str) -> String {
    let mut parts = Vec::new();
    if is_apu { parts.push("is_apu=true".to_string()); }
    if !gfx_target.is_empty() { parts.push(format!("gfx_target={}", gfx_target)); }
    if !rocm_ver.is_empty()   { parts.push(format!("rocm={}", rocm_ver)); }
    parts.join(",")
}

/// Total system physical memory in GB (used for APU VRAM reporting).
fn system_memory_gb() -> u32 {
    // /proc/meminfo: "MemTotal:       131072 kB"
    if let Ok(content) = std::fs::read_to_string("/proc/meminfo") {
        for line in content.lines() {
            if line.starts_with("MemTotal:") {
                let kb: u64 = line.split_whitespace()
                    .nth(1).unwrap_or("0").parse().unwrap_or(0);
                return (kb / 1024 / 1024) as u32;
            }
        }
    }
    0
}

fn rocm_version() -> String {
    // Try rocm-smi --version first
    if let Ok(out) = Command::new("rocm-smi").arg("--version").output() {
        let s = String::from_utf8_lossy(&out.stdout);
        for line in s.lines() {
            if let Some(ver) = line.strip_prefix("ROCm-SMI version: ") {
                return ver.trim().to_string();
            }
        }
    }
    // Fallback: read /opt/rocm/share/info/version
    std::fs::read_to_string("/opt/rocm/share/info/version")
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}
