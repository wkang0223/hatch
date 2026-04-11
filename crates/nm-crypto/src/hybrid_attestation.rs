//! Hybrid Ed25519 + Dilithium3 attestation.
//!
//! A hybrid attestation carries TWO independent signatures over the same claim:
//!   1. Ed25519   — classical, fast, 64 bytes
//!   2. Dilithium3 — post-quantum, 3293 bytes
//!
//! Security: an attacker must break BOTH schemes to forge an attestation.
//! This follows the IETF draft-ietf-tls-hybrid-design and NIST transition guidance.
//!
//! On-chain storage: we store BLAKE3(JSON(claim)) + both sigs.
//! The full attestation is stored off-chain (IPFS/coordinator); only the
//! 32-byte commitment hash goes on-chain (Solana/EVM).

use crate::keys::{NmKeypair, NmPublicKey};
use crate::pq_keys::{PqKeypair, PqPublicKey};
use anyhow::Result;
use nm_common::MacChipInfo;
use serde::{Deserialize, Serialize};

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ─── Claim ───────────────────────────────────────────────────────────────────

/// The hardware claim that a provider asserts about themselves.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HybridClaim {
    /// Ed25519 public key hex (classical identity, 32 bytes)
    pub ed25519_pubkey: String,
    /// Dilithium3 public key hex (PQ identity, 1952 bytes)
    pub dilithium3_pubkey: String,
    /// Job being bid on — binds attestation to a specific job (anti-replay)
    pub job_id: String,
    /// Hardware fingerprint from IOKit / GPU drivers
    pub chip: MacChipInfo,
    /// GPU vendor: "apple" | "nvidia" | "amd" | "intel_arc"
    pub gpu_vendor: String,
    /// Driver-reported GPU model string
    pub gpu_model: String,
    /// Nonce for extra replay protection (random u64)
    pub nonce: u64,
    /// Unix timestamp
    pub timestamp: u64,
    /// Schema version for forward compatibility
    pub version: u8,
}

impl HybridClaim {
    pub fn new(
        ed_keypair: &NmKeypair,
        pq_keypair: &PqKeypair,
        job_id: &str,
        chip: MacChipInfo,
        gpu_vendor: &str,
        gpu_model: &str,
    ) -> Self {
        Self {
            ed25519_pubkey: ed_keypair.public_key_hex(),
            dilithium3_pubkey: pq_keypair.public_key_hex(),
            job_id: job_id.to_string(),
            chip,
            gpu_vendor: gpu_vendor.to_string(),
            gpu_model: gpu_model.to_string(),
            nonce: rand_nonce(),
            timestamp: unix_now(),
            version: 1,
        }
    }

    /// Canonical signing bytes: BLAKE3(canonical JSON of claim).
    /// BLAKE3 is quantum-safe as a hash function.
    pub fn signing_bytes(&self) -> [u8; 32] {
        let json = serde_json::to_string(self).expect("HybridClaim serialisation");
        *blake3::hash(json.as_bytes()).as_bytes()
    }

    /// 32-byte on-chain commitment: BLAKE3(signing_bytes || ed_sig || dil3_sig).
    /// This is what gets stored in Solana/EVM — it commits to the full attestation
    /// without revealing the large PQ signature on-chain.
    pub fn on_chain_commitment(signing_bytes: &[u8; 32], ed_sig: &[u8], dil3_sig: &[u8]) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(signing_bytes);
        hasher.update(ed_sig);
        hasher.update(dil3_sig);
        *hasher.finalize().as_bytes()
    }
}

// ─── Signed attestation ──────────────────────────────────────────────────────

/// A fully hybrid-signed attestation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HybridAttestation {
    pub claim: HybridClaim,
    /// Ed25519 signature over BLAKE3(claim), hex-encoded (64 bytes)
    pub ed25519_sig: String,
    /// Dilithium3 signature over BLAKE3(claim), hex-encoded (~3293 bytes)
    pub dilithium3_sig: String,
    /// 32-byte on-chain commitment hash, hex-encoded
    pub on_chain_commitment: String,
}

impl HybridAttestation {
    /// Create and sign with both keypairs.
    pub fn create(
        ed_keypair: &NmKeypair,
        pq_keypair: &PqKeypair,
        job_id: &str,
        chip: MacChipInfo,
        gpu_vendor: &str,
        gpu_model: &str,
    ) -> Self {
        let claim = HybridClaim::new(ed_keypair, pq_keypair, job_id, chip, gpu_vendor, gpu_model);
        let signing_bytes = claim.signing_bytes();

        let ed_sig = ed_keypair.sign(&signing_bytes);
        let dil3_sig = pq_keypair.sign(&signing_bytes);

        let commitment = HybridClaim::on_chain_commitment(&signing_bytes, &ed_sig, &dil3_sig);

        Self {
            claim,
            ed25519_sig: hex::encode(&ed_sig),
            dilithium3_sig: hex::encode(&dil3_sig),
            on_chain_commitment: hex::encode(commitment),
        }
    }

    /// Verify both signatures and check freshness (5-minute window).
    pub fn verify(&self) -> Result<()> {
        let signing_bytes = self.claim.signing_bytes();

        // 1. Verify Ed25519
        let ed_pubkey = NmPublicKey::from_hex(&self.claim.ed25519_pubkey)?;
        let ed_sig_bytes = hex::decode(&self.ed25519_sig)
            .map_err(|e| anyhow::anyhow!("Invalid Ed25519 sig hex: {}", e))?;
        ed_pubkey.verify(&signing_bytes, &ed_sig_bytes)
            .map_err(|e| anyhow::anyhow!("Ed25519 verification failed: {}", e))?;

        // 2. Verify Dilithium3
        let pq_pubkey = PqPublicKey::from_hex(&self.claim.dilithium3_pubkey)?;
        let dil3_sig_bytes = hex::decode(&self.dilithium3_sig)
            .map_err(|e| anyhow::anyhow!("Invalid Dilithium3 sig hex: {}", e))?;
        pq_pubkey.verify(&signing_bytes, &dil3_sig_bytes)
            .map_err(|e| anyhow::anyhow!("Dilithium3 verification failed: {}", e))?;

        // 3. Verify commitment is consistent
        let recomputed = HybridClaim::on_chain_commitment(
            &signing_bytes,
            &ed_sig_bytes,
            &dil3_sig_bytes,
        );
        let stored = hex::decode(&self.on_chain_commitment)
            .map_err(|e| anyhow::anyhow!("Invalid commitment hex: {}", e))?;
        anyhow::ensure!(
            recomputed.as_ref() == stored.as_slice(),
            "On-chain commitment mismatch"
        );

        // 4. Freshness check (5 minutes)
        let age = unix_now().saturating_sub(self.claim.timestamp);
        anyhow::ensure!(age <= 300, "Attestation is stale ({} seconds old)", age);

        Ok(())
    }

    /// Commitment bytes for Solana/EVM (32 bytes).
    pub fn commitment_bytes(&self) -> Result<[u8; 32]> {
        let bytes = hex::decode(&self.on_chain_commitment)?;
        bytes.try_into()
            .map_err(|_| anyhow::anyhow!("Commitment is not 32 bytes"))
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("HybridAttestation serialisation")
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        serde_json::from_slice(bytes).map_err(Into::into)
    }

    /// Size in bytes (useful for budget estimation).
    pub fn size_estimate() -> usize {
        // claim JSON ~500 + ed_sig 128 hex + dil3_sig ~6586 hex + overhead
        7_500
    }
}

fn rand_nonce() -> u64 {
    use rand::Rng;
    rand::thread_rng().gen()
}
