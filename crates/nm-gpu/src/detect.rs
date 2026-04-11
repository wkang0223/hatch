//! Platform-level GPU detection — dispatches to all available backends.

use crate::backends::{GpuBackend, apple::AppleBackend, nvidia::NvidiaBackend,
                      amd::AmdBackend, intel_arc::IntelArcBackend};
use crate::types::GpuInfo;
use anyhow::Result;
use tracing::{debug, warn};

/// Detect all GPUs on this host across all vendors.
/// Returns empty vec on failure (non-fatal — logs warnings).
pub fn detect_gpus() -> Vec<GpuInfo> {
    let backends: Vec<Box<dyn GpuBackend>> = vec![
        Box::new(AppleBackend),
        Box::new(NvidiaBackend),
        Box::new(AmdBackend),
        Box::new(IntelArcBackend),
    ];

    let mut all = Vec::new();
    for backend in &backends {
        if !backend.is_available() {
            debug!("GPU backend {} not available on this host", backend.name());
            continue;
        }
        match backend.enumerate() {
            Ok(gpus) => {
                debug!("Backend {} found {} GPU(s)", backend.name(), gpus.len());
                all.extend(gpus);
            }
            Err(e) => {
                warn!("GPU backend {} enumeration failed: {}", backend.name(), e);
            }
        }
    }
    all
}

/// Detect the primary (highest-capability) GPU.
pub fn detect_primary_gpu() -> Option<GpuInfo> {
    let mut gpus = detect_gpus();
    gpus.sort_by(|a, b| b.capability.cmp(&a.capability));
    gpus.into_iter().next()
}
