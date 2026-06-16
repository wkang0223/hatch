//! NAT traversal helpers: STUN endpoint discovery + relay fallback.

use anyhow::Result;
use std::net::{SocketAddr, UdpSocket};
use tracing::{debug, warn};

/// Well-known public STUN servers (Google + Cloudflare).
const STUN_SERVERS: &[&str] = &[
    "stun.l.google.com:19302",
    "stun1.l.google.com:19302",
    "stun.cloudflare.com:3478",
];

/// Discover this machine's public IP and port for the given local UDP port.
/// Uses STUN Binding Request (RFC 5389).
pub fn discover_public_endpoint(local_port: u16) -> Result<String> {
    let socket = UdpSocket::bind(format!("0.0.0.0:{}", local_port))?;
    socket.set_read_timeout(Some(std::time::Duration::from_secs(3)))?;

    for server in STUN_SERVERS {
        match try_stun(&socket, server) {
            Ok(addr) => {
                debug!(public_endpoint = %addr, "STUN discovery succeeded");
                return Ok(addr.to_string());
            }
            Err(e) => {
                warn!(server, error = %e, "STUN server failed, trying next");
            }
        }
    }
    anyhow::bail!("All STUN servers failed — provider may be behind symmetric NAT")
}

/// Send a minimal STUN Binding Request and parse the mapped address.
fn try_stun(socket: &UdpSocket, server: &str) -> Result<SocketAddr> {
    use std::net::ToSocketAddrs;

    let server_addr = server
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| anyhow::anyhow!("Could not resolve {}", server))?;

    // STUN Binding Request (20 bytes)
    let request = build_stun_request();
    socket.send_to(&request, server_addr)?;

    let mut buf = [0u8; 512];
    let (len, _) = socket.recv_from(&mut buf)?;
    parse_stun_response(&buf[..len])
}

/// Build a minimal STUN Binding Request message.
fn build_stun_request() -> Vec<u8> {
    let mut msg = vec![0u8; 20];
    // Message type: Binding Request = 0x0001
    msg[0] = 0x00; msg[1] = 0x01;
    // Message length: 0 (no attributes)
    msg[2] = 0x00; msg[3] = 0x00;
    // Magic cookie: 0x2112A442
    msg[4] = 0x21; msg[5] = 0x12; msg[6] = 0xA4; msg[7] = 0x42;
    // Transaction ID: 12 random bytes
    for i in 8..20 {
        msg[i] = rand::random::<u8>();
    }
    msg
}

/// Parse STUN Binding Response to extract the XOR-MAPPED-ADDRESS.
fn parse_stun_response(buf: &[u8]) -> Result<SocketAddr> {
    if buf.len() < 20 {
        anyhow::bail!("STUN response too short");
    }

    // Skip 20-byte header, parse attributes
    let mut pos = 20;
    while pos + 4 <= buf.len() {
        let attr_type  = u16::from_be_bytes([buf[pos], buf[pos+1]]);
        let attr_len   = u16::from_be_bytes([buf[pos+2], buf[pos+3]]) as usize;
        pos += 4;

        if attr_type == 0x0020 && attr_len >= 8 {
            // XOR-MAPPED-ADDRESS
            // buf[pos] = 0x00 (reserved)
            // buf[pos+1] = family (0x01 = IPv4)
            // buf[pos+2..3] = XOR'd port
            // buf[pos+4..7] = XOR'd IP
            let xport = u16::from_be_bytes([buf[pos+2], buf[pos+3]]) ^ 0x2112u16;
            let xip   = u32::from_be_bytes([buf[pos+4], buf[pos+5], buf[pos+6], buf[pos+7]])
                        ^ 0x2112A442u32;
            let ip = std::net::Ipv4Addr::from(xip);
            return Ok(SocketAddr::from((ip, xport)));
        } else if attr_type == 0x0001 && attr_len >= 8 {
            // MAPPED-ADDRESS (fallback)
            let port = u16::from_be_bytes([buf[pos+2], buf[pos+3]]);
            let ip   = std::net::Ipv4Addr::from([buf[pos+4], buf[pos+5], buf[pos+6], buf[pos+7]]);
            return Ok(SocketAddr::from((ip, port)));
        }

        pos += (attr_len + 3) & !3; // round up to 4-byte boundary
    }
    anyhow::bail!("No mapped address found in STUN response")
}
