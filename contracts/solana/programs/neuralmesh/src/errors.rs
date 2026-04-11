use anchor_lang::prelude::*;

#[error_code]
pub enum NmError {
    // ── Auth ──────────────────────────────────────────────────────────────
    #[msg("Caller is not the programme authority")]
    Unauthorized,
    #[msg("This NFT is soul-bound and cannot be transferred")]
    SoulBound,

    // ── NMC token ─────────────────────────────────────────────────────────
    #[msg("Mint amount exceeds bridge daily limit")]
    MintLimitExceeded,
    #[msg("Insufficient NMC balance")]
    InsufficientBalance,

    // ── Provider NFT ──────────────────────────────────────────────────────
    #[msg("Provider is already registered")]
    AlreadyRegistered,
    #[msg("Attestation commitment is invalid (wrong length)")]
    InvalidCommitment,
    #[msg("Provider NFT has been revoked")]
    ProviderRevoked,
    #[msg("GPU vendor string too long (max 32 bytes)")]
    VendorTooLong,
    #[msg("GPU model string too long (max 64 bytes)")]
    ModelTooLong,

    // ── Escrow ────────────────────────────────────────────────────────────
    #[msg("Escrow is not in the Open state")]
    EscrowNotOpen,
    #[msg("Escrow is not in the Locked state")]
    EscrowNotLocked,
    #[msg("Escrow deadline has not passed yet")]
    DeadlineNotReached,
    #[msg("Actual cost exceeds locked amount")]
    CostExceedsLocked,
    #[msg("Platform fee basis points must be ≤ 1000 (10%)")]
    FeeTooHigh,
    #[msg("Job ID string too long (max 64 bytes)")]
    JobIdTooLong,
}
