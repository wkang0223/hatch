//! WireGuard X25519 key generation for ephemeral per-job tunnels.

use anyhow::Result;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use boringtun::x25519;
use rand::rngs::OsRng;

/// An ephemeral WireGuard keypair (X25519).
/// Generated fresh for every job — never reused.
#[derive(Clone)]
pub struct WgKeypair {
    secret: x25519::StaticSecret,
    public: x25519::PublicKey,
}

impl std::fmt::Debug for WgKeypair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WgKeypair")
            .field("public", &self.public_key_b64())
            .field("secret", &"[redacted]")
            .finish()
    }
}

impl WgKeypair {
    pub fn generate() -> Self {
        let secret = x25519::StaticSecret::random_from_rng(OsRng);
        let public = x25519::PublicKey::from(&secret);
        Self { secret, public }
    }

    /// Base64-encoded public key (standard WireGuard format).
    pub fn public_key_b64(&self) -> String {
        B64.encode(self.public.as_bytes())
    }

    /// Base64-encoded private key.
    pub fn private_key_b64(&self) -> String {
        B64.encode(self.secret.to_bytes())
    }

    pub fn public_key_bytes(&self) -> [u8; 32] {
        *self.public.as_bytes()
    }

    pub fn secret(&self) -> &x25519::StaticSecret {
        &self.secret
    }

    pub fn public(&self) -> &x25519::PublicKey {
        &self.public
    }
}

/// Parse a base64-encoded WireGuard public key into bytes.
pub fn parse_wg_pubkey(b64: &str) -> Result<[u8; 32]> {
    let bytes = B64.decode(b64)?;
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("WireGuard public key must be 32 bytes"))?;
    Ok(arr)
}
