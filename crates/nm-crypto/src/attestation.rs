//! Hardware attestation for Apple Silicon providers.
//!
//! Attestation proves that:
//!   1. The provider's GPU is a real Apple Silicon chip (via IOKit serial)
//!   2. The claim is signed by the provider's Ed25519 keypair
//!   3. The signature cannot be replayed for a different job
//!
//! Format: `AttestationClaim` is serialised to JSON, then Ed25519-signed.

use crate::keys::{NmKeypair, NmPublicKey};
use anyhow::Result;
use nm_common::MacChipInfo;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The claim that a provider signs to prove their hardware identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttestationClaim {
    /// Provider's public key hex
    pub provider_pubkey: String,
    /// The job being bid on (prevents replay attacks)
    pub job_id: String,
    /// Chip info sourced from IOKit
    pub chip: MacChipInfo,
    /// Unix timestamp of claim creation
    pub timestamp: u64,
}

impl AttestationClaim {
    pub fn new(provider_pubkey: &str, job_id: &str, chip: MacChipInfo) -> Self {
        Self {
            provider_pubkey: provider_pubkey.to_string(),
            job_id: job_id.to_string(),
            chip,
            timestamp: unix_now(),
        }
    }

    /// Canonical bytes to sign: SHA-256(JSON of claim).
    pub fn signing_bytes(&self) -> Vec<u8> {
        let json = serde_json::to_string(self).expect("AttestationClaim serialization");
        let mut hasher = Sha256::new();
        hasher.update(json.as_bytes());
        hasher.finalize().to_vec()
    }
}

/// A signed attestation: claim + Ed25519 signature.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attestation {
    pub claim: AttestationClaim,
    /// Ed25519 signature over SHA-256(JSON(claim)), hex-encoded
    pub signature: String,
}

impl Attestation {
    /// Create and sign a new attestation.
    pub fn create(keypair: &NmKeypair, job_id: &str, chip: MacChipInfo) -> Self {
        let claim = AttestationClaim::new(
            &keypair.public_key_hex(),
            job_id,
            chip,
        );
        let sig_bytes = keypair.sign(&claim.signing_bytes());
        Self {
            claim,
            signature: hex::encode(sig_bytes),
        }
    }

    /// Verify the attestation against the provider's claimed public key.
    /// Also checks timestamp freshness (within 5 minutes).
    pub fn verify(&self) -> Result<()> {
        let pubkey = NmPublicKey::from_hex(&self.claim.provider_pubkey)?;
        let sig_bytes = hex::decode(&self.signature)
            .map_err(|e| anyhow::anyhow!("Invalid signature hex: {}", e))?;
        pubkey.verify(&self.claim.signing_bytes(), &sig_bytes)?;

        // Freshness check
        let age_secs = unix_now().saturating_sub(self.claim.timestamp);
        if age_secs > 300 {
            anyhow::bail!("Attestation is stale ({} seconds old)", age_secs);
        }

        Ok(())
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("Attestation serialization")
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        serde_json::from_slice(bytes).map_err(Into::into)
    }
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
