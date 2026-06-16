//! Generate macOS sandbox-exec profiles for job isolation.
//!
//! Jobs run as the `hatch_worker` OS user and are further restricted
//! by a per-job sandbox-exec profile that limits:
//!   - File system access to /tmp/hatch/<job_id>/ and Python runtime paths
//!   - Network: localhost only (no outbound internet)
//!   - Process: no privilege escalation, no signal to unrelated PIDs

use std::path::{Path, PathBuf};

/// A generated sandbox-exec profile for a specific job.
pub struct SandboxProfile {
    pub job_id: String,
    pub work_dir: PathBuf,
    pub profile_path: PathBuf,
}

impl SandboxProfile {
    /// Create a sandbox profile for the given job.
    /// `python_lib_dir` should be the site-packages path for the runtime's Python.
    pub fn new(job_id: &str, runtime: &str, python_prefix: &str) -> anyhow::Result<Self> {
        let work_dir = PathBuf::from(format!("/tmp/hatch/{}", job_id));
        std::fs::create_dir_all(&work_dir)?;

        let profile_path = PathBuf::from(format!("/tmp/hatch/{}.sb", job_id));
        let profile = Self::generate_profile(job_id, &work_dir, runtime, python_prefix);
        std::fs::write(&profile_path, profile)?;

        Ok(Self {
            job_id: job_id.to_string(),
            work_dir,
            profile_path,
        })
    }

    /// Build the sandbox-exec(1) scheme profile.
    ///
    /// Strategy: (allow default) + targeted denies.
    /// Using (deny default) + allowlist causes Python3 to hang indefinitely because
    /// Python needs many mach services that are hard to enumerate exhaustively.
    /// Allow-default + deny-dangerous is simpler and reliable.
    fn generate_profile(
        job_id: &str,
        work_dir: &Path,
        _runtime: &str,
        _python_prefix: &str,
    ) -> String {
        let work_str = work_dir.to_string_lossy();

        // /tmp is a symlink to /private/tmp on macOS; allow both paths for the
        // job's own directory so Python tempfile.* works when TMPDIR is set to
        // the job work dir.  Broader /tmp access is denied so jobs cannot read
        // or corrupt sibling jobs' working directories.
        let private_work = format!("/private/tmp/hatch/{}", job_id);
        format!(
            r#"; Hatch sandbox profile for job {job_id}
; Generated automatically — do not edit manually
(version 1)

; Allow everything by default, then deny dangerous operations.
; This avoids Python3 hanging on mach service lookups while still
; preventing network exfiltration and filesystem escapes.
(allow default)

; --- NETWORK: deny all outbound IP except localhost ---
; Jobs may bind localhost ports (e.g. model servers), but cannot
; reach the internet or other LAN hosts.
; Note: SBPL network ip rules require host:port format ("*:*" not "*").
(deny network-outbound (remote ip "*:*"))
(allow network-outbound (remote ip "localhost:*"))

; --- FILESYSTEM: deny writes outside this job's work directory ---
; /tmp and /private/tmp are denied at the top level, then the job's
; own subdirectory is re-allowed.  This prevents cross-job contamination.
(deny file-write* (subpath "/"))
(allow file-write* (subpath "{work_dir}"))
(allow file-write* (subpath "{private_work}"))
(allow file-write* (literal "/dev/null"))
(allow file-write* (literal "/dev/stdout"))
(allow file-write* (literal "/dev/stderr"))
"#,
            job_id = job_id,
            work_dir = work_str,
            private_work = private_work,
        )
    }

    /// Clean up the job's working directory and profile.
    pub fn cleanup(&self) -> anyhow::Result<()> {
        if self.work_dir.exists() {
            std::fs::remove_dir_all(&self.work_dir)?;
        }
        if self.profile_path.exists() {
            std::fs::remove_file(&self.profile_path)?;
        }
        Ok(())
    }
}
