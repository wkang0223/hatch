// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.26;

import "@openzeppelin/contracts/token/ERC721/ERC721.sol";
import "@openzeppelin/contracts/token/ERC721/extensions/ERC721URIStorage.sol";
import "@openzeppelin/contracts/access/AccessControl.sol";
import "./QuantumVerifier.sol";

/**
 * @title ProviderNFT — Soul-bound GPU provider identity token
 *
 * Each verified GPU provider receives one non-transferable NFT.
 * The NFT metadata URI points to an IPFS JSON file containing:
 *   - GPU vendor + model
 *   - BLAKE3 attestation commitment (links to full hybrid PQ attestation)
 *   - Ed25519 pubkey
 *   - Dilithium3 pubkey hash
 *   - Hardware fingerprint (serial number hash)
 *
 * Soul-bound: _update() reverts on any transfer (mint to non-zero OK).
 * Slashing: admin can burn the NFT (marks provider as revoked on-chain).
 *
 * Providers must have a valid QuantumVerifier attestation to receive an NFT.
 */
contract ProviderNFT is ERC721, ERC721URIStorage, AccessControl {

    // ── Roles ─────────────────────────────────────────────────────────────
    bytes32 public constant MINTER_ROLE  = keccak256("MINTER_ROLE");
    bytes32 public constant SLASHER_ROLE = keccak256("SLASHER_ROLE");

    // ── State ─────────────────────────────────────────────────────────────
    QuantumVerifier public immutable quantumVerifier;

    uint256 private _nextTokenId;

    /// provider wallet → token id (0 means not registered)
    mapping(address => uint256) public providerTokenId;
    /// token id → provider wallet
    mapping(uint256 => address) public tokenProvider;

    struct ProviderMeta {
        string  gpuVendor;
        string  gpuModel;
        uint16  memoryGb;
        uint32  gpuCores;
        bytes32 attestationCommitment; // BLAKE3 hash from nm-crypto
        bytes32 dil3PubkeyHash;
        bytes32 serialNumberHash;      // BLAKE3(serial) — privacy-preserving
        uint64  registeredAt;
    }
    mapping(uint256 => ProviderMeta) public providerMeta;

    // ── Events ────────────────────────────────────────────────────────────
    event ProviderMinted(
        address indexed provider,
        uint256 indexed tokenId,
        string gpuVendor,
        string gpuModel,
        bytes32 attestationCommitment
    );
    event ProviderSlashed(address indexed provider, uint256 indexed tokenId);

    // ── Errors ────────────────────────────────────────────────────────────
    error SoulBound();
    error AlreadyRegistered();
    error NotRegistered();
    error ProviderNotVerified();

    // ── Constructor ───────────────────────────────────────────────────────

    constructor(
        address admin,
        address minter,
        address _quantumVerifier
    ) ERC721("NeuralMesh Provider", "NM-GPU") {
        _grantRole(DEFAULT_ADMIN_ROLE, admin);
        _grantRole(MINTER_ROLE, minter);
        _grantRole(SLASHER_ROLE, admin);
        quantumVerifier = QuantumVerifier(_quantumVerifier);
        _nextTokenId = 1;
    }

    // ── Mint ──────────────────────────────────────────────────────────────

    /**
     * @notice Mint a soul-bound provider NFT.
     *         Requires a valid QuantumVerifier attestation for `provider`.
     */
    function mintProvider(
        address provider,
        string calldata metadataURI,
        string calldata gpuVendor,
        string calldata gpuModel,
        uint16 memoryGb,
        uint32 gpuCores,
        bytes32 attestationCommitment,
        bytes32 dil3PubkeyHash,
        bytes32 serialNumberHash
    ) external onlyRole(MINTER_ROLE) returns (uint256 tokenId) {
        if (providerTokenId[provider] != 0) revert AlreadyRegistered();
        if (!quantumVerifier.isRegistered(provider)) revert ProviderNotVerified();

        // Verify commitment matches what's in QuantumVerifier
        require(
            quantumVerifier.getCommitment(provider) == attestationCommitment,
            "ProviderNFT: commitment mismatch"
        );

        tokenId = _nextTokenId++;
        _safeMint(provider, tokenId);
        _setTokenURI(tokenId, metadataURI);

        providerTokenId[provider] = tokenId;
        tokenProvider[tokenId] = provider;
        providerMeta[tokenId] = ProviderMeta({
            gpuVendor: gpuVendor,
            gpuModel: gpuModel,
            memoryGb: memoryGb,
            gpuCores: gpuCores,
            attestationCommitment: attestationCommitment,
            dil3PubkeyHash: dil3PubkeyHash,
            serialNumberHash: serialNumberHash,
            registeredAt: uint64(block.timestamp)
        });

        emit ProviderMinted(provider, tokenId, gpuVendor, gpuModel, attestationCommitment);
    }

    // ── Slashing ──────────────────────────────────────────────────────────

    /// Burn provider NFT (slashing). Revokes on QuantumVerifier too.
    function slashProvider(address provider) external onlyRole(SLASHER_ROLE) {
        uint256 tokenId = providerTokenId[provider];
        if (tokenId == 0) revert NotRegistered();

        providerTokenId[provider] = 0;
        delete tokenProvider[tokenId];
        _burn(tokenId);

        // Revoke attestation
        quantumVerifier.revokeAttestation(provider);

        emit ProviderSlashed(provider, tokenId);
    }

    // ── Soul-bound: block transfers ───────────────────────────────────────

    function _update(address to, uint256 tokenId, address auth)
        internal
        override(ERC721)
        returns (address)
    {
        address from = _ownerOf(tokenId);
        // Allow mint (from == 0) and burn (to == 0); block all transfers
        if (from != address(0) && to != address(0)) revert SoulBound();
        return super._update(to, tokenId, auth);
    }

    // ── View helpers ──────────────────────────────────────────────────────

    function isProvider(address wallet) external view returns (bool) {
        return providerTokenId[wallet] != 0;
    }

    // ── Required overrides ────────────────────────────────────────────────

    function tokenURI(uint256 tokenId)
        public view override(ERC721, ERC721URIStorage) returns (string memory)
    {
        return super.tokenURI(tokenId);
    }

    function supportsInterface(bytes4 interfaceId)
        public view override(ERC721, ERC721URIStorage, AccessControl) returns (bool)
    {
        return super.supportsInterface(interfaceId);
    }
}
