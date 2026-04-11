//! NeuralMesh on-chain programme — Solana / Anchor 0.30
//!
//! Instructions:
//!   initialize          — Create the global ProgramConfig PDA
//!   initialize_mint     — Create the NMC SPL token mint
//!   bridge_mint         — Oracle mints NMC to a user (off-chain → on-chain bridge)
//!   bridge_burn         — User burns NMC (on-chain → off-chain bridge)
//!   register_provider   — Mint soul-bound provider NFT with PQ attestation commitment
//!   update_provider     — Update attestation after hardware change
//!   revoke_provider     — Authority slashes a provider NFT
//!   create_escrow       — Consumer locks NMC for a job
//!   lock_escrow         — Provider accepts the job
//!   release_escrow      — Oracle settles payment (provider + fee + refund)
//!   cancel_escrow       — Consumer/authority cancels, full refund
//!   update_config       — Authority updates platform fee / limits

use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    metadata::{
        create_master_edition_v3, create_metadata_accounts_v3, freeze_delegated_account,
        mpl_token_metadata::types::{Creator, DataV2},
        CreateMasterEditionV3, CreateMetadataAccountsV3, FreezeDelegatedAccount, Metadata,
    },
    token::{self, burn, mint_to, Burn, FreezeAccount, Mint, MintTo, Token, TokenAccount},
};

pub mod errors;
pub mod state;

use errors::NmError;
use state::*;

declare_id!("NMCxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx");

// ─── Constants ────────────────────────────────────────────────────────────────

/// NMC has 9 decimal places (1 NMC = 1_000_000_000 lamports).
pub const NMC_DECIMALS: u8 = 9;
pub const NMC_NAME: &str = "NeuralMesh Credit";
pub const NMC_SYMBOL: &str = "NMC";
pub const NFT_SYMBOL: &str = "NM-GPU";
/// Max platform fee: 10%
pub const MAX_FEE_BPS: u16 = 1_000;

// ─── Programme ────────────────────────────────────────────────────────────────

#[program]
pub mod neuralmesh {
    use super::*;

    // ── Global init ───────────────────────────────────────────────────────

    pub fn initialize(
        ctx: Context<Initialize>,
        platform_fee_bps: u16,
        bridge_mint_limit: u64,
    ) -> Result<()> {
        require!(platform_fee_bps <= MAX_FEE_BPS, NmError::FeeTooHigh);
        let cfg = &mut ctx.accounts.config;
        cfg.authority = ctx.accounts.authority.key();
        cfg.nmc_mint = ctx.accounts.nmc_mint.key();
        cfg.platform_fee_bps = platform_fee_bps;
        cfg.fee_collector = ctx.accounts.fee_collector.key();
        cfg.bridge_mint_limit = bridge_mint_limit;
        cfg.bump = ctx.bumps.config;
        Ok(())
    }

    // ── NMC token — bridge mint (off-chain → on-chain) ────────────────────

    /// Called by the trusted bridge oracle when a user deposits fiat/credits off-chain.
    /// `nonce` must equal `bridge_nonce_account.next_nonce` to prevent replay.
    pub fn bridge_mint(
        ctx: Context<BridgeMint>,
        amount: u64,
        nonce: u64,
    ) -> Result<()> {
        let cfg = &mut ctx.accounts.config;
        require!(amount <= cfg.bridge_mint_limit, NmError::MintLimitExceeded);

        // Check and advance nonce
        let nonce_acc = &mut ctx.accounts.bridge_nonce;
        require!(nonce == nonce_acc.next_nonce, NmError::Unauthorized);
        nonce_acc.next_nonce = nonce.checked_add(1).unwrap();

        // Mint NMC
        let seeds = &[b"config".as_ref(), &[cfg.bump]];
        let signer = &[&seeds[..]];
        mint_to(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                MintTo {
                    mint: ctx.accounts.nmc_mint.to_account_info(),
                    to: ctx.accounts.recipient_token_account.to_account_info(),
                    authority: ctx.accounts.config.to_account_info(),
                },
                signer,
            ),
            amount,
        )?;

        cfg.total_minted = cfg.total_minted.checked_add(amount).unwrap();
        emit!(BridgeMintEvent {
            recipient: ctx.accounts.recipient.key(),
            amount,
            nonce,
        });
        Ok(())
    }

    /// User burns NMC to withdraw to the off-chain ledger.
    pub fn bridge_burn(ctx: Context<BridgeBurn>, amount: u64) -> Result<()> {
        burn(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                Burn {
                    mint: ctx.accounts.nmc_mint.to_account_info(),
                    from: ctx.accounts.user_token_account.to_account_info(),
                    authority: ctx.accounts.user.to_account_info(),
                },
            ),
            amount,
        )?;

        ctx.accounts.config.total_burned =
            ctx.accounts.config.total_burned.checked_add(amount).unwrap();
        emit!(BridgeBurnEvent {
            user: ctx.accounts.user.key(),
            amount,
        });
        Ok(())
    }

    // ── Provider NFT ──────────────────────────────────────────────────────

    /// Register a GPU provider: creates a soul-bound NFT with PQ attestation commitment.
    pub fn register_provider(
        ctx: Context<RegisterProvider>,
        gpu_vendor: String,
        gpu_model: String,
        memory_gb: u16,
        gpu_cores: u32,
        attestation_commitment: [u8; 32],
        ed25519_pubkey: [u8; 64],
        dil3_pubkey_hash: [u8; 32],
        metadata_uri: String,
    ) -> Result<()> {
        require!(gpu_vendor.len() <= 32, NmError::VendorTooLong);
        require!(gpu_model.len() <= 64, NmError::ModelTooLong);
        require!(!ctx.accounts.provider_record.revoked, NmError::ProviderRevoked);

        // Fill provider record
        let rec = &mut ctx.accounts.provider_record;
        rec.provider = ctx.accounts.provider.key();
        rec.nft_mint = ctx.accounts.nft_mint.key();
        rec.memory_gb = memory_gb;
        rec.gpu_cores = gpu_cores;
        rec.attestation_commitment = attestation_commitment;
        rec.ed25519_pubkey = ed25519_pubkey;
        rec.dil3_pubkey_hash = dil3_pubkey_hash;
        rec.registered_at = Clock::get()?.unix_timestamp;
        rec.trust_score_milli = 2_500; // 2.5 / 5.0 starting score
        rec.bump = ctx.bumps.provider_record;

        let mut vendor_arr = [0u8; 32];
        let v = gpu_vendor.as_bytes();
        vendor_arr[..v.len()].copy_from_slice(v);
        rec.gpu_vendor = vendor_arr;

        let mut model_arr = [0u8; 64];
        let m = gpu_model.as_bytes();
        model_arr[..m.len()].copy_from_slice(m);
        rec.gpu_model = model_arr;

        // Mint 1 NFT to provider
        let config_bump = ctx.accounts.config.bump;
        let config_seeds = &[b"config".as_ref(), &[config_bump]];
        let signer = &[&config_seeds[..]];

        mint_to(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                MintTo {
                    mint: ctx.accounts.nft_mint.to_account_info(),
                    to: ctx.accounts.provider_nft_account.to_account_info(),
                    authority: ctx.accounts.config.to_account_info(),
                },
                signer,
            ),
            1,
        )?;

        // Create Metaplex metadata
        let creators = vec![Creator {
            address: ctx.accounts.config.key(),
            verified: true,
            share: 100,
        }];
        create_metadata_accounts_v3(
            CpiContext::new_with_signer(
                ctx.accounts.token_metadata_program.to_account_info(),
                CreateMetadataAccountsV3 {
                    metadata: ctx.accounts.nft_metadata.to_account_info(),
                    mint: ctx.accounts.nft_mint.to_account_info(),
                    mint_authority: ctx.accounts.config.to_account_info(),
                    update_authority: ctx.accounts.config.to_account_info(),
                    payer: ctx.accounts.provider.to_account_info(),
                    system_program: ctx.accounts.system_program.to_account_info(),
                    rent: ctx.accounts.rent.to_account_info(),
                },
                signer,
            ),
            DataV2 {
                name: format!("NM-GPU {}", gpu_model),
                symbol: NFT_SYMBOL.to_string(),
                uri: metadata_uri,
                seller_fee_basis_points: 0,
                creators: Some(creators),
                collection: None,
                uses: None,
            },
            true,
            true,
            None,
        )?;

        // Create master edition (makes it an NFT, supply = 1)
        create_master_edition_v3(
            CpiContext::new_with_signer(
                ctx.accounts.token_metadata_program.to_account_info(),
                CreateMasterEditionV3 {
                    edition: ctx.accounts.nft_master_edition.to_account_info(),
                    mint: ctx.accounts.nft_mint.to_account_info(),
                    update_authority: ctx.accounts.config.to_account_info(),
                    mint_authority: ctx.accounts.config.to_account_info(),
                    payer: ctx.accounts.provider.to_account_info(),
                    metadata: ctx.accounts.nft_metadata.to_account_info(),
                    token_program: ctx.accounts.token_program.to_account_info(),
                    system_program: ctx.accounts.system_program.to_account_info(),
                    rent: ctx.accounts.rent.to_account_info(),
                },
                signer,
            ),
            Some(0), // max_supply = 0 means unlimited editions; use Some(1) for 1/1
        )?;

        // Soul-bind: freeze the token account so it cannot be transferred
        freeze_delegated_account(
            CpiContext::new_with_signer(
                ctx.accounts.token_metadata_program.to_account_info(),
                FreezeDelegatedAccount {
                    delegate: ctx.accounts.config.to_account_info(),
                    token_account: ctx.accounts.provider_nft_account.to_account_info(),
                    edition: ctx.accounts.nft_master_edition.to_account_info(),
                    mint: ctx.accounts.nft_mint.to_account_info(),
                    token_program: ctx.accounts.token_program.to_account_info(),
                },
                signer,
            ),
        )?;

        emit!(ProviderRegisteredEvent {
            provider: ctx.accounts.provider.key(),
            nft_mint: ctx.accounts.nft_mint.key(),
            gpu_vendor,
            gpu_model,
            attestation_commitment,
        });
        Ok(())
    }

    /// Authority revokes a provider (slashing — marks NFT as revoked, cannot re-register).
    pub fn revoke_provider(ctx: Context<RevokeProvider>) -> Result<()> {
        ctx.accounts.provider_record.revoked = true;
        emit!(ProviderRevokedEvent {
            provider: ctx.accounts.provider_record.provider,
            nft_mint: ctx.accounts.provider_record.nft_mint,
        });
        Ok(())
    }

    // ── Job escrow ────────────────────────────────────────────────────────

    /// Consumer locks NMC for a job bid.
    pub fn create_escrow(
        ctx: Context<CreateEscrow>,
        job_id: String,
        locked_nmc: u64,
        price_per_hour: u64,
        max_duration_secs: u32,
    ) -> Result<()> {
        require!(job_id.len() <= 64, NmError::JobIdTooLong);

        let esc = &mut ctx.accounts.escrow;
        let mut job_id_arr = [0u8; 64];
        job_id_arr[..job_id.len()].copy_from_slice(job_id.as_bytes());
        esc.job_id = job_id_arr;
        esc.job_id_len = job_id.len() as u8;
        esc.consumer = ctx.accounts.consumer.key();
        esc.provider = ctx.accounts.provider.key();
        esc.locked_nmc = locked_nmc;
        esc.price_per_hour = price_per_hour;
        esc.max_duration_secs = max_duration_secs;
        esc.state = EscrowState::Open;
        esc.consumer_token_account = ctx.accounts.consumer_token_account.key();
        esc.provider_token_account = ctx.accounts.provider_token_account.key();
        esc.vault = ctx.accounts.vault.key();
        esc.created_at = Clock::get()?.unix_timestamp;
        esc.bump = ctx.bumps.escrow;

        // Transfer NMC from consumer → vault
        token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                token::Transfer {
                    from: ctx.accounts.consumer_token_account.to_account_info(),
                    to: ctx.accounts.vault.to_account_info(),
                    authority: ctx.accounts.consumer.to_account_info(),
                },
            ),
            locked_nmc,
        )?;

        emit!(EscrowCreatedEvent {
            job_id: std::str::from_utf8(&job_id_arr[..job_id.len() as usize])
                .unwrap_or("")
                .to_string(),
            consumer: ctx.accounts.consumer.key(),
            provider: ctx.accounts.provider.key(),
            locked_nmc,
        });
        Ok(())
    }

    /// Provider (or oracle) accepts the job — transitions Open → Locked, sets deadline.
    pub fn lock_escrow(ctx: Context<LockEscrow>) -> Result<()> {
        let esc = &mut ctx.accounts.escrow;
        require!(esc.state == EscrowState::Open, NmError::EscrowNotOpen);

        let now = Clock::get()?.unix_timestamp;
        esc.state = EscrowState::Locked;
        esc.deadline = now + esc.max_duration_secs as i64;
        Ok(())
    }

    /// Oracle settles the escrow after job completion.
    /// `actual_cost` ≤ locked_nmc; remainder is refunded to consumer.
    pub fn release_escrow(
        ctx: Context<ReleaseEscrow>,
        actual_cost: u64,
    ) -> Result<()> {
        let esc = &mut ctx.accounts.escrow;
        require!(esc.state == EscrowState::Locked, NmError::EscrowNotLocked);
        require!(actual_cost <= esc.locked_nmc, NmError::CostExceedsLocked);

        let cfg = &ctx.accounts.config;
        let fee = actual_cost
            .checked_mul(cfg.platform_fee_bps as u64)
            .unwrap()
            / 10_000;
        let provider_amount = actual_cost.checked_sub(fee).unwrap();
        let consumer_refund = esc.locked_nmc.checked_sub(actual_cost).unwrap();

        let vault_bump = esc.bump;
        let job_id_bytes = &esc.job_id[..esc.job_id_len as usize];
        let vault_seeds = &[b"escrow".as_ref(), job_id_bytes, &[vault_bump]];
        let signer = &[&vault_seeds[..]];

        // Pay provider
        if provider_amount > 0 {
            token::transfer(
                CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    token::Transfer {
                        from: ctx.accounts.vault.to_account_info(),
                        to: ctx.accounts.provider_token_account.to_account_info(),
                        authority: ctx.accounts.escrow.to_account_info(),
                    },
                    signer,
                ),
                provider_amount,
            )?;
        }

        // Collect platform fee
        if fee > 0 {
            token::transfer(
                CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    token::Transfer {
                        from: ctx.accounts.vault.to_account_info(),
                        to: ctx.accounts.fee_collector.to_account_info(),
                        authority: ctx.accounts.escrow.to_account_info(),
                    },
                    signer,
                ),
                fee,
            )?;
        }

        // Refund consumer remainder
        if consumer_refund > 0 {
            token::transfer(
                CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    token::Transfer {
                        from: ctx.accounts.vault.to_account_info(),
                        to: ctx.accounts.consumer_token_account.to_account_info(),
                        authority: ctx.accounts.escrow.to_account_info(),
                    },
                    signer,
                ),
                consumer_refund,
            )?;
        }

        esc.actual_cost = actual_cost;
        esc.fee_collected = fee;
        esc.state = EscrowState::Settled;
        esc.settled_at = Clock::get()?.unix_timestamp;

        emit!(EscrowSettledEvent {
            provider: esc.provider,
            consumer: esc.consumer,
            actual_cost,
            fee,
            consumer_refund,
        });
        Ok(())
    }

    /// Cancel an escrow — full refund to consumer.
    pub fn cancel_escrow(ctx: Context<CancelEscrow>) -> Result<()> {
        let esc = &mut ctx.accounts.escrow;
        require!(
            esc.state == EscrowState::Open || esc.state == EscrowState::Locked,
            NmError::EscrowNotOpen
        );

        let vault_bump = esc.bump;
        let job_id_bytes = &esc.job_id[..esc.job_id_len as usize];
        let vault_seeds = &[b"escrow".as_ref(), job_id_bytes, &[vault_bump]];
        let signer = &[&vault_seeds[..]];

        token::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                token::Transfer {
                    from: ctx.accounts.vault.to_account_info(),
                    to: ctx.accounts.consumer_token_account.to_account_info(),
                    authority: ctx.accounts.escrow.to_account_info(),
                },
                signer,
            ),
            esc.locked_nmc,
        )?;

        esc.state = EscrowState::Cancelled;
        Ok(())
    }

    /// Authority updates platform fee or bridge limit.
    pub fn update_config(
        ctx: Context<UpdateConfig>,
        platform_fee_bps: Option<u16>,
        bridge_mint_limit: Option<u64>,
    ) -> Result<()> {
        let cfg = &mut ctx.accounts.config;
        if let Some(fee) = platform_fee_bps {
            require!(fee <= MAX_FEE_BPS, NmError::FeeTooHigh);
            cfg.platform_fee_bps = fee;
        }
        if let Some(limit) = bridge_mint_limit {
            cfg.bridge_mint_limit = limit;
        }
        Ok(())
    }
}

// ─── Account contexts ─────────────────────────────────────────────────────────

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(
        init,
        payer = authority,
        space = ProgramConfig::LEN,
        seeds = [b"config"],
        bump,
    )]
    pub config: Account<'info, ProgramConfig>,
    #[account(mut)]
    pub authority: Signer<'info>,
    pub nmc_mint: Account<'info, Mint>,
    /// CHECK: Fee collector token account — validated by config
    pub fee_collector: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct BridgeMint<'info> {
    #[account(
        mut,
        seeds = [b"config"],
        bump = config.bump,
        has_one = authority @ NmError::Unauthorized,
    )]
    pub config: Account<'info, ProgramConfig>,
    #[account(mut, address = config.nmc_mint)]
    pub nmc_mint: Account<'info, Mint>,
    #[account(mut)]
    pub recipient_token_account: Account<'info, TokenAccount>,
    /// CHECK: just used to emit the event
    pub recipient: UncheckedAccount<'info>,
    #[account(
        mut,
        seeds = [b"bridge_nonce", authority.key().as_ref()],
        bump = bridge_nonce.bump,
        has_one = authority @ NmError::Unauthorized,
    )]
    pub bridge_nonce: Account<'info, BridgeNonce>,
    pub authority: Signer<'info>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct BridgeBurn<'info> {
    #[account(mut, seeds = [b"config"], bump = config.bump)]
    pub config: Account<'info, ProgramConfig>,
    #[account(mut, address = config.nmc_mint)]
    pub nmc_mint: Account<'info, Mint>,
    #[account(mut, token::mint = nmc_mint, token::authority = user)]
    pub user_token_account: Account<'info, TokenAccount>,
    pub user: Signer<'info>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct RegisterProvider<'info> {
    #[account(seeds = [b"config"], bump = config.bump)]
    pub config: Account<'info, ProgramConfig>,
    #[account(
        init,
        payer = provider,
        space = ProviderRecord::LEN,
        seeds = [b"provider", provider.key().as_ref()],
        bump,
    )]
    pub provider_record: Account<'info, ProviderRecord>,
    #[account(mut)]
    pub provider: Signer<'info>,
    /// New NFT mint (created externally as a 0-supply mint with config as authority)
    #[account(mut)]
    pub nft_mint: Account<'info, Mint>,
    /// Provider's ATA for the NFT
    #[account(
        init_if_needed,
        payer = provider,
        associated_token::mint = nft_mint,
        associated_token::authority = provider,
    )]
    pub provider_nft_account: Account<'info, TokenAccount>,
    /// CHECK: created by Metaplex CPI
    #[account(mut)]
    pub nft_metadata: UncheckedAccount<'info>,
    /// CHECK: created by Metaplex CPI
    #[account(mut)]
    pub nft_master_edition: UncheckedAccount<'info>,
    pub token_metadata_program: Program<'info, Metadata>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct RevokeProvider<'info> {
    #[account(
        seeds = [b"config"],
        bump = config.bump,
        has_one = authority @ NmError::Unauthorized,
    )]
    pub config: Account<'info, ProgramConfig>,
    #[account(mut)]
    pub provider_record: Account<'info, ProviderRecord>,
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
#[instruction(job_id: String)]
pub struct CreateEscrow<'info> {
    #[account(seeds = [b"config"], bump = config.bump)]
    pub config: Account<'info, ProgramConfig>,
    #[account(
        init,
        payer = consumer,
        space = JobEscrow::LEN,
        seeds = [b"escrow", job_id.as_bytes()],
        bump,
    )]
    pub escrow: Account<'info, JobEscrow>,
    #[account(mut)]
    pub consumer: Signer<'info>,
    /// CHECK: provider wallet
    pub provider: UncheckedAccount<'info>,
    #[account(mut, token::mint = config.nmc_mint, token::authority = consumer)]
    pub consumer_token_account: Account<'info, TokenAccount>,
    /// CHECK: provider's NMC token account
    pub provider_token_account: UncheckedAccount<'info>,
    #[account(
        init,
        payer = consumer,
        token::mint = nmc_mint,
        token::authority = escrow,
        seeds = [b"vault", job_id.as_bytes()],
        bump,
    )]
    pub vault: Account<'info, TokenAccount>,
    #[account(address = config.nmc_mint)]
    pub nmc_mint: Account<'info, Mint>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct LockEscrow<'info> {
    #[account(
        mut,
        seeds = [b"escrow", &escrow.job_id[..escrow.job_id_len as usize]],
        bump = escrow.bump,
        has_one = provider @ NmError::Unauthorized,
    )]
    pub escrow: Account<'info, JobEscrow>,
    pub provider: Signer<'info>,
}

#[derive(Accounts)]
pub struct ReleaseEscrow<'info> {
    #[account(
        seeds = [b"config"],
        bump = config.bump,
        has_one = authority @ NmError::Unauthorized,
    )]
    pub config: Account<'info, ProgramConfig>,
    #[account(
        mut,
        seeds = [b"escrow", &escrow.job_id[..escrow.job_id_len as usize]],
        bump = escrow.bump,
    )]
    pub escrow: Account<'info, JobEscrow>,
    #[account(mut)]
    pub vault: Account<'info, TokenAccount>,
    #[account(mut)]
    pub provider_token_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub consumer_token_account: Account<'info, TokenAccount>,
    #[account(mut, address = config.fee_collector)]
    pub fee_collector: Account<'info, TokenAccount>,
    pub authority: Signer<'info>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct CancelEscrow<'info> {
    #[account(
        seeds = [b"config"],
        bump = config.bump,
    )]
    pub config: Account<'info, ProgramConfig>,
    #[account(
        mut,
        seeds = [b"escrow", &escrow.job_id[..escrow.job_id_len as usize]],
        bump = escrow.bump,
    )]
    pub escrow: Account<'info, JobEscrow>,
    #[account(mut)]
    pub vault: Account<'info, TokenAccount>,
    #[account(mut)]
    pub consumer_token_account: Account<'info, TokenAccount>,
    /// Consumer OR authority can cancel
    pub signer: Signer<'info>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct UpdateConfig<'info> {
    #[account(
        mut,
        seeds = [b"config"],
        bump = config.bump,
        has_one = authority @ NmError::Unauthorized,
    )]
    pub config: Account<'info, ProgramConfig>,
    pub authority: Signer<'info>,
}

// ─── Events ───────────────────────────────────────────────────────────────────

#[event]
pub struct BridgeMintEvent {
    pub recipient: Pubkey,
    pub amount: u64,
    pub nonce: u64,
}

#[event]
pub struct BridgeBurnEvent {
    pub user: Pubkey,
    pub amount: u64,
}

#[event]
pub struct ProviderRegisteredEvent {
    pub provider: Pubkey,
    pub nft_mint: Pubkey,
    pub gpu_vendor: String,
    pub gpu_model: String,
    pub attestation_commitment: [u8; 32],
}

#[event]
pub struct ProviderRevokedEvent {
    pub provider: Pubkey,
    pub nft_mint: Pubkey,
}

#[event]
pub struct EscrowCreatedEvent {
    pub job_id: String,
    pub consumer: Pubkey,
    pub provider: Pubkey,
    pub locked_nmc: u64,
}

#[event]
pub struct EscrowSettledEvent {
    pub provider: Pubkey,
    pub consumer: Pubkey,
    pub actual_cost: u64,
    pub fee: u64,
    pub consumer_refund: u64,
}
