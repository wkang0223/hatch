//! Prevent macOS from sleeping while a NeuralMesh job is running.
//! Uses `caffeinate` as a subprocess — works without any special entitlements.

use anyhow::Result;
use std::process::{Child, Command};
use tracing::{info, warn};

/// Holds a `caffeinate` subprocess that prevents system sleep.
/// Dropped automatically when the job ends.
pub struct SleepAssertion {
    child: Option<Child>,
    job_id: String,
}

impl SleepAssertion {
    /// No-op assertion — used when sleep prevention is not needed (e.g., tests).
    pub fn noop() -> Self {
        Self {
            child: None,
            job_id: String::new(),
        }
    }

    /// Start preventing sleep for the given job.
    pub fn acquire(job_id: &str) -> Result<Self> {
        // -i: prevent idle sleep
        // -s: prevent system sleep (requires AC power on laptops)
        // -d: prevent display sleep
        let child = Command::new("caffeinate")
            .args(["-i", "-s"])
            .spawn()?;

        info!(job_id, "Sleep assertion acquired (caffeinate running)");
        Ok(Self {
            child: Some(child),
            job_id: job_id.to_string(),
        })
    }

    /// Explicitly release the assertion (also released on drop).
    pub fn release(&mut self) {
        if let Some(mut child) = self.child.take() {
            if let Err(e) = child.kill() {
                warn!(job_id = %self.job_id, error = %e, "Failed to kill caffeinate");
            } else {
                info!(job_id = %self.job_id, "Sleep assertion released");
            }
        }
    }
}

impl Drop for SleepAssertion {
    fn drop(&mut self) {
        self.release();
    }
}
