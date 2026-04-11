use anchor_lang::prelude::*;

// ─── Global configuration ────────────────────────────────────────────────────

#[account]
#[derive(Default)]
pub struct ProgramConfig {
    /// Programme authority (multisig in production)
    pub authority: Pubkey,
    /// NMC SPL token mint
    pub nmc_mint: Pubkey,
    /// Platform fee in basis points (e.g. 300 = 3%)
    pub platform_fee_bps: u16,
    /// Fee collector token account
    pub fee_collector: Pubkey,
    /// Max NMC that can be minted per bridge tx (anti-abuse)
    pub bridge_mint_limit: u64,
    /// Total NMC ever minted (for supply tracking)
    pub total_minted: u64,
    /// Total NMC ever burned (bridge-out)
    pub total_burned: u64,
    /// Bump for this PDA
    pub bump: u8,
}

impl ProgramConfig {
    pub const LEN: usize = 8   // discriminator
        + 32   // authority
        + 32   // nmc_mint
        + 2    // platform_fee_bps
        + 32   // fee_collector
        + 8    // bridge_mint_limit
        + 8    // total_minted
        + 8    // total_burned
        + 1;   // bump
}

// ─── Provider NFT metadata ───────────────────────────────────────────────────

/// Stored as a PDA (seeds = [b"provider", provider_wallet]).
/// The associated NFT mint is 1:1 with this account; soul-bound = frozen.
#[account]
pub struct ProviderRecord {
    /// Provider wallet (owner)
    pub provider: Pubkey,
    /// The NFT mint for this provider
    pub nft_mint: Pubkey,
    /// GPU vendor: "apple" | "nvidia" | "amd" | "intel_arc"
    pub gpu_vendor: [u8; 32],
    /// GPU model string (driver-reported)
    pub gpu_model: [u8; 64],
    /// Unified/VRAM memory in GB
    pub memory_gb: u16,
    /// GPU core count
    pub gpu_cores: u32,
    /// 32-byte BLAKE3 on-chain commitment from hybrid attestation
    pub attestation_commitment: [u8; 32],
    /// Ed25519 pubkey hex (32 bytes = 64 hex chars)
    pub ed25519_pubkey: [u8; 64],
    /// Dilithium3 pubkey hash (BLAKE3 of 1952-byte pubkey → 32 bytes on-chain)
    pub dil3_pubkey_hash: [u8; 32],
    /// Number of jobs completed
    pub jobs_completed: u64,
    /// NMC earned lifetime (lamport-equivalent, 9 decimals)
    pub nmc_earned: u64,
    /// Trust score * 1000 (0–5000)
    pub trust_score_milli: u16,
    /// Registration timestamp
    pub registered_at: i64,
    /// Revoked flag (soul-bound slashing)
    pub revoked: bool,
    /// Bump for this PDA
    pub bump: u8,
}

impl ProviderRecord {
    pub const LEN: usize = 8    // discriminator
        + 32   // provider
        + 32   // nft_mint
        + 32   // gpu_vendor
        + 64   // gpu_model
        + 2    // memory_gb
        + 4    // gpu_cores
        + 32   // attestation_commitment
        + 64   // ed25519_pubkey
        + 32   // dil3_pubkey_hash
        + 8    // jobs_completed
        + 8    // nmc_earned
        + 2    // trust_score_milli
        + 8    // registered_at
        + 1    // revoked
        + 1;   // bump
}

// ─── Job Escrow ───────────────────────────────────────────────────────────────

#[derive(AnchorSerialize, AnchorDeserialize, Clone, PartialEq, Eq)]
pub enum EscrowState {
    /// Created, NMC locked; waiting for provider to start
    Open,
    /// Provider accepted job; counting down to deadline
    Locked,
    /// Settled: NMC distributed (provider + fee + consumer refund)
    Settled,
    /// Cancelled: full refund to consumer
    Cancelled,
}

/// PDA seeds = [b"escrow", job_id.as_bytes()]
#[account]
pub struct JobEscrow {
    /// Off-chain job UUID (max 64 bytes)
    pub job_id: [u8; 64],
    pub job_id_len: u8,
    /// Consumer (funder)
    pub consumer: Pubkey,
    /// Assigned provider
    pub provider: Pubkey,
    /// Amount of NMC locked (with 9 decimals)
    pub locked_nmc: u64,
    /// Provider floor price per hour (NMC with 9 decimals)
    pub price_per_hour: u64,
    /// Max job duration in seconds
    pub max_duration_secs: u32,
    /// Wall-clock deadline (Unix timestamp)
    pub deadline: i64,
    /// State machine
    pub state: EscrowState,
    /// Actual cost settled (set on release)
    pub actual_cost: u64,
    /// Consumer NMC token account
    pub consumer_token_account: Pubkey,
    /// Provider NMC token account
    pub provider_token_account: Pubkey,
    /// Escrow vault (PDA-owned token account)
    pub vault: Pubkey,
    /// Platform fee collected
    pub fee_collected: u64,
    /// Creation timestamp
    pub created_at: i64,
    /// Settlement timestamp (0 = not settled)
    pub settled_at: i64,
    pub bump: u8,
}

impl JobEscrow {
    pub const LEN: usize = 8    // discriminator
        + 64   // job_id
        + 1    // job_id_len
        + 32   // consumer
        + 32   // provider
        + 8    // locked_nmc
        + 8    // price_per_hour
        + 4    // max_duration_secs
        + 8    // deadline
        + 2    // state (enum)
        + 8    // actual_cost
        + 32   // consumer_token_account
        + 32   // provider_token_account
        + 32   // vault
        + 8    // fee_collected
        + 8    // created_at
        + 8    // settled_at
        + 1;   // bump
}

// ─── Bridge nonce (replay protection) ────────────────────────────────────────

/// One per bridge authority; tracks nonces to prevent double-minting.
#[account]
pub struct BridgeNonce {
    pub authority: Pubkey,
    pub next_nonce: u64,
    pub bump: u8,
}

impl BridgeNonce {
    pub const LEN: usize = 8 + 32 + 8 + 1;
}
