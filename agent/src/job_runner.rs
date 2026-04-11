//! Execute a NeuralMesh job inside a macOS sandbox.
//!
//! Flow:
//!   1. Download job bundle (tar.gz) from artifact store
//!   2. Create /tmp/neuralmesh/<job_id>/ working directory
//!   3. Generate sandbox-exec profile
//!   4. Acquire IOPMAssertion to prevent sleep
//!   5. Start restricted sshd for consumer access
//!   6. Run job via sandbox-exec as neuralmesh_worker user
//!   7. Stream stdout/stderr to coordinator
//!   8. Report completion + compute hash of output

use anyhow::{Context, Result};
use nm_common::JobSpec;
use nm_macos::{SandboxProfile, SleepAssertion};
use sha2::{Digest, Sha256};
use std::process::{Command, Stdio};
use std::time::Instant;
use tokio::io::{AsyncBufReadExt, BufReader};
use tracing::{error, info, warn};
use uuid::Uuid;

pub struct JobRunner;

#[derive(Debug)]
pub struct JobResult {
    pub job_id: Uuid,
    pub exit_code: i32,
    pub output_hash: String,
    pub actual_runtime_s: u64,
    pub avg_gpu_util_pct: f32,
    pub peak_ram_gb: u32,
}

impl JobRunner {
    /// Execute a job. Returns when the job completes or fails.
    pub async fn run(spec: &JobSpec, python_prefix: &str) -> Result<JobResult> {
        let job_id = spec.job_id.to_string();
        let start  = Instant::now();

        // 1. Download bundle
        info!(job_id, "Downloading job bundle");
        let bundle_path = download_bundle(&job_id, &spec.bundle_url, &spec.bundle_hash).await?;

        // 2. Generate sandbox profile
        let sandbox = SandboxProfile::new(&job_id, spec.runtime.as_str(), python_prefix)?;

        // 3. Extract bundle to work dir
        info!(job_id, work_dir = %sandbox.work_dir.display(), "Extracting job bundle");
        extract_bundle(&bundle_path, &sandbox.work_dir)?;

        // 4. Acquire sleep assertion (prevents Mac from sleeping during job)
        let _sleep_guard = SleepAssertion::acquire(&job_id)
            .unwrap_or_else(|e| {
                warn!(job_id, error = %e, "Could not acquire sleep assertion");
                // Create a no-op guard
                SleepAssertion::noop()
            });

        // 5. Find the entry point script
        let entry_script = find_entry_script(&sandbox.work_dir, &spec.runtime)?;
        info!(job_id, script = %entry_script, "Entry script found");

        // 6. Build sandbox-exec command
        let python_cmd = python_for_runtime(&spec.runtime);
        let mut cmd = Command::new("sudo");
        cmd.args([
            "-u", "neuralmesh_worker",
            "sandbox-exec",
            "-f", sandbox.profile_path.to_str().unwrap(),
            &python_cmd,
            &entry_script,
        ])
        .current_dir(&sandbox.work_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

        // Set resource limits via env
        cmd.env("NM_JOB_ID", &job_id)
           .env("NM_RAM_LIMIT_GB", spec.min_ram_gb.to_string())
           .env("PYTORCH_MPS_HIGH_WATERMARK_RATIO", "0.0"); // Unlimited MPS memory

        // Apply runtime-specific env
        match spec.runtime.as_str() {
            "mlx" => { cmd.env("MLX_USE_DEFAULT_DEVICE", "gpu"); }
            "torch-mps" => { cmd.env("PYTORCH_ENABLE_MPS_FALLBACK", "1"); }
            _ => {}
        }

        // 7. Run the job
        info!(job_id, "Starting job process");
        let mut child = cmd.spawn().context("Failed to spawn job process")?;

        // Collect output and compute hash
        let mut hasher = Sha256::new();
        let mut gpu_utils: Vec<f32> = Vec::new();

        // Stream stdout
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let log_dir = sandbox.work_dir.join("nm-output.log");
        let mut log_file = std::fs::File::create(&log_dir)?;

        // Simplified: just wait for completion (streaming done separately via SSH)
        let exit_status = child.wait().context("Waiting for job process")?;
        let exit_code = exit_status.code().unwrap_or(-1);

        // Hash the output log
        if log_dir.exists() {
            let content = std::fs::read(&log_dir).unwrap_or_default();
            hasher.update(&content);
        }
        let output_hash = hex::encode(hasher.finalize());
        let actual_runtime_s = start.elapsed().as_secs();

        info!(
            job_id,
            exit_code,
            runtime_s = actual_runtime_s,
            output_hash = %output_hash,
            "Job complete"
        );

        // 8. Cleanup
        if let Err(e) = sandbox.cleanup() {
            warn!(job_id, error = %e, "Sandbox cleanup error");
        }
        let _ = std::fs::remove_file(&bundle_path);

        Ok(JobResult {
            job_id: spec.job_id,
            exit_code,
            output_hash,
            actual_runtime_s,
            avg_gpu_util_pct: 0.0, // TODO: average from polls
            peak_ram_gb: 0,        // TODO: track via periodic sampling
        })
    }
}

async fn download_bundle(job_id: &str, url: &str, expected_hash: &str) -> Result<String> {
    let dest = format!("/tmp/neuralmesh/bundle-{}.tar.gz", job_id);
    std::fs::create_dir_all("/tmp/neuralmesh")?;

    // Use curl for download (available on all Macs)
    let status = Command::new("curl")
        .args([
            "-fsSL",
            "--max-time", "300",
            "-o", &dest,
            url,
        ])
        .status()
        .context("curl download")?;

    if !status.success() {
        anyhow::bail!("Failed to download job bundle from {}", url);
    }

    // Verify SHA-256 hash
    let content = std::fs::read(&dest)?;
    let mut hasher = Sha256::new();
    hasher.update(&content);
    let actual_hash = hex::encode(hasher.finalize());

    if actual_hash != expected_hash {
        let _ = std::fs::remove_file(&dest);
        anyhow::bail!(
            "Bundle hash mismatch: expected {}, got {}",
            expected_hash, actual_hash
        );
    }

    Ok(dest)
}

fn extract_bundle(bundle_path: &str, work_dir: &std::path::Path) -> Result<()> {
    let status = Command::new("tar")
        .args(["-xzf", bundle_path, "-C", work_dir.to_str().unwrap()])
        .status()
        .context("tar extraction")?;

    if !status.success() {
        anyhow::bail!("Failed to extract bundle");
    }
    Ok(())
}

fn find_entry_script(work_dir: &std::path::Path, runtime: &nm_common::Runtime) -> Result<String> {
    // Look for main.py, inference.py, train.py, run.py, script.py in order
    let candidates = match runtime {
        nm_common::Runtime::Shell => vec!["run.sh", "main.sh", "start.sh"],
        _ => vec!["main.py", "inference.py", "train.py", "run.py", "script.py"],
    };

    for name in candidates {
        let path = work_dir.join(name);
        if path.exists() {
            return Ok(path.to_string_lossy().to_string());
        }
    }

    // Fall back: find any .py file at root
    for entry in std::fs::read_dir(work_dir)? {
        let entry = entry?;
        let path  = entry.path();
        if path.extension().map(|e| e == "py").unwrap_or(false) {
            return Ok(path.to_string_lossy().to_string());
        }
    }

    anyhow::bail!("No entry script found in job bundle")
}

fn python_for_runtime(runtime: &nm_common::Runtime) -> String {
    // Use the system Python3 — runtimes installed via pip3 into system Python
    "python3".to_string()
}
