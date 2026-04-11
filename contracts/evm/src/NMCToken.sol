// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.26;

import "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import "@openzeppelin/contracts/token/ERC20/extensions/ERC20Burnable.sol";
import "@openzeppelin/contracts/token/ERC20/extensions/ERC20Permit.sol";
import "@openzeppelin/contracts/token/ERC20/extensions/ERC20Votes.sol";
import "@openzeppelin/contracts/access/AccessControl.sol";
import "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import "./QuantumVerifier.sol";

/**
 * @title NMCToken — NeuralMesh Credit (EVM / Arbitrum)
 *
 * Token design:
 *   - ERC-20 with 18 decimals (1 NMC = 1e18 wei-NMC)
 *   - Mintable only by BRIDGE_ROLE (trusted Solana<>EVM bridge oracle)
 *   - Burnable by anyone (bridge-out to Solana or off-chain)
 *   - ERC-2612 permit for gasless approvals
 *   - ERC-20Votes for future governance
 *   - Max supply cap of 1 billion NMC
 *   - Requires provider to have a valid QuantumVerifier attestation to earn
 *
 * Bridge nonce: each bridge operation has a unique nonce stored here to
 * prevent double-minting across chain re-orgs.
 */
contract NMCToken is ERC20, ERC20Burnable, ERC20Permit, ERC20Votes, AccessControl, ReentrancyGuard {

    // ── Roles ─────────────────────────────────────────────────────────────
    bytes32 public constant BRIDGE_ROLE    = keccak256("BRIDGE_ROLE");
    bytes32 public constant PAUSER_ROLE    = keccak256("PAUSER_ROLE");

    // ── Constants ─────────────────────────────────────────────────────────
    uint256 public constant MAX_SUPPLY = 1_000_000_000 * 1e18; // 1B NMC
    uint256 public constant MAX_BRIDGE_MINT = 1_000_000 * 1e18; // per-tx limit

    // ── State ─────────────────────────────────────────────────────────────
    QuantumVerifier public immutable quantumVerifier;
    bool public paused;

    /// Bridge nonce tracking: nonce → used
    mapping(uint256 => bool) public usedNonces;

    // ── Events ────────────────────────────────────────────────────────────
    event BridgeMint(address indexed recipient, uint256 amount, uint256 nonce);
    event BridgeBurn(address indexed from, uint256 amount, bytes32 destinationAccount);
    event Paused(bool state);

    // ── Errors ────────────────────────────────────────────────────────────
    error MaxSupplyExceeded();
    error MintLimitExceeded();
    error NonceAlreadyUsed();
    error ContractPaused();
    error ProviderNotVerified();
    error ZeroAmount();

    // ── Constructor ───────────────────────────────────────────────────────

    constructor(
        address admin,
        address bridge,
        address _quantumVerifier
    )
        ERC20("NeuralMesh Credit", "NMC")
        ERC20Permit("NeuralMesh Credit")
    {
        _grantRole(DEFAULT_ADMIN_ROLE, admin);
        _grantRole(BRIDGE_ROLE, bridge);
        _grantRole(PAUSER_ROLE, admin);
        quantumVerifier = QuantumVerifier(_quantumVerifier);
    }

    // ── Bridge mint (Solana → EVM or off-chain credits → EVM) ────────────

    /**
     * @notice Called by the bridge oracle to mint NMC on EVM side.
     * @param recipient Address to receive NMC
     * @param amount    Amount in wei-NMC (18 decimals)
     * @param nonce     Unique nonce — must not have been used before
     */
    function bridgeMint(
        address recipient,
        uint256 amount,
        uint256 nonce
    ) external onlyRole(BRIDGE_ROLE) nonReentrant {
        if (paused) revert ContractPaused();
        if (amount == 0) revert ZeroAmount();
        if (amount > MAX_BRIDGE_MINT) revert MintLimitExceeded();
        if (usedNonces[nonce]) revert NonceAlreadyUsed();
        if (totalSupply() + amount > MAX_SUPPLY) revert MaxSupplyExceeded();

        usedNonces[nonce] = true;
        _mint(recipient, amount);
        emit BridgeMint(recipient, amount, nonce);
    }

    /**
     * @notice User burns NMC to bridge back to Solana / off-chain ledger.
     * @param amount              Amount to burn
     * @param destinationAccount  Base58-encoded Solana pubkey as bytes32
     */
    function bridgeBurn(uint256 amount, bytes32 destinationAccount) external nonReentrant {
        if (paused) revert ContractPaused();
        if (amount == 0) revert ZeroAmount();
        _burn(msg.sender, amount);
        emit BridgeBurn(msg.sender, amount, destinationAccount);
    }

    // ── Governance / admin ────────────────────────────────────────────────

    function setPaused(bool _paused) external onlyRole(PAUSER_ROLE) {
        paused = _paused;
        emit Paused(_paused);
    }

    // ── ERC-20Votes overrides (required) ──────────────────────────────────

    function _update(address from, address to, uint256 value)
        internal
        override(ERC20, ERC20Votes)
    {
        super._update(from, to, value);
    }

    function nonces(address owner)
        public
        view
        override(ERC20Permit, Nonces)
        returns (uint256)
    {
        return super.nonces(owner);
    }
}
