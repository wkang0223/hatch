//! Execute a Hatch job — VM, DMTCP, or sandbox-exec fallback.
//!
//! Execution strategy (tried in order):
//!
//!   1. **Virtualization.framework VM** (macOS 13+ with nm-vm-helper installed)
//!      - True OS isolation: separate kernel, PID/network namespaces
//!      - virtio-fs job directory mounted at /job inside the VM
//!      - Requires: `nm-vm-helper` binary + base Linux ARM64 image
//!
//!   2. **sandbox-exec + DMTCP** (Linux / VM guest with dmtcp_launch)
//!      - Binary-level checkpoint/restore — job can migrate across providers
//!      - Checkpoint every 5 minutes, stored at /var/hatch/checkpoints/<job_id>/
//!      - Coordinator heartbeat watcher re-queues jobs with checkpoint on disconnect
//!
//!   3. **sandbox-exec** (macOS sandbox, no checkpointing)
//!      - macOS App Sandbox via sandbox-exec(1)
//!      - filesystem/network restricted to job working directory
//!      - No migration on disconnect — coordinator refunds consumer on failure

use crate::checkpoint::{
    is_dmtcp_available, spawn_checkpoint_ticker, CheckpointMeta, DmtcpSession,
    CHECKPOINT_DIR, CHECKPOINT_INTERVAL_SECS,
};
use crate::idle_monitor::JobSpec;
use anyhow::{Context, Result};

#[cfg(target_os = "macos")]
use hatch_macos::{SandboxProfile, SleepAssertion};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::time::{interval, Duration};
use tracing::{info, warn};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct JobResult {
    pub job_id: Uuid,
    pub exit_code: i32,
    pub output_hash: String,
    pub output_log: String,   // stdout+stderr content to upload to coordinator
    pub actual_runtime_s: u64,
    pub avg_gpu_util_pct: f32,
    pub peak_ram_gb: u32,
}

pub struct JobRunner;

impl JobRunner {
    /// Execute a job end-to-end. Returns when the job completes or fails.
    pub async fn run(
        spec: &JobSpec,
        python_prefix: &str,
        rest_base: &str,
        provider_id: &str,
    ) -> Result<JobResult> {
        let job_id = spec.job_id.to_string();
        let start  = Instant::now();

        // 1. Validate we have something to run
        if spec.bundle_url.is_empty() && spec.checkpoint_url.is_none() {
            anyhow::bail!("Job has neither bundle_url nor checkpoint_url — cannot execute");
        }

        // 2. Prevent system sleep for the duration of the job (macOS only)
        #[cfg(target_os = "macos")]
        let _sleep_guard = {
            use hatch_macos::SleepAssertion;
            SleepAssertion::acquire(&job_id).unwrap_or_else(|e| {
                warn!(job_id, error = %e, "Could not acquire sleep assertion");
                SleepAssertion::noop()
            })
        };

        // 3. Prepare working directory
        let work_dir = PathBuf::from(format!("/tmp/hatch/{}", job_id));
        std::fs::create_dir_all(&work_dir)?;

        // 4a. If restoring from checkpoint — DMTCP restore (macOS only for now)
        #[cfg(not(target_os = "linux"))]
        if let Some(ref ckpt_url) = spec.checkpoint_url {
            info!(job_id, checkpoint_url = %ckpt_url, "Restoring job from DMTCP checkpoint");
            return Self::run_restore(spec, &work_dir, start, rest_base, provider_id).await;
        }

        // 4b. Fresh start — download and extract bundle
        info!(job_id, url = %spec.bundle_url, "Downloading job bundle");
        let bundle_path = download_bundle(&job_id, &spec.bundle_url, &spec.bundle_hash).await?;

        info!(job_id, "Extracting bundle");
        extract_bundle(&bundle_path, &work_dir)?;
        let _ = std::fs::remove_file(&bundle_path);

        // 5. Find entry script — prefer coordinator-supplied name; fall back to heuristic scan
        let entry_script = if let Some(ref name) = spec.script_name {
            let explicit = work_dir.join(name);
            if explicit.exists() {
                explicit.to_string_lossy().to_string()
            } else {
                warn!(job_id, script_name = %name, "Coordinator-specified script not found; scanning bundle");
                find_entry_script(&work_dir, &spec.runtime)
                    .context("No runnable entry script found in bundle")?
            }
        } else {
            find_entry_script(&work_dir, &spec.runtime)
                .context("No runnable entry script found in bundle")?
        };
        info!(job_id, script = %entry_script, "Entry script resolved");

        // 6. Try VM execution path (macOS 13+ only)
        #[cfg(target_os = "macos")]
        {
            if hatch_macos::virt::VmConfig::is_available() {
                info!(job_id, "Virtualization.framework available — running in isolated Linux VM");
                let vm_cfg = hatch_macos::virt::VmConfig::for_job(
                    &job_id,
                    work_dir.clone(),
                    spec.min_ram_gb,
                );
                match hatch_macos::virt::run_in_vm(&vm_cfg, &entry_script).await {
                    Ok(vm_result) => {
                        let mut h = Sha256::new();
                        h.update(vm_result.stdout.as_bytes());
                        h.update(vm_result.stderr.as_bytes());
                        let output_hash = hex::encode(h.finalize());
                        let actual_runtime_s = start.elapsed().as_secs();

                        // Cleanup work dir
                        let _ = std::fs::remove_dir_all(&work_dir);

                        if vm_result.exit_code != 0 {
                            anyhow::bail!("VM job exited with code {}", vm_result.exit_code);
                        }

                        info!(
                            job_id,
                            exit_code = vm_result.exit_code,
                            runtime_s = actual_runtime_s,
                            "VM job completed"
                        );
                        let output_log = format!("{}{}", vm_result.stdout, vm_result.stderr);
                        return Ok(JobResult {
                            job_id: spec.job_id,
                            exit_code: vm_result.exit_code,
                            output_hash,
                            output_log,
                            actual_runtime_s,
                            avg_gpu_util_pct: 0.0, // GPU stats inside VM not yet available
                            peak_ram_gb: spec.min_ram_gb,
                        });
                    }
                    Err(e) => {
                        warn!(job_id, error = %e, "VM execution failed — falling back to sandbox-exec");
                    }
                }
            }
        }
        // Non-zero VM exit returns Ok(JobResult) so idle_monitor can upload the log.

        // 7. Platform sandbox path
        #[cfg(target_os = "linux")]
        {
            return Self::run_linux_sandboxed(spec, &work_dir, &entry_script, python_prefix, start).await;
        }
        #[cfg(not(target_os = "linux"))]
        Self::run_sandboxed(spec, &work_dir, &entry_script, python_prefix, start, rest_base, provider_id).await
    }

    /// Linux sandbox — cgroups v2 + optional bwrap.
    #[cfg(target_os = "linux")]
    async fn run_linux_sandboxed(
        spec: &JobSpec,
        work_dir: &PathBuf,
        entry_script: &str,
        python_prefix: &str,
        start: Instant,
    ) -> Result<JobResult> {
        use hatch_linux::sandbox::LinuxSandbox;
        use std::process::Stdio;

        let job_id  = spec.job_id.to_string();
        let sandbox = LinuxSandbox::new(&job_id, spec.min_ram_gb);
        let python_bin = format!("{}/bin/python3", python_prefix);

        let log_path = work_dir.join("nm-output.log");
        let log_out  = std::fs::File::create(&log_path)?;
        let log_err  = log_out.try_clone()?;

        // GPU stats sampler
        let gpu_samples: std::sync::Arc<std::sync::Mutex<Vec<f32>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let gpu_writer = gpu_samples.clone();
        let sampler = tokio::spawn(async move {
            let mut tick = tokio::time::interval(tokio::time::Duration::from_secs(10));
            loop {
                tick.tick().await;
                if let Some(s) = hatch_linux::sample_gpu_utilization() {
                    gpu_writer.lock().unwrap().push(s.utilization_pct);
                }
            }
        });

        let mut cmd = sandbox.wrap_command(&python_bin, &[entry_script]);
        cmd.current_dir(work_dir)
            .stdout(Stdio::from(log_out))
            .stderr(Stdio::from(log_err))
            .env("HATCH_JOB_ID", &job_id)
            .env("HATCH_RAM_LIMIT_GB", spec.min_ram_gb.to_string())
            .env("PYTORCH_MPS_HIGH_WATERMARK_RATIO", "0.0")
            .env("TMPDIR", work_dir.to_str().unwrap_or("/tmp"));

        // ROCm device visibility (AMD)
        cmd.env("HSA_OVERRIDE_GFX_VERSION", "")
            .env("ROCR_VISIBLE_DEVICES", "0");
        // CUDA device visibility (NVIDIA)
        cmd.env("CUDA_VISIBLE_DEVICES", "0");

        for (k, v) in &spec.env_vars { cmd.env(k, v); }

        info!(job_id, "Spawning job (Linux cgroups sandbox)");
        let mut child = cmd.spawn().context("Failed to spawn job process")?;

        // Assign child to cgroup immediately after spawn
        if let Some(pid) = child.id() { sandbox.assign_pid(pid); }

        let status = tokio::task::spawn_blocking(move || child.wait()).await??;
        let exit_code = status.code().unwrap_or(-1);

        sampler.abort();

        let log_bytes = std::fs::read(&log_path).unwrap_or_default();
        let output_log = String::from_utf8_lossy(&log_bytes).into_owned();
        let mut hasher = sha2::Sha256::new();
        sha2::Digest::update(&mut hasher, &log_bytes);
        let output_hash = hex::encode(sha2::Digest::finalize(hasher));

        let samples = gpu_samples.lock().unwrap();
        let avg_gpu = if samples.is_empty() { 0.0 }
                      else { samples.iter().sum::<f32>() / samples.len() as f32 };
        let peak_ram_gb = hatch_linux::sample_gpu_utilization()
            .map(|s| (s.memory_used_mb / 1024) as u32)
            .unwrap_or(0);

        sandbox.cleanup();

        info!(job_id, exit_code, runtime_s = start.elapsed().as_secs(), "Job process finished");
        Ok(JobResult {
            job_id: spec.job_id,
            exit_code,
            output_hash,
            output_log,
            actual_runtime_s: start.elapsed().as_secs(),
            avg_gpu_util_pct: avg_gpu,
            peak_ram_gb,
        })
    }

    /// macOS sandbox-exec path (with optional DMTCP).
    #[cfg(not(target_os = "linux"))]
    async fn run_sandboxed(
        spec: &JobSpec,
        work_dir: &PathBuf,
        entry_script: &str,
        python_prefix: &str,
        start: Instant,
        rest_base: &str,
        provider_id: &str,
    ) -> Result<JobResult> {
        let job_id = spec.job_id.to_string();

        // Create sandbox profile (filesystem + network restrictions)
        let sandbox = SandboxProfile::new(&job_id, spec.runtime.as_str(), python_prefix)?;

        // Resolve python interpreter
        let python_bin = resolve_python(python_prefix, &spec.runtime);

        // GPU utilization sampler
        let gpu_samples: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
        let gpu_writer = gpu_samples.clone();
        let sampler_handle = tokio::spawn(async move {
            let mut tick = interval(Duration::from_secs(10));
            loop {
                tick.tick().await;
                if let Some(stats) = hatch_macos::gpu_detect::sample_gpu_utilization() {
                    gpu_writer.lock().unwrap().push(stats.utilization_pct);
                }
            }
        });

        let exit_code = if is_dmtcp_available() {
            info!(job_id, "DMTCP available — launching with checkpoint support");
            Self::run_with_dmtcp(spec, work_dir, entry_script, python_bin, &sandbox, rest_base, provider_id).await?
        } else {
            Self::run_direct(spec, work_dir, entry_script, &python_bin, &sandbox).await?
        };

        // Stop GPU sampler
        sampler_handle.abort();

        // Read log content BEFORE cleanup — this is the stdout+stderr of the job.
        // We compute the hash and capture the text here; both go to the coordinator
        // so consumers can view job output via `hatch job logs`.
        let log_path = sandbox.work_dir.join("nm-output.log");
        let log_bytes = if log_path.exists() {
            std::fs::read(&log_path).unwrap_or_default()
        } else {
            Vec::new()
        };
        let output_log = String::from_utf8_lossy(&log_bytes).into_owned();
        let mut hasher = Sha256::new();
        hasher.update(&log_bytes);
        let output_hash = hex::encode(hasher.finalize());

        // GPU summary
        let samples = gpu_samples.lock().unwrap();
        let avg_gpu = if samples.is_empty() {
            0.0
        } else {
            samples.iter().sum::<f32>() / samples.len() as f32
        };
        let peak_ram_gb = hatch_macos::gpu_detect::sample_gpu_utilization()
            .map(|s| (s.memory_used_mb / 1024) as u32)
            .unwrap_or(0);

        let actual_runtime_s = start.elapsed().as_secs();

        // Clean up DMTCP checkpoint dir if job completed successfully
        if exit_code == 0 {
            crate::checkpoint::DmtcpSession::cleanup(&job_id);
        }

        // Cleanup sandbox (deletes work dir and profile)
        if let Err(e) = sandbox.cleanup() {
            warn!(job_id, error = %e, "Sandbox cleanup error (non-fatal)");
        }

        info!(
            job_id,
            exit_code,
            runtime_s = actual_runtime_s,
            avg_gpu_pct = avg_gpu,
            "Job process finished"
        );

        // Always return Ok — idle_monitor checks exit_code to route to
        // report_completion (0) or report_failure (non-zero).  Bailing here
        // would drop output_log before it can be uploaded to the coordinator.
        Ok(JobResult {
            job_id: spec.job_id,
            exit_code,
            output_hash,
            output_log,
            actual_runtime_s,
            avg_gpu_util_pct: avg_gpu,
            peak_ram_gb,
        })
    }

    /// Direct sandbox-exec execution (no DMTCP). macOS only.
    #[cfg(not(target_os = "linux"))]
    async fn run_direct(
        spec: &JobSpec,
        work_dir: &PathBuf,
        entry_script: &str,
        python_bin: &str,
        sandbox: &SandboxProfile,
    ) -> Result<i32> {
        let job_id = spec.job_id.to_string();

        // Capture stdout + stderr to log file so the coordinator can serve them.
        let log_path = work_dir.join("nm-output.log");
        let log_out = std::fs::File::create(&log_path)
            .with_context(|| format!("Creating log file at {}", log_path.display()))?;
        let log_err = log_out.try_clone().context("Cloning log file handle")?;

        let mut cmd = Command::new("sandbox-exec");
        cmd.args(["-f", sandbox.profile_path.to_str().unwrap()])
            .arg(python_bin)
            .arg(entry_script)
            .current_dir(work_dir)
            .stdout(Stdio::from(log_out))
            .stderr(Stdio::from(log_err))
            .env("HATCH_JOB_ID", &job_id)
            .env("HATCH_RAM_LIMIT_GB", spec.min_ram_gb.to_string())
            .env("PYTORCH_MPS_HIGH_WATERMARK_RATIO", "0.0")
            // Direct Python tempfile.* into the job's sandbox directory so that
            // cross-job /tmp writes are blocked by the sandbox profile above.
            .env("TMPDIR", work_dir.to_str().unwrap_or("/tmp"));

        for (k, v) in &spec.env_vars {
            cmd.env(k, v);
        }
        apply_runtime_env(&mut cmd, &spec.runtime);

        info!(job_id, "Spawning job (sandbox-exec, no DMTCP)");
        let mut child = cmd.spawn().context("Failed to spawn sandbox-exec")?;

        let status = tokio::task::spawn_blocking(move || child.wait()).await??;
        Ok(status.code().unwrap_or(-1))
    }

    /// DMTCP-wrapped sandbox-exec execution with periodic checkpointing. macOS only.
    #[cfg(not(target_os = "linux"))]
    async fn run_with_dmtcp(
        spec: &JobSpec,
        work_dir: &PathBuf,
        entry_script: &str,
        python_bin: String,
        sandbox: &SandboxProfile,
        rest_base: &str,
        provider_id: &str,
    ) -> Result<i32> {
        let job_id = spec.job_id.to_string();

        let env_vars: Vec<(String, String)> = {
            let mut ev = vec![
                ("HATCH_JOB_ID".to_string(), job_id.clone()),
                ("HATCH_RAM_LIMIT_GB".to_string(), spec.min_ram_gb.to_string()),
                ("PYTORCH_MPS_HIGH_WATERMARK_RATIO".to_string(), "0.0".to_string()),
            ];
            ev.extend(spec.env_vars.clone());
            ev
        };

        // Log file — stdout + stderr of the sandboxed job go here.
        let log_path = work_dir.join("nm-output.log");

        // Launch sandbox-exec wrapped in dmtcp_launch
        let sandbox_args: Vec<&str> = vec![
            "-f",
            sandbox.profile_path.to_str().unwrap(),
            &python_bin,
            entry_script,
        ];

        let (mut session, mut child) = DmtcpSession::launch(
            &job_id,
            "sandbox-exec",
            &sandbox_args,
            work_dir,
            &env_vars,
            Some(&log_path),
        )?;

        // Spawn checkpoint ticker — reports each snapshot to the coordinator.
        let ckpt_dir = PathBuf::from(CHECKPOINT_DIR).join(&job_id);
        let ticker = spawn_checkpoint_ticker(
            job_id.clone(),
            session.coord_port,
            ckpt_dir,
            CHECKPOINT_INTERVAL_SECS,
            rest_base.to_string(),
            provider_id.to_string(),
        );

        // Wait for process
        let status = tokio::task::spawn_blocking(move || child.wait()).await??;

        // Stop ticker
        ticker.abort();

        Ok(status.code().unwrap_or(-1))
    }

    /// Restore a previously checkpointed job. macOS only (DMTCP not available on Linux).
    #[cfg(not(target_os = "linux"))]
    async fn run_restore(
        spec: &JobSpec,
        work_dir: &PathBuf,
        start: Instant,
        rest_base: &str,
        provider_id: &str,
    ) -> Result<JobResult> {
        let job_id = spec.job_id.to_string();

        if !is_dmtcp_available() {
            anyhow::bail!(
                "Job {} needs DMTCP restore but dmtcp_launch is not available on this machine",
                job_id
            );
        }

        // Fetch checkpoint metadata from coordinator — the canonical source of truth.
        // This works even when restoring on a different provider than the one that saved it.
        let meta = fetch_checkpoint_meta(rest_base, &job_id)
            .await
            .context("Could not fetch checkpoint metadata from coordinator")?;

        info!(
            job_id,
            iteration = meta.iteration,
            elapsed_secs = meta.elapsed_secs,
            "Restoring job from DMTCP checkpoint"
        );

        // Log file — restored job output goes here too.
        let log_path = work_dir.join("nm-output.log");

        let (mut child, effective_port) = DmtcpSession::restore(&meta, work_dir, Some(&log_path))?;

        // Spawn a fresh checkpoint ticker for the restored session.
        let ckpt_dir = PathBuf::from(CHECKPOINT_DIR).join(&job_id);
        let ticker = spawn_checkpoint_ticker(
            job_id.clone(),
            effective_port,
            ckpt_dir,
            CHECKPOINT_INTERVAL_SECS,
            rest_base.to_string(),
            provider_id.to_string(),
        );

        let status = tokio::task::spawn_blocking(move || child.wait()).await??;
        ticker.abort();

        let exit_code = status.code().unwrap_or(-1);
        let actual_runtime_s = start.elapsed().as_secs() + meta.elapsed_secs;

        if exit_code == 0 {
            DmtcpSession::cleanup(&job_id);
        }

        // Read log BEFORE cleanup so coordinator can serve it.
        let log_bytes = if log_path.exists() {
            std::fs::read(&log_path).unwrap_or_default()
        } else {
            Vec::new()
        };
        let output_log = String::from_utf8_lossy(&log_bytes).into_owned();
        let mut hasher = Sha256::new();
        hasher.update(&log_bytes);
        let output_hash = hex::encode(hasher.finalize());

        info!(job_id, exit_code, runtime_s = actual_runtime_s, "Restored job completed");

        Ok(JobResult {
            job_id: spec.job_id,
            exit_code,
            output_hash,
            output_log,
            actual_runtime_s,
            avg_gpu_util_pct: 0.0,
            peak_ram_gb: spec.min_ram_gb,
        })
    }
}

// ── Checkpoint metadata fetch ────────────────────────────────────────────────

/// Fetch the latest checkpoint metadata for `job_id` from the coordinator.
/// This replaces the old `CheckpointMeta::load()` which read from local disk —
/// that broke cross-provider restore because the files lived on Provider A.
async fn fetch_checkpoint_meta(rest_base: &str, job_id: &str) -> Result<crate::checkpoint::CheckpointMeta> {
    let url = format!("{}/api/v1/jobs/{}/checkpoint", rest_base, job_id);
    let resp = reqwest::Client::new()
        .get(&url)
        .send()
        .await
        .context("GET checkpoint metadata from coordinator")?;

    if !resp.status().is_success() {
        anyhow::bail!(
            "Coordinator returned {} for checkpoint metadata of job {}",
            resp.status(),
            job_id
        );
    }

    resp.json::<crate::checkpoint::CheckpointMeta>()
        .await
        .context("Parsing checkpoint metadata JSON")
}

// ── Download + verify ────────────────────────────────────────────────────────

async fn download_bundle(job_id: &str, url: &str, expected_hash: &str) -> Result<String> {
    let dest = format!("/tmp/hatch/bundle-{}.tar.gz", job_id);
    std::fs::create_dir_all("/tmp/hatch")?;

    let status = Command::new("curl")
        .args(["-fsSL", "--max-time", "300", "--max-filesize", "524288000", "-o", &dest, url])
        .status()
        .context("curl download")?;

    if !status.success() {
        anyhow::bail!("curl failed downloading bundle from {}", url);
    }

    // Only verify if expected_hash looks like a SHA-256 (64 hex chars).
    // The coordinator currently stores MD5 (32 chars) as the dedup key;
    // skip integrity check for those until the coordinator emits SHA-256.
    if expected_hash.len() == 64 {
        let content = std::fs::read(&dest)?;
        let mut h = Sha256::new();
        h.update(&content);
        let actual = hex::encode(h.finalize());
        if actual != expected_hash {
            let _ = std::fs::remove_file(&dest);
            anyhow::bail!(
                "Bundle hash mismatch: expected {}, got {}",
                expected_hash,
                actual
            );
        }
    }

    Ok(dest)
}

fn extract_bundle(bundle_path: &str, work_dir: &PathBuf) -> Result<()> {
    let status = Command::new("tar")
        .args(["-xzf", bundle_path, "-C", work_dir.to_str().unwrap()])
        .status()
        .context("tar extraction")?;

    if !status.success() {
        anyhow::bail!("tar failed extracting bundle");
    }
    Ok(())
}

// ── Entry script resolution ──────────────────────────────────────────────────

fn find_entry_script(work_dir: &PathBuf, runtime: &hatch_common::Runtime) -> Result<String> {
    let candidates: &[&str] = match runtime {
        hatch_common::Runtime::Shell => &["run.sh", "main.sh", "start.sh"],
        _ => &["main.py", "inference.py", "train.py", "run.py", "script.py"],
    };

    for name in candidates {
        let p = work_dir.join(name);
        if p.exists() {
            return Ok(p.to_string_lossy().to_string());
        }
    }

    // Fallback: any .py at root — sorted so the result is deterministic across filesystems.
    let mut entries: Vec<_> = std::fs::read_dir(work_dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|e| e == "py").unwrap_or(false))
        .collect();
    entries.sort();
    if let Some(path) = entries.into_iter().next() {
        return Ok(path.to_string_lossy().to_string());
    }

    anyhow::bail!("No entry script found in bundle (tried: {:?})", candidates)
}

// ── Python interpreter resolution ────────────────────────────────────────────

fn resolve_python(prefix: &str, _runtime: &hatch_common::Runtime) -> String {
    let versioned = [
        format!("{}/bin/python3.12", prefix),
        format!("{}/bin/python3.13", prefix),
        format!("{}/bin/python3.11", prefix),
        format!("{}/bin/python3", prefix),
    ];

    for bin in &versioned {
        if std::path::Path::new(bin).exists() {
            return bin.clone();
        }
    }

    for py in &["python3.12", "python3.13", "python3.11", "python3"] {
        if Command::new("which")
            .arg(py)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            return py.to_string();
        }
    }

    "python3".to_string()
}

// ── Runtime-specific environment variables ───────────────────────────────────

fn apply_runtime_env(cmd: &mut Command, runtime: &hatch_common::Runtime) {
    match runtime {
        hatch_common::Runtime::Mlx => {
            cmd.env("MLX_USE_DEFAULT_DEVICE", "gpu");
        }
        hatch_common::Runtime::TorchMps => {
            cmd.env("PYTORCH_ENABLE_MPS_FALLBACK", "1");
        }
        _ => {}
    }
}
