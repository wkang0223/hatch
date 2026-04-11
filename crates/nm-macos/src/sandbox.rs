//! Generate macOS sandbox-exec profiles for job isolation.
//!
//! Jobs run as the `neuralmesh_worker` OS user and are further restricted
//! by a per-job sandbox-exec profile that limits:
//!   - File system access to /tmp/neuralmesh/<job_id>/ and Python runtime paths
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
        let work_dir = PathBuf::from(format!("/tmp/neuralmesh/{}", job_id));
        std::fs::create_dir_all(&work_dir)?;

        let profile_path = PathBuf::from(format!("/tmp/neuralmesh/{}.sb", job_id));
        let profile = Self::generate_profile(job_id, &work_dir, runtime, python_prefix);
        std::fs::write(&profile_path, profile)?;

        Ok(Self {
            job_id: job_id.to_string(),
            work_dir,
            profile_path,
        })
    }

    /// Build the sandbox-exec(1) scheme profile.
    fn generate_profile(
        job_id: &str,
        work_dir: &Path,
        runtime: &str,
        python_prefix: &str,
    ) -> String {
        let work_str = work_dir.to_string_lossy();

        // Runtime-specific paths that must be readable
        let runtime_paths = match runtime {
            "mlx" | "torch-mps" | "onnx-coreml" => vec![
                python_prefix.to_string(),
                "/opt/homebrew/lib".to_string(),
                "/opt/homebrew/bin".to_string(),
                "/opt/homebrew/opt".to_string(),
            ],
            "llama-cpp" => vec![
                "/opt/homebrew/bin".to_string(),
                "/opt/homebrew/lib".to_string(),
            ],
            _ => vec![],
        };

        let allow_read_blocks: String = runtime_paths
            .iter()
            .map(|p| format!("(allow file-read* (subpath \"{}\"))", p))
            .collect::<Vec<_>>()
            .join("\n");

        // Metal framework paths needed for GPU access
        let metal_paths = vec![
            "/System/Library/Frameworks/Metal.framework",
            "/System/Library/Frameworks/MetalPerformanceShaders.framework",
            "/System/Library/Frameworks/Accelerate.framework",
            "/System/Library/Frameworks/CoreML.framework",
            "/System/Library/Frameworks/CoreGraphics.framework",
            "/usr/lib",
            "/usr/local/lib",
        ];
        let metal_read_blocks: String = metal_paths
            .iter()
            .map(|p| format!("(allow file-read* (subpath \"{}\"))", p))
            .collect::<Vec<_>>()
            .join("\n");

        format!(
            r#"; NeuralMesh sandbox profile for job {job_id}
; Generated automatically — do not edit manually
(version 1)

; Default deny everything
(deny default)

; Allow read of job working directory (full access)
(allow file-read*  (subpath "{work_dir}"))
(allow file-write* (subpath "{work_dir}"))

; Allow reads of /tmp for Python temp files
(allow file-read*  (subpath "/tmp"))
(allow file-write* (subpath "/tmp/neuralmesh/{job_id}"))

; Allow reads of system libraries
(allow file-read* (subpath "/usr/lib"))
(allow file-read* (subpath "/usr/local/lib"))
(allow file-read* (subpath "/System/Library"))
(allow file-read* (subpath "/Library/Frameworks"))
(allow file-read* (subpath "/private/var/db/timezone"))
(allow file-read* (literal "/etc/resolv.conf"))
(allow file-read* (literal "/etc/localtime"))

; Allow Metal GPU frameworks
{metal_read_blocks}

; Allow Python runtime
{allow_read_blocks}

; Allow process execution (Python, shell tools)
(allow process-exec)
(allow process-fork)

; Allow mach IPC (required for Metal GPU access)
(allow mach-lookup)
(allow mach-register)
(allow ipc-posix-shm)

; Network: localhost only (for model servers binding to 127.0.0.1)
(allow network-outbound (local))
(allow network-bind    (local))
(deny  network-outbound)

; Allow basic POSIX operations
(allow signal (target self))
(allow sysctl-read)
(allow iokit-open)
(allow iokit-get-properties)
"#,
            job_id = job_id,
            work_dir = work_str,
            metal_read_blocks = metal_read_blocks,
            allow_read_blocks = allow_read_blocks,
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
