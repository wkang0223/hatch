pub mod apple;
pub mod nvidia;
pub mod amd;
pub mod intel_arc;

use crate::types::GpuInfo;
use anyhow::Result;

/// Each backend implements this trait.
pub trait GpuBackend: Send + Sync {
    /// Enumerate all GPUs this backend can see.
    fn enumerate(&self) -> Result<Vec<GpuInfo>>;
    /// Check if this backend is available on the current host.
    fn is_available(&self) -> bool;
    /// Backend name for logging.
    fn name(&self) -> &'static str;
}
