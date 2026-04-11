//! Ed25519 keypair management for NeuralMesh nodes.
//! Each provider and consumer has one persistent Ed25519 keypair.
//! The public key serves as the node's identity; the private key signs attestations.

use anyhow::{Context, Result};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;
use std::path::Path;

/// A NeuralMesh Ed25519 keypair (signing + verifying).
pub struct NmKeypair {
    inner: SigningKey,
}

impl NmKeypair {
    /// Generate a new random keypair.
    pub fn generate() -> Self {
        let inner = SigningKey::generate(&mut OsRng);
        Self { inner }
    }

    /// Load from 32-byte raw seed stored in a file.
    pub fn load_from_file(path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path)
            .with_context(|| format!("Reading keypair from {}", path.display()))?;
        let seed: [u8; 32] = bytes
            .try_into()
            .map_err(|_| anyhow::anyhow!("Invalid keypair file: expected 32 bytes"))?;
        Ok(Self {
            inner: SigningKey::from_bytes(&seed),
        })
    }

    /// Save the 32-byte seed to a file (chmod 600).
    pub fn save_to_file(&self, path: &Path) -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, self.inner.to_bytes())?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        Ok(())
    }

    /// Sign arbitrary bytes. Returns a 64-byte Ed25519 signature.
    pub fn sign(&self, message: &[u8]) -> Vec<u8> {
        self.inner.sign(message).to_bytes().to_vec()
    }

    /// The public key (verifying key) as bytes.
    pub fn public_key_bytes(&self) -> [u8; 32] {
        self.inner.verifying_key().to_bytes()
    }

    /// The public key as a hex string (used as provider ID).
    pub fn public_key_hex(&self) -> String {
        hex::encode(self.public_key_bytes())
    }

    pub fn verifying_key(&self) -> VerifyingKey {
        self.inner.verifying_key()
    }
}

/// A NeuralMesh public key used to verify signatures.
#[derive(Debug, Clone)]
pub struct NmPublicKey {
    inner: VerifyingKey,
}

impl NmPublicKey {
    pub fn from_bytes(bytes: &[u8; 32]) -> Result<Self> {
        let inner = VerifyingKey::from_bytes(bytes)
            .context("Invalid Ed25519 public key")?;
        Ok(Self { inner })
    }

    pub fn from_hex(hex_str: &str) -> Result<Self> {
        let bytes = hex::decode(hex_str).context("Invalid hex public key")?;
        let arr: [u8; 32] = bytes
            .try_into()
            .map_err(|_| anyhow::anyhow!("Public key must be 32 bytes"))?;
        Self::from_bytes(&arr)
    }

    /// Verify a signature over message. Returns Ok(()) on success.
    pub fn verify(&self, message: &[u8], signature_bytes: &[u8]) -> Result<()> {
        let sig_arr: [u8; 64] = signature_bytes
            .try_into()
            .map_err(|_| anyhow::anyhow!("Signature must be 64 bytes"))?;
        let sig = Signature::from_bytes(&sig_arr);
        self.inner.verify(message, &sig)
            .context("Signature verification failed")
    }

    pub fn to_hex(&self) -> String {
        hex::encode(self.inner.to_bytes())
    }
}
