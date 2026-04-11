//! WireGuard tunnel management using the `wg` userspace tool.
//! We use `wg-quick`-style approach via shell commands so that the agent
//! doesn't require kernel module compilation — works on macOS with the
//! wireguard-go or boringtun userspace implementation.
//!
//! Each job gets an ephemeral tunnel on a unique /30 subnet:
//!   Provider: 10.77.<job_slot>.1/30
//!   Consumer: 10.77.<job_slot>.2/30

use crate::keys::WgKeypair;
use anyhow::{Context, Result};
use std::process::Command;
use tracing::{debug, info, warn};

#[derive(Debug, Clone)]
pub struct TunnelConfig {
    /// Interface name e.g. "nm-job0"
    pub iface: String,
    /// Provider's tunnel IP (always the .1)
    pub local_ip: String,
    /// Consumer's tunnel IP (always the .2)
    pub peer_ip: String,
    /// UDP listen port on provider side
    pub listen_port: u16,
    /// Provider's ephemeral WireGuard keypair
    pub local_keypair: WgKeypair,
    /// Consumer's WireGuard public key (base64)
    pub peer_pubkey_b64: String,
    /// Consumer's endpoint (IP:port) if known; None if behind NAT (relay used)
    pub peer_endpoint: Option<String>,
}

/// A live WireGuard tunnel. Destroyed on Drop.
pub struct WgTunnel {
    iface: String,
}

impl WgTunnel {
    /// Bring up a WireGuard interface using the macOS `wg` + `ifconfig` toolchain.
    /// Requires: `wireguard-tools` installed via Homebrew (`brew install wireguard-tools`).
    pub fn bring_up(cfg: &TunnelConfig) -> Result<Self> {
        let iface = &cfg.iface;

        // Write a temporary wg config file
        let wg_conf = format!(
            "[Interface]\nPrivateKey = {}\nListenPort = {}\n\n\
             [Peer]\nPublicKey = {}\nAllowedIPs = {}/32\n{}\n",
            cfg.local_keypair.private_key_b64(),
            cfg.listen_port,
            cfg.peer_pubkey_b64,
            cfg.peer_ip,
            cfg.peer_endpoint
                .as_ref()
                .map(|ep| format!("Endpoint = {}", ep))
                .unwrap_or_default(),
        );

        let conf_path = format!("/tmp/neuralmesh/wg-{}.conf", iface);
        std::fs::write(&conf_path, &wg_conf)
            .context("Writing WireGuard config")?;

        // macOS: create utun interface via wireguard-go or wg-quick
        // Try wg-quick first (simplest), fallback to manual utun creation
        let status = Command::new("wg-quick")
            .args(["up", &conf_path])
            .status();

        match status {
            Ok(s) if s.success() => {
                info!(iface, "WireGuard tunnel up");
            }
            _ => {
                // Fallback: manual utun setup
                Self::manual_setup(cfg, &wg_conf, &conf_path)?;
            }
        }

        // Clean up conf file — key material no longer needed on disk
        let _ = std::fs::remove_file(&conf_path);

        Ok(Self { iface: iface.clone() })
    }

    /// Fallback tunnel setup using utun + wg commands directly.
    fn manual_setup(cfg: &TunnelConfig, wg_conf: &str, conf_path: &str) -> Result<()> {
        // On macOS, use `sudo wireguard-go <iface>` then configure
        Command::new("sudo")
            .args(["wireguard-go", &cfg.iface])
            .status()
            .context("wireguard-go")?;

        Command::new("sudo")
            .args(["wg", "setconf", &cfg.iface, conf_path])
            .status()
            .context("wg setconf")?;

        Command::new("sudo")
            .args([
                "ifconfig", &cfg.iface,
                "inet", &format!("{}/30", cfg.local_ip),
                &cfg.peer_ip,
                "up",
            ])
            .status()
            .context("ifconfig")?;

        debug!(iface = %cfg.iface, "WireGuard interface configured manually");
        Ok(())
    }

    pub fn iface(&self) -> &str {
        &self.iface
    }
}

impl Drop for WgTunnel {
    fn drop(&mut self) {
        info!(iface = %self.iface, "Tearing down WireGuard tunnel");
        let _ = Command::new("wg-quick")
            .args(["down", &self.iface])
            .status();
        // Fallback: kill wireguard-go process for this interface
        let _ = Command::new("sudo")
            .args(["kill", &format!("$(cat /var/run/wireguard/{}.name)", self.iface)])
            .status();
    }
}

/// Allocate a tunnel IP pair for a job slot index.
/// Slots 0–253 → 10.77.0.0/30 … 10.77.253.0/30
pub fn allocate_tunnel_ips(slot: u8) -> (String, String) {
    let provider_ip = format!("10.77.{}.1", slot);
    let consumer_ip = format!("10.77.{}.2", slot);
    (provider_ip, consumer_ip)
}

/// Generate a unique interface name for a job.
pub fn iface_name(job_id: &str) -> String {
    // Take first 8 chars of job UUID for the interface name
    let short = job_id.chars().take(8).collect::<String>();
    format!("nm-{}", short)
}
