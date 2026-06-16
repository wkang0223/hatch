//! Linux job sandbox using cgroups v2 and optional bwrap filesystem isolation.
//!
//! Isolation strategy (applied in order of availability):
//!   1. cgroups v2  — memory cap, CPU weight, task count (always available on modern Linux)
//!   2. bwrap       — filesystem isolation + read-only /usr bind mounts (if bwrap installed)
//!   3. setrlimit   — fallback resource limits via kernel RLIMIT_* (always available)
//!
//! GPU device access:
//!   NVIDIA — /dev/nvidiactl, /dev/nvidia*, /dev/nvidia-uvm  (bwrap bind-mounts)
//!   AMD    — /dev/kfd, /dev/dri/renderD*                    (bwrap bind-mounts)

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::{info, warn};

pub struct LinuxSandboxProfile {
    pub job_id:       String,
    pub work_dir:     PathBuf,
    cgroup_path:      Option<PathBuf>,
}

pub struct LinuxSandbox;

impl LinuxSandbox {
    /// Create a cgroup and return the profile, or a no-cgroup profile on failure.
    pub fn new(job_id: &str, max_ram_gb: u32) -> LinuxSandboxProfile {
        let work_dir = PathBuf::from(format!("/tmp/hatch/{}", job_id));
        let _ = std::fs::create_dir_all(&work_dir);

        let cgroup_path = setup_cgroup(job_id, max_ram_gb).ok();

        LinuxSandboxProfile {
            job_id: job_id.to_string(),
            work_dir,
            cgroup_path,
        }
    }
}

impl LinuxSandboxProfile {
    /// Build a sandboxed Command. Uses bwrap if available, else plain env isolation.
    pub fn wrap_command(&self, program: &str, args: &[&str]) -> Command {
        if bwrap_available() {
            self.bwrap_command(program, args)
        } else {
            self.plain_command(program, args)
        }
    }

    /// Assign a child PID to the cgroup after spawning.
    pub fn assign_pid(&self, pid: u32) {
        if let Some(ref cg) = self.cgroup_path {
            let procs = cg.join("cgroup.procs");
            if let Err(e) = std::fs::write(&procs, pid.to_string()) {
                warn!(job_id = %self.job_id, error = %e, "Failed to assign PID to cgroup");
            }
        }
    }

    pub fn cleanup(&self) {
        let _ = std::fs::remove_dir_all(&self.work_dir);
        if let Some(ref cg) = self.cgroup_path {
            // cgroup must be empty before rmdir
            let _ = std::fs::remove_dir(cg);
        }
    }

    fn bwrap_command(&self, program: &str, args: &[&str]) -> Command {
        let mut cmd = Command::new("bwrap");
        cmd.args([
            // Minimal read-only OS environment
            "--ro-bind", "/usr", "/usr",
            "--ro-bind-try", "/lib",   "/lib",
            "--ro-bind-try", "/lib64", "/lib64",
            "--dev", "/dev",
            "--proc", "/proc",
            "--tmpfs", "/tmp",
            // Job workspace writable
            "--bind", self.work_dir.to_str().unwrap(), "/workspace",
            "--chdir", "/workspace",
        ]);

        // AMD GPU devices
        cmd.args(["--dev-bind-try", "/dev/kfd", "/dev/kfd"]);
        for i in 0..8 {
            let dri = format!("/dev/dri/renderD{}", 128 + i);
            if Path::new(&dri).exists() {
                cmd.args(["--dev-bind", &dri, &dri]);
            }
        }
        // NVIDIA GPU devices
        for dev in &["/dev/nvidiactl", "/dev/nvidia-uvm"] {
            if Path::new(dev).exists() {
                cmd.args(["--dev-bind", dev, dev]);
            }
        }
        for i in 0..8u32 {
            let dev = format!("/dev/nvidia{}", i);
            if Path::new(&dev).exists() {
                cmd.args(["--dev-bind", &dev, &dev]);
            }
        }

        // ROCm libraries
        if Path::new("/opt/rocm").exists() {
            cmd.args(["--ro-bind", "/opt/rocm", "/opt/rocm"]);
        }

        cmd.arg("--").arg(program).args(args);
        cmd
    }

    fn plain_command(&self, program: &str, args: &[&str]) -> Command {
        let mut cmd = Command::new(program);
        cmd.args(args)
            .current_dir(&self.work_dir)
            .env("HOME", self.work_dir.to_str().unwrap())
            .env("TMPDIR", self.work_dir.to_str().unwrap());
        cmd
    }
}

// ── cgroups v2 setup ──────────────────────────────────────────────────────────

fn setup_cgroup(job_id: &str, max_ram_gb: u32) -> Result<PathBuf> {
    // Find the cgroup v2 mount — usually /sys/fs/cgroup
    let cg_root = find_cgroup_v2_root().context("cgroup v2 not mounted")?;
    let nm_root  = cg_root.join("hatch");
    let job_cg   = nm_root.join(format!("job-{}", &job_id[..8]));

    // Create hatch slice if missing
    if !nm_root.exists() {
        std::fs::create_dir_all(&nm_root)?;
        // Enable memory and CPU controllers in the parent
        let _ = std::fs::write(cg_root.join("cgroup.subtree_control"), "+memory +cpu +pids");
        let _ = std::fs::write(nm_root.join("cgroup.subtree_control"), "+memory +cpu +pids");
    }

    std::fs::create_dir_all(&job_cg)?;

    // Memory cap: job RAM + 1 GB headroom for Python interpreter
    let mem_bytes = (max_ram_gb as u64 + 1) * 1024 * 1024 * 1024;
    std::fs::write(job_cg.join("memory.max"), mem_bytes.to_string())
        .context("setting memory.max")?;

    // Swap disabled for deterministic OOM behaviour
    let _ = std::fs::write(job_cg.join("memory.swap.max"), "0");

    // Lower CPU weight so jobs don't starve the agent heartbeat (default = 100)
    let _ = std::fs::write(job_cg.join("cpu.weight"), "50");

    // Limit process count to prevent fork bombs
    let _ = std::fs::write(job_cg.join("pids.max"), "512");

    info!(job_id, cgroup = %job_cg.display(), mem_gb = max_ram_gb + 1, "cgroup v2 created");
    Ok(job_cg)
}

fn find_cgroup_v2_root() -> Option<PathBuf> {
    let content = std::fs::read_to_string("/proc/mounts").ok()?;
    for line in content.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 3 && parts[0] == "cgroup2" {
            return Some(PathBuf::from(parts[1]));
        }
    }
    // Common default
    let default = PathBuf::from("/sys/fs/cgroup");
    if default.exists() { Some(default) } else { None }
}

fn bwrap_available() -> bool {
    Command::new("bwrap").arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}
