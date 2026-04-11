use thiserror::Error;

#[derive(Debug, Error)]
pub enum NmError {
    #[error("GPU detection failed: {0}")]
    GpuDetect(String),

    #[error("Provider not available: {0}")]
    ProviderUnavailable(String),

    #[error("Insufficient credits: need {need:.4} NMC, have {have:.4} NMC")]
    InsufficientCredits { need: f64, have: f64 },

    #[error("Job not found: {job_id}")]
    JobNotFound { job_id: String },

    #[error("Job rejected: {reason}")]
    JobRejected { reason: String },

    #[error("Attestation verification failed: {0}")]
    AttestationFailed(String),

    #[error("WireGuard error: {0}")]
    WireGuard(String),

    #[error("Network error: {0}")]
    Network(String),

    #[error("Sandbox error: {0}")]
    Sandbox(String),

    #[error("Runtime not installed: {0}")]
    RuntimeNotInstalled(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("Internal error: {0}")]
    Internal(String),
}

pub type NmResult<T> = Result<T, NmError>;
