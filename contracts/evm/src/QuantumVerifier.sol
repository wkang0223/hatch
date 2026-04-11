// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.26;

/**
 * @title QuantumVerifier
 * @notice On-chain registry for post-quantum attestation commitments.
 *
 * Full Dilithium3 verification is ~3 KB of signature data and requires
 * ~50M gas on EVM — impractical today. Instead we use a commitment scheme:
 *
 *   commitment = BLAKE3(signing_bytes || ed25519_sig || dilithium3_sig)
 *
 * The 32-byte commitment is stored on-chain. The full attestation is stored
 * off-chain (IPFS / coordinator). A trusted verifier oracle (multisig) calls
 * `recordAttestation()` after verifying both Ed25519 and Dilithium3 off-chain.
 *
 * Migration path: when EIP-XXXX (PQ precompiles) ships, replace the oracle
 * with a direct `verifyDilithium3()` precompile call inside this contract.
 *
 * Security model:
 *   - Classical threat: Ed25519 protects commitment; oracle is a 3-of-5 multisig
 *   - Quantum threat: Dilithium3 off-chain verification; commitment hash is
 *     BLAKE3 which is quantum-safe as a hash function
 *   - Compromise requires breaking BOTH the oracle multisig AND the hash
 */
contract QuantumVerifier {
    // ── Events ────────────────────────────────────────────────────────────

    event AttestationRecorded(
        address indexed provider,
        bytes32 indexed commitment,
        string gpuVendor,
        string gpuModel,
        uint64 timestamp
    );
    event AttestationRevoked(address indexed provider, bytes32 commitment);
    event OracleAdded(address indexed oracle);
    event OracleRemoved(address indexed oracle);
    event ThresholdUpdated(uint8 newThreshold);

    // ── Structs ───────────────────────────────────────────────────────────

    struct AttestationRecord {
        bytes32 commitment;      // BLAKE3(signing_bytes || ed_sig || dil3_sig)
        string  gpuVendor;       // "apple" | "nvidia" | "amd" | "intel_arc"
        string  gpuModel;        // driver-reported model string
        bytes32 dil3PubkeyHash;  // keccak256(dilithium3_pubkey_bytes)
        bytes   ed25519Pubkey;   // 32-byte Ed25519 public key
        uint64  timestamp;
        bool    revoked;
    }

    // ── State ─────────────────────────────────────────────────────────────

    address public owner;
    mapping(address => bool) public oracles;
    uint8 public oracleCount;
    uint8 public threshold; // min oracle approvals to record

    // provider wallet → attestation
    mapping(address => AttestationRecord) public attestations;

    // commitment → approved oracle count (for multisig flow)
    mapping(bytes32 => mapping(address => bool)) private _oracleApproved;
    mapping(bytes32 => uint8) public approvalCount;

    // ── Modifiers ─────────────────────────────────────────────────────────

    modifier onlyOwner() {
        require(msg.sender == owner, "QV: not owner");
        _;
    }

    modifier onlyOracle() {
        require(oracles[msg.sender], "QV: not oracle");
        _;
    }

    // ── Constructor ───────────────────────────────────────────────────────

    constructor(address[] memory _oracles, uint8 _threshold) {
        require(_oracles.length >= _threshold && _threshold > 0, "QV: bad threshold");
        owner = msg.sender;
        threshold = _threshold;
        for (uint256 i = 0; i < _oracles.length; i++) {
            _addOracle(_oracles[i]);
        }
    }

    // ── Oracle management ─────────────────────────────────────────────────

    function addOracle(address oracle) external onlyOwner {
        _addOracle(oracle);
    }

    function removeOracle(address oracle) external onlyOwner {
        require(oracles[oracle], "QV: not an oracle");
        oracles[oracle] = false;
        oracleCount--;
        require(oracleCount >= threshold, "QV: would break threshold");
        emit OracleRemoved(oracle);
    }

    function setThreshold(uint8 _threshold) external onlyOwner {
        require(_threshold > 0 && _threshold <= oracleCount, "QV: bad threshold");
        threshold = _threshold;
        emit ThresholdUpdated(_threshold);
    }

    // ── Attestation recording (multisig flow) ─────────────────────────────

    /**
     * @notice Each oracle approves a (provider, commitment) pair.
     *         When `threshold` oracles approve, the attestation is recorded.
     * @param provider      Provider wallet address
     * @param commitment    32-byte BLAKE3 commitment from hybrid attestation
     * @param gpuVendor     "apple" | "nvidia" | "amd" | "intel_arc"
     * @param gpuModel      GPU model string
     * @param dil3PubkeyHash  keccak256 of the 1952-byte Dilithium3 public key
     * @param ed25519Pubkey  32-byte Ed25519 public key
     */
    function approveAttestation(
        address provider,
        bytes32 commitment,
        string calldata gpuVendor,
        string calldata gpuModel,
        bytes32 dil3PubkeyHash,
        bytes calldata ed25519Pubkey
    ) external onlyOracle {
        require(ed25519Pubkey.length == 32, "QV: ed25519 key must be 32 bytes");
        require(!attestations[provider].revoked, "QV: provider revoked");

        bytes32 approvalKey = keccak256(abi.encodePacked(provider, commitment));
        require(!_oracleApproved[approvalKey][msg.sender], "QV: already approved");
        _oracleApproved[approvalKey][msg.sender] = true;
        approvalCount[approvalKey]++;

        if (approvalCount[approvalKey] >= threshold) {
            attestations[provider] = AttestationRecord({
                commitment: commitment,
                gpuVendor: gpuVendor,
                gpuModel: gpuModel,
                dil3PubkeyHash: dil3PubkeyHash,
                ed25519Pubkey: ed25519Pubkey,
                timestamp: uint64(block.timestamp),
                revoked: false
            });
            emit AttestationRecorded(provider, commitment, gpuVendor, gpuModel, uint64(block.timestamp));
        }
    }

    /// Revoke a provider's attestation (slashing).
    function revokeAttestation(address provider) external onlyOwner {
        require(attestations[provider].commitment != bytes32(0), "QV: not registered");
        bytes32 commitment = attestations[provider].commitment;
        attestations[provider].revoked = true;
        emit AttestationRevoked(provider, commitment);
    }

    // ── View helpers ──────────────────────────────────────────────────────

    function isRegistered(address provider) external view returns (bool) {
        AttestationRecord storage rec = attestations[provider];
        return rec.commitment != bytes32(0) && !rec.revoked;
    }

    function getCommitment(address provider) external view returns (bytes32) {
        return attestations[provider].commitment;
    }

    // ── Internal ──────────────────────────────────────────────────────────

    function _addOracle(address oracle) internal {
        require(!oracles[oracle], "QV: already oracle");
        oracles[oracle] = true;
        oracleCount++;
        emit OracleAdded(oracle);
    }
}
