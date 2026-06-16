//! NVIDIA GPU detection via nvidia-smi subprocess.
//!
//! Supports RTX 3xxx–7xxx consumer and Hopper/Blackwell data-center cards.
//! Architecture map:
//!   RTX 30xx  → Ampere   (GA10x, sm_86)
//!   RTX 40xx  → Ada      (AD10x, sm_89)
//!   RTX 50xx  → Blackwell consumer (GB2xx, sm_120)
//!   RTX 60xx  → Rubin    (GR10x, sm_100-class next-gen, compute_cap 13.x)
//!   RTX 70xx  → post-Rubin (sm_14x range, speculative)
//!   B100/B200 → Blackwell datacenter (sm_100)
//!   H100/H200 → Hopper datacenter (sm_90)
//!
//! CUDA core counts fall back to 0 for unreleased or unrecognised models —
//! capability tiers are still assigned correctly via VRAM.

use super::GpuBackend;
use crate::types::*;
use anyhow::Result;
use std::process::Command;

pub struct NvidiaBackend;

impl GpuBackend for NvidiaBackend {
    fn name(&self) -> &'static str { "nvidia-cuda" }

    fn is_available(&self) -> bool {
        Command::new("nvidia-smi")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn enumerate(&self) -> Result<Vec<GpuInfo>> {
        // Single call: index, name, VRAM (MiB), driver, PCI bus, compute capability
        let out = Command::new("nvidia-smi")
            .args([
                "--query-gpu=index,name,memory.total,driver_version,pci.bus_id,compute_cap",
                "--format=csv,noheader,nounits",
            ])
            .output()?;

        anyhow::ensure!(out.status.success(), "nvidia-smi failed");

        let stdout = String::from_utf8_lossy(&out.stdout);
        let mut gpus = Vec::new();

        for line in stdout.lines() {
            let parts: Vec<&str> = line.split(',').map(str::trim).collect();
            if parts.len() < 5 { continue; }

            let index: u32    = parts[0].parse().unwrap_or(0);
            let model          = parts[1].to_string();
            let vram_mb: u64  = parts[2].parse().unwrap_or(0);
            let vram_gb        = (vram_mb / 1024) as u32;
            let driver         = parts[3].to_string();
            let device_id      = parts[4].to_string();
            // parts[5] = compute capability, e.g. "12.0" (Blackwell GB2xx), "10.0" (B100)
            let compute_cap    = parts.get(5).map(|s| s.to_string()).unwrap_or_default();

            gpus.push(GpuInfo {
                vendor: GpuVendor::Nvidia,
                model: model.clone(),
                vram_gb,
                compute_cores: nvidia_cuda_cores(&model, &compute_cap),
                compute_api: ComputeApi::Cuda,
                capability: capability_from_nvidia(vram_gb, &compute_cap),
                driver_version: driver,
                device_index: index,
                device_id,
                platform: current_platform(),
                extra: Some(format!("compute_cap={}", compute_cap)),
            });
        }
        Ok(gpus)
    }
}

/// Sample live stats for all NVIDIA GPUs.
pub fn sample_stats() -> Vec<GpuStats> {
    let out = match Command::new("nvidia-smi")
        .args([
            "--query-gpu=index,utilization.gpu,memory.used,memory.total,temperature.gpu,power.draw,clocks.gr",
            "--format=csv,noheader,nounits",
        ])
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return vec![],
    };

    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut stats = Vec::new();

    for line in stdout.lines() {
        let parts: Vec<&str> = line.split(',').map(str::trim).collect();
        if parts.len() < 7 { continue; }

        let index: u32          = parts[0].parse().unwrap_or(0);
        let util_pct: f32       = parts[1].parse().unwrap_or(0.0);
        let vram_used_mb: u64   = parts[2].parse().unwrap_or(0);
        let vram_total_mb: u64  = parts[3].parse().unwrap_or(0);
        let temp: f32           = parts[4].parse().unwrap_or(0.0);
        let power: f32          = parts[5].parse().unwrap_or(0.0);
        let clock: u32          = parts[6].parse().unwrap_or(0);

        stats.push(GpuStats {
            device_index: index,
            utilisation_pct: util_pct,
            vram_used_mb,
            vram_total_mb,
            temp_celsius: temp,
            power_draw_w: power,
            clock_mhz: clock,
        });
    }
    stats
}

/// CUDA core count heuristic by model name + compute capability.
/// Always check more-specific names (Ti, Super, XT) before the base model.
fn nvidia_cuda_cores(model: &str, compute_cap: &str) -> u32 {
    let m = model.to_lowercase();

    // ── RTX 70xx — post-Rubin (sm_14x, speculative) ───────────────────────────
    // Specs unknown; 0 causes capability tier to fall back to VRAM-based logic.
    if m.contains("7090") { return 0; }
    if m.contains("7080") { return 0; }
    if m.contains("7070") { return 0; }
    if m.contains("7060") { return 0; }
    if m.contains("7050") { return 0; }

    // ── RTX 60xx — Rubin (GR10x, sm_13x) ─────────────────────────────────────
    // Exact core counts will update once specs are official.
    if m.contains("6090") { return 0; }
    if m.contains("6080") { return 0; }
    if m.contains("6070 ti")  { return 0; }
    if m.contains("6070")     { return 0; }
    if m.contains("6060 ti")  { return 0; }
    if m.contains("6060")     { return 0; }
    if m.contains("6050")     { return 0; }

    // ── RTX 50xx — Blackwell consumer (GB2xx, sm_120) ─────────────────────────
    if m.contains("5090")         { return 21_760; }  // GB202: 170 SM × 128
    if m.contains("5080")         { return 10_752; }  // GB203:  84 SM × 128
    if m.contains("5070 ti")      { return  8_960; }  // GB203:  70 SM × 128
    if m.contains("5070")         { return  6_144; }  // GB205:  48 SM × 128
    if m.contains("5060 ti")      { return  4_608; }  // GB206:  36 SM × 128
    if m.contains("5060")         { return  3_072; }  // GB206:  24 SM × 128
    if m.contains("5050")         { return  2_048; }  // GB207 estimate

    // ── Blackwell datacenter (B-series, sm_100) ───────────────────────────────
    if m.contains("b200")         { return 26_624; }  // GB100: 208 SM × 128
    if m.contains("b100")         { return 26_624; }  // GB100: 208 SM × 128
    if m.contains("b40")          { return 18_432; }  // GB204 estimate

    // ── RTX 40xx — Ada Lovelace (AD10x, sm_89) ────────────────────────────────
    if m.contains("4090")              { return 16_384; }  // AD102: 128 SM × 128
    if m.contains("4080 super")        { return 10_240; }  // AD103:  80 SM × 128
    if m.contains("4080")              { return  9_728; }  // AD103:  76 SM × 128
    if m.contains("4070 ti super")     { return  8_448; }  // AD103:  66 SM × 128
    if m.contains("4070 ti")           { return  7_680; }  // AD104:  60 SM × 128
    if m.contains("4070 super")        { return  7_168; }  // AD104:  56 SM × 128
    if m.contains("4070")              { return  5_888; }  // AD104:  46 SM × 128
    if m.contains("4060 ti")           { return  4_352; }  // AD106:  34 SM × 128
    if m.contains("4060")              { return  3_072; }  // AD107:  24 SM × 128
    if m.contains("4050")              { return  2_560; }  // AD107:  20 SM × 128 (mobile)

    // ── Hopper datacenter (H-series, sm_90) ───────────────────────────────────
    if m.contains("h200")              { return 16_896; }  // GH100: 132 SM × 128
    if m.contains("h100")              { return 16_896; }  // GH100: 132 SM × 128

    // ── Ampere datacenter ─────────────────────────────────────────────────────
    if m.contains("a100")              { return  6_912; }  // GA100:  108 SM × 64
    if m.contains("a10g")              { return  9_216; }  // GA102:   72 SM × 128
    if m.contains("a10")               { return  9_216; }  // GA102:   72 SM × 128

    // ── RTX 30xx — Ampere consumer (GA10x, sm_86) ─────────────────────────────
    if m.contains("3090 ti")           { return 10_752; }  // GA102:  84 SM × 128
    if m.contains("3090")              { return 10_496; }  // GA102:  82 SM × 128
    if m.contains("3080 ti")           { return 10_240; }  // GA102:  80 SM × 128
    if m.contains("3080 12gb")         { return  8_960; }  // GA102:  70 SM × 128
    if m.contains("3080")              { return  8_704; }  // GA102:  68 SM × 128
    if m.contains("3070 ti")           { return  6_144; }  // GA104:  48 SM × 128
    if m.contains("3070")              { return  5_888; }  // GA104:  46 SM × 128
    if m.contains("3060 ti")           { return  4_864; }  // GA104:  38 SM × 128
    if m.contains("3060")              { return  3_584; }  // GA106:  28 SM × 128
    if m.contains("3050 ti")           { return  2_560; }  // GA107:  20 SM × 128
    if m.contains("3050")              { return  2_048; }  // GA107:  16 SM × 128

    // Compute-cap fallback: return 0 so tier still resolves from VRAM
    let _ = compute_cap;
    0
}

/// Derive capability tier from VRAM + compute capability string.
///
/// Compute capability → architecture rough guide:
///   8.6  → Ampere consumer (RTX 30xx)
///   8.9  → Ada Lovelace (RTX 40xx)
///   10.0 → Blackwell datacenter (B100/B200) — always Training
///   12.0 → Blackwell consumer (RTX 50xx)
///   13.x → Rubin (RTX 60xx, future)
///   14.x → post-Rubin (RTX 70xx, future)
///
/// For all consumer cards we use VRAM to derive the tier — this keeps
/// future hardware working correctly even before core counts are known.
fn capability_from_nvidia(vram_gb: u32, compute_cap: &str) -> GpuCapability {
    // Blackwell / Hopper datacenter (sm_10x, sm_9x) — always Training grade
    if compute_cap.starts_with("10.") || compute_cap.starts_with("9.") {
        return GpuCapability::Training;
    }
    // All consumer architectures (Ampere, Ada, Blackwell, Rubin, post-Rubin):
    // use VRAM size to assign tier. Works for today and unknown future cards.
    GpuCapability::from_vram_gb(vram_gb)
}

fn current_platform() -> Platform {
    #[cfg(target_os = "linux")]   { Platform::Linux }
    #[cfg(target_os = "windows")] { Platform::Windows }
    #[cfg(target_os = "macos")]   { Platform::Macos }
}
