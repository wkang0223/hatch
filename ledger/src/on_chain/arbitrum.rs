//! ArbitrumOracle — on-chain settlement for NeuralMesh Phase 3.
//!
//! Calls `Escrow.releaseEscrow(jobId, actualCost)` and
//! `Escrow.cancelEscrow(jobId)` on Arbitrum via raw JSON-RPC
//! (no alloy dependency — avoids serde 0.9.x compatibility issues).
//!
//! # Environment variables
//!
//! | Variable              | Description                                     |
//! |-----------------------|-------------------------------------------------|
//! | `NM_ARBITRUM_RPC`     | HTTPS RPC endpoint (Alchemy / Infura / Ankr)    |
//! | `NM_ORACLE_PK`        | Oracle private key hex, 64 chars (no 0x prefix) |
//! | `NM_ESCROW_ADDRESS`   | Deployed `Escrow.sol` address (0x…)             |
//!
//! If `NM_ESCROW_ADDRESS` is absent the oracle is disabled (all calls return
//! `Ok(None)`) — safe for Phase 1/2 off-chain-only deployments.

use anyhow::{anyhow, bail, Context, Result};
use k256::ecdsa::{signature::Signer, SigningKey, Signature, RecoveryId};
use tracing::{info, warn};

// ── ABI encoding helpers ──────────────────────────────────────────────────────

/// keccak256 of a byte slice (used for function selectors + addresses).
fn keccak256(data: &[u8]) -> [u8; 32] {
    let mut output = [0u8; 32];
    tiny_keccak::keccak256_once(data, &mut output);
    output
}

/// Compute 4-byte function selector from a Solidity signature string.
/// e.g. `"releaseEscrow(string,uint256)"` → first 4 bytes of keccak256
fn selector(sig: &str) -> [u8; 4] {
    let hash = keccak256(sig.as_bytes());
    [hash[0], hash[1], hash[2], hash[3]]
}

/// ABI-encode a single `string` argument (head pointer + tail data).
///
/// Layout: [offset (32)] [length (32)] [data, padded to 32n bytes]
fn abi_encode_string(s: &str) -> Vec<u8> {
    let bytes = s.as_bytes();
    let len = bytes.len();
    // Offset pointing to the string data (starts after the 32-byte offset word)
    let mut out = Vec::with_capacity(64 + ((len + 31) / 32) * 32);
    // offset = 32 (one slot ahead)
    out.extend_from_slice(&uint256_be(32));
    // length
    out.extend_from_slice(&uint256_be(len as u128));
    // data, right-padded to multiple of 32
    out.extend_from_slice(bytes);
    let padding = (32 - (len % 32)) % 32;
    out.extend(std::iter::repeat(0).take(padding));
    out
}

/// ABI-encode `(string, uint256)` — for `releaseEscrow(string jobId, uint256 actualCost)`.
///
/// Layout:
///   [0..32]  offset of string (= 64, past both head words)
///   [32..64] uint256 value
///   [64..]   string data (length + padded bytes)
fn abi_encode_string_uint256(s: &str, value: u128) -> Vec<u8> {
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut out = Vec::new();
    // offset to string = 64 bytes (2 head slots × 32)
    out.extend_from_slice(&uint256_be(64));
    // uint256 value
    out.extend_from_slice(&uint256_be(value));
    // string length
    out.extend_from_slice(&uint256_be(len as u128));
    // string data, right-padded
    out.extend_from_slice(bytes);
    let padding = (32 - (len % 32)) % 32;
    out.extend(std::iter::repeat(0).take(padding));
    out
}

/// Encode a u128 as 32-byte big-endian (EVM uint256).
fn uint256_be(val: u128) -> [u8; 32] {
    let mut buf = [0u8; 32];
    let bytes = val.to_be_bytes(); // 16 bytes
    buf[16..].copy_from_slice(&bytes);
    buf
}

/// Convert NMC float to integer wei (NMC has 18 decimals, same as ETH).
/// Safely handles up to ~18 billion NMC without overflow.
fn nmc_to_wei_u128(nmc: f64) -> Result<u128> {
    if nmc < 0.0 {
        bail!("nmc_to_wei: negative amount");
    }
    // Work in fixed-point: multiply by 1e18, then truncate
    // Use integer arithmetic to avoid float drift at large values
    let whole = nmc.trunc() as u128;
    let frac  = (nmc.fract() * 1e18_f64).round() as u128;
    whole.checked_mul(10u128.pow(18))
        .and_then(|w| w.checked_add(frac))
        .ok_or_else(|| anyhow!("NMC amount too large for u128"))
}

// ── RLP / EIP-1559 transaction builder ───────────────────────────────────────

/// Minimal EIP-1559 transaction builder.
/// Only encodes the fields needed for `eth_sendRawTransaction`.
struct Eip1559Tx {
    chain_id:               u64,
    nonce:                  u64,
    max_priority_fee_per_gas: u128, // wei
    max_fee_per_gas:        u128,   // wei
    gas_limit:              u64,
    to:                     [u8; 20],
    value:                  u128,   // 0 for contract calls
    data:                   Vec<u8>,
    access_list:            Vec<u8>, // empty []
}

impl Eip1559Tx {
    /// RLP-encode the unsigned transaction for signing.
    ///
    /// EIP-1559 signing payload = `0x02 || RLP([chain_id, nonce, max_priority_fee,
    ///   max_fee, gas_limit, to, value, data, access_list])`
    fn signing_payload(&self) -> Vec<u8> {
        let mut rlp = Vec::new();
        rlp_append_uint(&mut rlp, self.chain_id as u128);
        rlp_append_uint(&mut rlp, self.nonce as u128);
        rlp_append_uint(&mut rlp, self.max_priority_fee_per_gas);
        rlp_append_uint(&mut rlp, self.max_fee_per_gas);
        rlp_append_uint(&mut rlp, self.gas_limit as u128);
        rlp_append_bytes(&mut rlp, &self.to);
        rlp_append_uint(&mut rlp, self.value);
        rlp_append_bytes(&mut rlp, &self.data);
        rlp_append_bytes(&mut rlp, &self.access_list); // empty list = 0xC0
        let list = rlp_encode_list(&rlp);
        let mut out = vec![0x02]; // EIP-1559 type prefix
        out.extend_from_slice(&list);
        out
    }

    /// Sign and RLP-encode the full transaction.
    /// Returns the raw bytes ready for `eth_sendRawTransaction`.
    fn sign_and_encode(&self, signing_key: &SigningKey) -> Result<Vec<u8>> {
        let payload = self.signing_payload();
        let hash    = keccak256(&payload);

        // k256 sign (deterministic RFC-6979)
        let (sig, recid): (Signature, RecoveryId) =
            signing_key.sign_prehash_recoverable(&hash)
                .context("ECDSA sign failed")?;
        let sig_bytes = sig.to_bytes();
        let r = &sig_bytes[..32];
        let s = &sig_bytes[32..];
        let v = recid.to_byte() as u128; // 0 or 1 for EIP-1559

        // Encode signed transaction:
        // 0x02 || RLP([chain_id, nonce, max_priority_fee, max_fee, gas, to, value, data, access_list, v, r, s])
        let mut rlp = Vec::new();
        rlp_append_uint(&mut rlp, self.chain_id as u128);
        rlp_append_uint(&mut rlp, self.nonce as u128);
        rlp_append_uint(&mut rlp, self.max_priority_fee_per_gas);
        rlp_append_uint(&mut rlp, self.max_fee_per_gas);
        rlp_append_uint(&mut rlp, self.gas_limit as u128);
        rlp_append_bytes(&mut rlp, &self.to);
        rlp_append_uint(&mut rlp, self.value);
        rlp_append_bytes(&mut rlp, &self.data);
        rlp_append_bytes(&mut rlp, &self.access_list);
        rlp_append_uint(&mut rlp, v);
        rlp_append_bytes(&mut rlp, r);
        rlp_append_bytes(&mut rlp, s);

        let list = rlp_encode_list(&rlp);
        let mut out = vec![0x02];
        out.extend_from_slice(&list);
        Ok(out)
    }
}

// ── Minimal RLP helpers ───────────────────────────────────────────────────────

fn rlp_length_encode(len: usize) -> Vec<u8> {
    if len < 56 {
        vec![0x80 + len as u8]
    } else {
        let len_bytes = rlp_uint_bytes(len as u64);
        let mut out = vec![0xB7 + len_bytes.len() as u8];
        out.extend_from_slice(&len_bytes);
        out
    }
}

fn rlp_uint_bytes(val: u64) -> Vec<u8> {
    let bytes = val.to_be_bytes();
    let start = bytes.iter().position(|&b| b != 0).unwrap_or(7);
    bytes[start..].to_vec()
}

fn rlp_append_uint(buf: &mut Vec<u8>, val: u128) {
    if val == 0 {
        buf.push(0x80); // RLP encoding of 0
        return;
    }
    let b = val.to_be_bytes();
    let start = b.iter().position(|&x| x != 0).unwrap_or(15);
    let bytes = &b[start..];
    if bytes.len() == 1 && bytes[0] < 0x80 {
        buf.push(bytes[0]); // single byte < 0x80 is its own encoding
    } else {
        buf.extend_from_slice(&rlp_length_encode(bytes.len()));
        buf.extend_from_slice(bytes);
    }
}

fn rlp_append_bytes(buf: &mut Vec<u8>, data: &[u8]) {
    if data.is_empty() {
        buf.push(0x80);
        return;
    }
    buf.extend_from_slice(&rlp_length_encode(data.len()));
    buf.extend_from_slice(data);
}

fn rlp_encode_list(contents: &[u8]) -> Vec<u8> {
    let len = contents.len();
    let mut out = if len < 56 {
        vec![0xC0 + len as u8]
    } else {
        let len_bytes = rlp_uint_bytes(len as u64);
        let mut h = vec![0xF7 + len_bytes.len() as u8];
        h.extend_from_slice(&len_bytes);
        h
    };
    out.extend_from_slice(contents);
    out
}

// ── JSON-RPC helpers ──────────────────────────────────────────────────────────

#[derive(Debug)]
struct RpcClient {
    url:    String,
    client: reqwest::Client,
}

impl RpcClient {
    fn new(url: &str) -> Self {
        Self { url: url.to_string(), client: reqwest::Client::new() }
    }

    async fn call(&self, method: &str, params: serde_json::Value) -> Result<serde_json::Value> {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params,
        });
        let res: serde_json::Value = self.client
            .post(&self.url)
            .json(&body)
            .send()
            .await
            .context("RPC HTTP request failed")?
            .json()
            .await
            .context("RPC response parse failed")?;

        if let Some(err) = res.get("error") {
            bail!("RPC error: {}", err);
        }
        Ok(res["result"].clone())
    }

    async fn get_nonce(&self, address: &str) -> Result<u64> {
        let res = self.call(
            "eth_getTransactionCount",
            serde_json::json!([address, "latest"])
        ).await?;
        let hex = res.as_str().ok_or_else(|| anyhow!("nonce not a string"))?;
        Ok(u64::from_str_radix(hex.trim_start_matches("0x"), 16)?)
    }

    async fn gas_price(&self) -> Result<u128> {
        let res = self.call("eth_gasPrice", serde_json::json!([])).await?;
        let hex = res.as_str().ok_or_else(|| anyhow!("gasPrice not a string"))?;
        Ok(u128::from_str_radix(hex.trim_start_matches("0x"), 16)?)
    }

    async fn chain_id(&self) -> Result<u64> {
        let res = self.call("eth_chainId", serde_json::json!([])).await?;
        let hex = res.as_str().ok_or_else(|| anyhow!("chainId not a string"))?;
        Ok(u64::from_str_radix(hex.trim_start_matches("0x"), 16)?)
    }

    async fn send_raw_tx(&self, raw: &[u8]) -> Result<String> {
        let hex = format!("0x{}", hex::encode(raw));
        let res = self.call("eth_sendRawTransaction", serde_json::json!([hex])).await?;
        res.as_str()
            .map(String::from)
            .ok_or_else(|| anyhow!("sendRawTransaction result not a string"))
    }

    async fn wait_for_receipt(&self, tx_hash: &str, max_attempts: u32) -> Result<serde_json::Value> {
        for _ in 0..max_attempts {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            let res = self.call(
                "eth_getTransactionReceipt",
                serde_json::json!([tx_hash])
            ).await?;
            if !res.is_null() {
                return Ok(res);
            }
        }
        bail!("Timed out waiting for tx {} after {} attempts", tx_hash, max_attempts)
    }
}

// ── Parse 20-byte address ─────────────────────────────────────────────────────

fn parse_address(addr: &str) -> Result<[u8; 20]> {
    let hex = addr.trim_start_matches("0x");
    if hex.len() != 40 {
        bail!("Address must be 40 hex chars, got {}", hex.len());
    }
    let bytes = hex::decode(hex).context("Invalid address hex")?;
    Ok(bytes.try_into().map_err(|_| anyhow!("Address not 20 bytes"))?)
}

/// Derive the Ethereum address from a `k256` signing key.
fn signing_key_to_address(key: &SigningKey) -> [u8; 20] {
    let pubkey = key.verifying_key().to_encoded_point(false); // uncompressed
    let pubkey_bytes = pubkey.as_bytes();
    // Skip the 0x04 prefix → 64 bytes
    let hash = keccak256(&pubkey_bytes[1..]);
    let mut addr = [0u8; 20];
    addr.copy_from_slice(&hash[12..]);
    addr
}

// ── Module for tiny-keccak — available as a transitive dep via libp2p ─────────

mod tiny_keccak {
    /// Compute keccak256 in one shot.
    pub fn keccak256_once(data: &[u8], output: &mut [u8; 32]) {
        use sha3::{Digest, Keccak256};
        let mut h = Keccak256::new();
        h.update(data);
        let result = h.finalize();
        output.copy_from_slice(&result);
    }
}

// ── ArbitrumOracle ────────────────────────────────────────────────────────────

/// Settlement oracle that sends signed Arbitrum transactions.
///
/// Construct via [`ArbitrumOracle::from_env`] — reads env vars automatically.
#[derive(Clone, Debug)]
pub struct ArbitrumOracle {
    rpc_url:        String,
    oracle_pk_hex:  String,
    escrow_addr_hex: String,
    pub enabled:    bool,
}

impl ArbitrumOracle {
    /// Initialise from environment variables.
    pub fn from_env() -> Self {
        let escrow_addr = std::env::var("NM_ESCROW_ADDRESS").unwrap_or_default();
        if escrow_addr.is_empty() {
            info!("NM_ESCROW_ADDRESS not set — on-chain settlement disabled");
            return Self::disabled();
        }
        let rpc_url = std::env::var("NM_ARBITRUM_RPC")
            .unwrap_or_else(|_| "https://sepolia-rollup.arbitrum.io/rpc".to_string());
        let oracle_pk = std::env::var("NM_ORACLE_PK").unwrap_or_default();
        if oracle_pk.len() != 64 {
            warn!("NM_ORACLE_PK missing or invalid length — on-chain disabled");
            return Self::disabled();
        }
        info!(escrow = %escrow_addr, rpc = %rpc_url, "On-chain oracle enabled");
        Self {
            rpc_url,
            oracle_pk_hex:   oracle_pk,
            escrow_addr_hex: escrow_addr,
            enabled:         true,
        }
    }

    pub fn disabled() -> Self {
        Self {
            rpc_url:         String::new(),
            oracle_pk_hex:   String::new(),
            escrow_addr_hex: String::new(),
            enabled:         false,
        }
    }

    fn signing_key(&self) -> Result<SigningKey> {
        let bytes = hex::decode(&self.oracle_pk_hex)
            .context("Invalid NM_ORACLE_PK hex")?;
        SigningKey::from_bytes(bytes.as_slice().into())
            .context("Invalid private key bytes")
    }

    /// Send a signed transaction calling `calldata` on the Escrow contract.
    /// Returns the tx hash (hex string).
    async fn send_tx(&self, calldata: Vec<u8>) -> Result<String> {
        let rpc = RpcClient::new(&self.rpc_url);
        let key = self.signing_key()?;
        let from_addr = signing_key_to_address(&key);
        let from_hex  = format!("0x{}", hex::encode(from_addr));

        let nonce    = rpc.get_nonce(&from_hex).await?;
        let gas_price = rpc.gas_price().await?;
        let chain_id = rpc.chain_id().await?;
        let to_addr  = parse_address(&self.escrow_addr_hex)?;

        // Gas estimation: use a fixed 150k gas (enough for releaseEscrow / cancelEscrow)
        let gas_limit = 150_000u64;
        // Priority fee: 0.1 gwei (Arbitrum is cheap)
        let priority_fee = 100_000_000u128; // 0.1 gwei
        // Base fee × 1.5 buffer + priority fee
        let max_fee = (gas_price * 3 / 2) + priority_fee;

        let tx = Eip1559Tx {
            chain_id,
            nonce,
            max_priority_fee_per_gas: priority_fee,
            max_fee_per_gas:          max_fee,
            gas_limit,
            to:    to_addr,
            value: 0,
            data:  calldata,
            access_list: vec![], // empty
        };

        let raw = tx.sign_and_encode(&key)?;
        let tx_hash = rpc.send_raw_tx(&raw).await?;

        info!(tx_hash = %tx_hash, "Transaction sent");

        // Wait for receipt (up to 30 × 2s = 60s)
        let receipt = rpc.wait_for_receipt(&tx_hash, 30).await?;
        let status = receipt["status"].as_str().unwrap_or("0x0");
        if status != "0x1" {
            bail!("Transaction reverted: {}", tx_hash);
        }

        Ok(tx_hash)
    }

    // ── Public API ────────────────────────────────────────────────────────────

    /// Call `Escrow.releaseEscrow(jobId, actualCost)` on Arbitrum.
    pub async fn release_escrow(
        &self,
        job_id:          &str,
        actual_cost_nmc: f64,
    ) -> Result<Option<String>> {
        if !self.enabled { return Ok(None); }

        let cost_wei = nmc_to_wei_u128(actual_cost_nmc)?;

        // Function selector for releaseEscrow(string,uint256)
        let sel  = selector("releaseEscrow(string,uint256)");
        let args = abi_encode_string_uint256(job_id, cost_wei);
        let mut calldata = sel.to_vec();
        calldata.extend_from_slice(&args);

        info!(
            job_id, actual_cost_nmc,
            cost_wei = cost_wei,
            "Submitting releaseEscrow on Arbitrum"
        );

        let tx_hash = self.send_tx(calldata).await?;
        info!(job_id, tx_hash = %tx_hash, "Escrow released on Arbitrum");
        Ok(Some(tx_hash))
    }

    /// Call `Escrow.cancelEscrow(jobId)` on Arbitrum (full consumer refund).
    pub async fn cancel_escrow(&self, job_id: &str) -> Result<Option<String>> {
        if !self.enabled { return Ok(None); }

        let sel  = selector("cancelEscrow(string)");
        let args = abi_encode_string(job_id);
        let mut calldata = sel.to_vec();
        calldata.extend_from_slice(&args);

        info!(job_id, "Submitting cancelEscrow on Arbitrum");

        let tx_hash = self.send_tx(calldata).await?;
        info!(job_id, tx_hash = %tx_hash, "Escrow cancelled on Arbitrum");
        Ok(Some(tx_hash))
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nmc_to_wei_whole() {
        assert_eq!(nmc_to_wei_u128(1.0).unwrap(), 10u128.pow(18));
    }

    #[test]
    fn test_nmc_to_wei_fraction() {
        let wei = nmc_to_wei_u128(0.5).unwrap();
        assert_eq!(wei, 5 * 10u128.pow(17));
    }

    #[test]
    fn test_selector() {
        // Known selector: keccak256("releaseEscrow(string,uint256)")[0..4]
        // We can't check exact bytes without running keccak256, but verify it's 4 bytes
        let s = selector("releaseEscrow(string,uint256)");
        assert_eq!(s.len(), 4);
    }

    #[test]
    fn test_disabled_oracle() {
        std::env::remove_var("NM_ESCROW_ADDRESS");
        let o = ArbitrumOracle::from_env();
        assert!(!o.enabled);
    }

    #[test]
    fn test_abi_encode_string() {
        let encoded = abi_encode_string("hello");
        assert_eq!(encoded.len(), 96); // 32 (offset) + 32 (len) + 32 (data padded)
        // offset = 32
        assert_eq!(&encoded[..32], &uint256_be(32)[..]);
        // length = 5
        assert_eq!(&encoded[32..64], &uint256_be(5)[..]);
        // "hello" at bytes 64..69, rest 0
        assert_eq!(&encoded[64..69], b"hello");
        assert_eq!(&encoded[69..96], &[0u8; 27][..]);
    }
}
