//! Per-job WireGuard tunnel management for the agent.

use anyhow::Result;
use nm_wireguard::{
    keys::WgKeypair,
    tunnel::{TunnelConfig, WgTunnel, allocate_tunnel_ips, iface_name},
    nat::discover_public_endpoint,
};
use tracing::info;

pub struct JobTunnel {
    _tunnel: WgTunnel,
    pub local_ip: String,
    pub peer_ip: String,
    pub public_endpoint: Option<String>,
    pub our_pubkey_b64: String,
}

impl JobTunnel {
    /// Set up an ephemeral WireGuard tunnel for the given job.
    pub fn setup(
        job_id: &str,
        slot: u8,
        listen_port: u16,
        consumer_wg_pubkey_b64: &str,
        consumer_endpoint: Option<&str>,
    ) -> Result<Self> {
        let keypair = WgKeypair::generate();
        let our_pubkey_b64 = keypair.public_key_b64();
        let (local_ip, peer_ip) = allocate_tunnel_ips(slot);
        let iface = iface_name(job_id);

        // Try to discover our public endpoint via STUN
        let public_endpoint = discover_public_endpoint(listen_port)
            .ok()
            .map(|ep| ep);

        let cfg = TunnelConfig {
            iface: iface.clone(),
            local_ip: local_ip.clone(),
            peer_ip: peer_ip.clone(),
            listen_port,
            local_keypair: keypair,
            peer_pubkey_b64: consumer_wg_pubkey_b64.to_string(),
            peer_endpoint: consumer_endpoint.map(str::to_string),
        };

        let tunnel = WgTunnel::bring_up(&cfg)?;
        info!(job_id, iface, local_ip, peer_ip, "WireGuard tunnel established");

        Ok(Self {
            _tunnel: tunnel,
            local_ip,
            peer_ip,
            public_endpoint,
            our_pubkey_b64,
        })
    }

    /// The endpoint string to send to the consumer (IP:port).
    pub fn endpoint_for_consumer(&self, port: u16) -> String {
        match &self.public_endpoint {
            Some(ep) => ep.clone(),
            None => format!("{}:{}", self.local_ip, port),
        }
    }
}
