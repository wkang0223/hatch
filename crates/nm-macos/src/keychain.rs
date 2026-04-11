//! Store and retrieve secrets from the macOS Keychain.
//! On Apple Silicon, the Keychain is backed by the Secure Enclave.

use anyhow::{Context, Result};
use std::process::Command;

const SERVICE: &str = "io.neuralmesh.agent";

/// Store a secret in the macOS Keychain.
pub fn keychain_set(key: &str, value: &str) -> Result<()> {
    // Delete existing item first (ignore error if not found)
    let _ = Command::new("security")
        .args([
            "delete-generic-password",
            "-s", SERVICE,
            "-a", key,
        ])
        .output();

    let status = Command::new("security")
        .args([
            "add-generic-password",
            "-s", SERVICE,
            "-a", key,
            "-w", value,
            "-U", // Update if exists
        ])
        .status()
        .context("Failed to run `security` command")?;

    if !status.success() {
        anyhow::bail!("Failed to store key {} in Keychain", key);
    }
    Ok(())
}

/// Retrieve a secret from the macOS Keychain.
pub fn keychain_get(key: &str) -> Result<String> {
    let out = Command::new("security")
        .args([
            "find-generic-password",
            "-s", SERVICE,
            "-a", key,
            "-w", // Output password only
        ])
        .output()
        .context("Failed to run `security` command")?;

    if !out.status.success() {
        anyhow::bail!("Key {} not found in Keychain", key);
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Delete a secret from the macOS Keychain.
pub fn keychain_delete(key: &str) -> Result<()> {
    Command::new("security")
        .args(["delete-generic-password", "-s", SERVICE, "-a", key])
        .status()
        .context("Failed to run `security` command")?;
    Ok(())
}
