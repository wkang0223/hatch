//! Post-quantum keypair management using CRYSTALS-Dilithium3 (ML-DSA, NIST FIPS 204).
//!
//! Dilithium3 provides NIST security level 3 (~AES-192 equivalent), with:
//!   - Public key:  1952 bytes
//!   - Secret key:  4000 bytes
//!   - Signature:   3293 bytes
//!
//! We store only the packed bytes on disk (no raw secret); the file is chmod 600.

use anyhow::{Context, Result};
use pqcrypto_dilithium::dilithium3::{
    self, PublicKey as Dilithium3PublicKey, SecretKey as Dilithium3SecretKey,
    SignedMessage,
};
use pqcrypto_traits::sign::{PublicKey, SecretKey, SignedMessage as SignedMessageTrait};
use serde::{Deserialize, Serialize};
use std::path::Path;

// ─── Keypair ────────────────────────────────────────────────────────────────

pub struct PqKeypair {
    pub_key: Dilithium3PublicKey,
    sec_key: Dilithium3SecretKey,
}

impl PqKeypair {
    /// Generate a fresh Dilithium3 keypair.
    pub fn generate() -> Self {
        let (pk, sk) = dilithium3::keypair();
        Self {
            pub_key: pk,
            sec_key: sk,
        }
    }

    /// Load from a file that contains `[32-byte length prefix][pk bytes][sk bytes]`.
    pub fn load_from_file(path: &Path) -> Result<Self> {
        let raw = std::fs::read(path)
            .with_context(|| format!("Reading PQ keypair from {}", path.display()))?;

        let pk_len = dilithium3::public_key_bytes();
        let sk_len = dilithium3::secret_key_bytes();
        anyhow::ensure!(
            raw.len() == pk_len + sk_len,
            "PQ keypair file has wrong length: expected {}, got {}",
            pk_len + sk_len,
            raw.len()
        );

        let pub_key = Dilithium3PublicKey::from_bytes(&raw[..pk_len])
            .context("Invalid Dilithium3 public key bytes")?;
        let sec_key = Dilithium3SecretKey::from_bytes(&raw[pk_len..])
            .context("Invalid Dilithium3 secret key bytes")?;

        Ok(Self { pub_key, sec_key })
    }

    /// Save keypair to file (chmod 600).
    pub fn save_to_file(&self, path: &Path) -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut data = self.pub_key.as_bytes().to_vec();
        data.extend_from_slice(self.sec_key.as_bytes());
        std::fs::write(path, &data)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        Ok(())
    }

    /// Sign arbitrary bytes. Returns the detached signature bytes (~3293 bytes).
    pub fn sign(&self, message: &[u8]) -> Vec<u8> {
        let signed = dilithium3::sign(message, &self.sec_key);
        // Extract the signature prefix (signed = sig || message)
        let sig_len = signed.as_bytes().len() - message.len();
        signed.as_bytes()[..sig_len].to_vec()
    }

    /// Returns the public key bytes (1952 bytes).
    pub fn public_key_bytes(&self) -> Vec<u8> {
        self.pub_key.as_bytes().to_vec()
    }

    /// Returns the public key as hex.
    pub fn public_key_hex(&self) -> String {
        hex::encode(self.pub_key.as_bytes())
    }

    pub fn public_key(&self) -> &Dilithium3PublicKey {
        &self.pub_key
    }
}

// ─── Public key (verify-only) ────────────────────────────────────────────────

pub struct PqPublicKey {
    inner: Dilithium3PublicKey,
}

impl PqPublicKey {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let inner = Dilithium3PublicKey::from_bytes(bytes)
            .context("Invalid Dilithium3 public key")?;
        Ok(Self { inner })
    }

    pub fn from_hex(hex_str: &str) -> Result<Self> {
        let bytes = hex::decode(hex_str).context("Invalid hex public key")?;
        Self::from_bytes(&bytes)
    }

    /// Verify a detached signature over `message`. Returns Ok(()) on success.
    pub fn verify(&self, message: &[u8], signature: &[u8]) -> Result<()> {
        // Re-attach signature to message to use pqcrypto's open API
        let sig_len = dilithium3::signature_bytes();
        anyhow::ensure!(
            signature.len() == sig_len,
            "Dilithium3 signature must be {} bytes, got {}",
            sig_len,
            signature.len()
        );
        let mut signed_bytes = signature.to_vec();
        signed_bytes.extend_from_slice(message);
        let signed = SignedMessage::from_bytes(&signed_bytes)
            .context("Malformed signed message")?;
        dilithium3::open(&signed, &self.inner)
            .map(|_| ())
            .map_err(|_| anyhow::anyhow!("Dilithium3 signature verification failed"))
    }

    pub fn to_hex(&self) -> String {
        hex::encode(self.inner.as_bytes())
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.inner.as_bytes()
    }
}

// ─── KEM (Kyber-768 / ML-KEM for encrypted channels) ───────────────────────

use pqcrypto_kyber::kyber768::{
    self, Ciphertext, PublicKey as KyberPublicKey, SecretKey as KyberSecretKey, SharedSecret,
};
use pqcrypto_traits::kem::{
    Ciphertext as CiphertextTrait, PublicKey as KemPublicKey, SecretKey as KemSecretKey,
    SharedSecret as SharedSecretTrait,
};

pub struct KemKeypair {
    pub_key: KyberPublicKey,
    sec_key: KyberSecretKey,
}

impl KemKeypair {
    pub fn generate() -> Self {
        let (pk, sk) = kyber768::keypair();
        Self {
            pub_key: pk,
            sec_key: sk,
        }
    }

    /// Encapsulate: consumer produces a ciphertext + shared secret for the provider's public key.
    pub fn encapsulate_for(their_pubkey: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
        let pk = KyberPublicKey::from_bytes(their_pubkey)
            .context("Invalid Kyber768 public key")?;
        let (shared, ciphertext) = kyber768::encapsulate(&pk);
        Ok((shared.as_bytes().to_vec(), ciphertext.as_bytes().to_vec()))
    }

    /// Decapsulate: provider recovers the shared secret from the ciphertext.
    pub fn decapsulate(&self, ciphertext: &[u8]) -> Result<Vec<u8>> {
        let ct = Ciphertext::from_bytes(ciphertext)
            .context("Invalid Kyber768 ciphertext")?;
        let shared = kyber768::decapsulate(&ct, &self.sec_key);
        Ok(shared.as_bytes().to_vec())
    }

    pub fn public_key_bytes(&self) -> Vec<u8> {
        self.pub_key.as_bytes().to_vec()
    }
}

// ─── Serializable summary for on-chain / storage ─────────────────────────────

/// Compact identity record stored in the ledger and on-chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PqIdentity {
    /// Dilithium3 public key, hex-encoded
    pub dil3_pubkey: String,
    /// Kyber768 KEM public key, hex-encoded
    pub kyber768_pubkey: String,
}
