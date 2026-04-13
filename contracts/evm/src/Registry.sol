// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.26;

import "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import "@openzeppelin/contracts/access/AccessControl.sol";
import "./NMCToken.sol";

/**
 * @title Registry — NeuralMesh provider staking & anti-Sybil registry
 *
 * Providers must lock a NMC stake to participate in the network.
 * Higher stake → higher trust tier → preferred in job matching.
 *
 * Stake tiers:
 *   Tier 0  — no stake          → blocked
 *   Tier 1  — ≥ 100 NMC stake   → standard  (1× job capacity)
 *   Tier 2  — ≥ 500 NMC stake   → verified  (2× job capacity, priority matching)
 *   Tier 3  — ≥ 2000 NMC stake  → elite     (5× job capacity, first pick)
 *
 * Slashing:
 *   SLASH_ROLE (= oracle / coordinator) calls slash() to deduct stake on
 *   provider misbehaviour (missed heartbeat, invalid output, etc).
 *   Slashed NMC goes to the protocol treasury.
 *
 * Unstaking:
 *   7-day unbonding period — prevents stake-and-flee attacks.
 *   Provider claims NMC after unbonding via claimUnstake().
 */
contract Registry is ReentrancyGuard, AccessControl {

    // ── Roles ─────────────────────────────────────────────────────────────
    bytes32 public constant SLASH_ROLE   = keccak256("SLASH_ROLE");
    bytes32 public constant PAUSER_ROLE  = keccak256("PAUSER_ROLE");

    // ── Constants ─────────────────────────────────────────────────────────
    uint256 public constant TIER1_MIN_STAKE  = 100  * 1e18;  //   100 NMC
    uint256 public constant TIER2_MIN_STAKE  = 500  * 1e18;  //   500 NMC
    uint256 public constant TIER3_MIN_STAKE  = 2000 * 1e18;  //  2000 NMC
    uint256 public constant UNBONDING_PERIOD = 7 days;

    // ── Config ────────────────────────────────────────────────────────────
    NMCToken public immutable nmc;
    address  public treasury;
    bool     public paused;

    // ── Provider record ───────────────────────────────────────────────────
    struct ProviderRecord {
        uint256 stakedNmc;
        uint256 unbondingNmc;
        uint64  unbondingEndsAt;
        uint32  slashCount;
        bool    active;
        /// Off-chain provider ID (Ed25519 pubkey hex, first 20 chars for gas efficiency)
        bytes20 providerIdPrefix;
    }

    mapping(address => ProviderRecord) public providers;
    address[] public providerList;

    // ── Events ────────────────────────────────────────────────────────────
    event Staked(address indexed provider, uint256 amount, uint8 tier);
    event UnstakeQueued(address indexed provider, uint256 amount, uint64 claimAt);
    event Unstaked(address indexed provider, uint256 amount);
    event Slashed(address indexed provider, uint256 amount, string reason);
    event ProviderRegistered(address indexed provider, bytes20 providerIdPrefix);
    event ProviderDeregistered(address indexed provider);

    // ── Errors ────────────────────────────────────────────────────────────
    error InsufficientStake(uint256 have, uint256 need);
    error AlreadyActive();
    error NotActive();
    error UnbondingPending();
    error UnbondingNotComplete();
    error NothingToUnstake();
    error ContractPaused();
    error ZeroAmount();

    // ── Constructor ───────────────────────────────────────────────────────

    constructor(address _nmc, address _treasury, address admin, address slasher) {
        nmc = NMCToken(_nmc);
        treasury = _treasury;
        _grantRole(DEFAULT_ADMIN_ROLE, admin);
        _grantRole(SLASH_ROLE, slasher);
        _grantRole(PAUSER_ROLE, admin);
    }

    // ── Registration + staking ────────────────────────────────────────────

    /**
     * @notice Register as a provider and stake NMC.
     *         Requires at least TIER1_MIN_STAKE (100 NMC).
     * @param amount          Amount of NMC to stake (must be ≥ 100 NMC)
     * @param providerIdPrefix  First 20 bytes of Ed25519 pubkey hex for indexing
     */
    function registerAndStake(
        uint256 amount,
        bytes20 providerIdPrefix
    ) external nonReentrant {
        if (paused) revert ContractPaused();
        if (amount == 0) revert ZeroAmount();
        if (amount < TIER1_MIN_STAKE)
            revert InsufficientStake(amount, TIER1_MIN_STAKE);

        ProviderRecord storage rec = providers[msg.sender];
        if (rec.active) revert AlreadyActive();
        if (rec.unbondingNmc > 0) revert UnbondingPending();

        nmc.transferFrom(msg.sender, address(this), amount);

        rec.stakedNmc = amount;
        rec.active = true;
        rec.providerIdPrefix = providerIdPrefix;
        providerList.push(msg.sender);

        emit ProviderRegistered(msg.sender, providerIdPrefix);
        emit Staked(msg.sender, amount, _tier(amount));
    }

    /**
     * @notice Add more NMC to an existing stake.
     *         Useful for upgrading to a higher tier.
     */
    function addStake(uint256 amount) external nonReentrant {
        if (paused) revert ContractPaused();
        if (amount == 0) revert ZeroAmount();
        ProviderRecord storage rec = providers[msg.sender];
        if (!rec.active) revert NotActive();

        nmc.transferFrom(msg.sender, address(this), amount);
        rec.stakedNmc += amount;

        emit Staked(msg.sender, amount, _tier(rec.stakedNmc));
    }

    // ── Unstaking (7-day unbonding) ───────────────────────────────────────

    /**
     * @notice Queue an unstake. NMC is locked for 7 days before it can be claimed.
     *         Provider must deregister first (or partial unstake down to tier minimum).
     */
    function queueUnstake(uint256 amount) external nonReentrant {
        ProviderRecord storage rec = providers[msg.sender];
        if (!rec.active) revert NotActive();
        if (amount == 0) revert ZeroAmount();
        if (amount > rec.stakedNmc) revert InsufficientStake(rec.stakedNmc, amount);
        if (rec.unbondingNmc > 0) revert UnbondingPending();

        uint256 remaining = rec.stakedNmc - amount;
        // If provider is still active, they must keep at least TIER1_MIN_STAKE
        if (remaining > 0 && remaining < TIER1_MIN_STAKE)
            revert InsufficientStake(remaining, TIER1_MIN_STAKE);

        rec.stakedNmc -= amount;
        rec.unbondingNmc = amount;
        rec.unbondingEndsAt = uint64(block.timestamp + UNBONDING_PERIOD);

        // If stake drops to zero, deactivate provider
        if (rec.stakedNmc == 0) {
            rec.active = false;
            emit ProviderDeregistered(msg.sender);
        }

        emit UnstakeQueued(msg.sender, amount, rec.unbondingEndsAt);
    }

    /**
     * @notice Claim NMC after the unbonding period has elapsed.
     */
    function claimUnstake() external nonReentrant {
        ProviderRecord storage rec = providers[msg.sender];
        if (rec.unbondingNmc == 0) revert NothingToUnstake();
        if (block.timestamp < rec.unbondingEndsAt) revert UnbondingNotComplete();

        uint256 amount = rec.unbondingNmc;
        rec.unbondingNmc = 0;
        rec.unbondingEndsAt = 0;

        nmc.transfer(msg.sender, amount);
        emit Unstaked(msg.sender, amount);
    }

    // ── Slashing ──────────────────────────────────────────────────────────

    /**
     * @notice Slash a provider's stake.
     *         Slashed NMC is sent to the treasury.
     * @param provider  Provider wallet to slash
     * @param amount    Amount to slash (capped at staked amount)
     * @param reason    Human-readable reason (stored in event)
     */
    function slash(
        address provider,
        uint256 amount,
        string calldata reason
    ) external onlyRole(SLASH_ROLE) nonReentrant {
        ProviderRecord storage rec = providers[provider];
        if (!rec.active && rec.stakedNmc == 0) revert NotActive();

        uint256 slashAmount = amount > rec.stakedNmc ? rec.stakedNmc : amount;
        rec.stakedNmc -= slashAmount;
        rec.slashCount++;

        // If stake drops below tier 1 minimum, forcibly deactivate
        if (rec.stakedNmc < TIER1_MIN_STAKE && rec.active) {
            rec.active = false;
            emit ProviderDeregistered(provider);
        }

        if (slashAmount > 0) {
            nmc.transfer(treasury, slashAmount);
        }

        emit Slashed(provider, slashAmount, reason);
    }

    // ── View helpers ──────────────────────────────────────────────────────

    /**
     * @notice Returns the stake tier for a provider (0–3).
     */
    function tierOf(address provider) external view returns (uint8) {
        return _tier(providers[provider].stakedNmc);
    }

    /**
     * @notice Returns true if the provider is active (staked and registered).
     */
    function isActive(address provider) external view returns (bool) {
        return providers[provider].active;
    }

    /**
     * @notice Returns the number of registered providers.
     */
    function providerCount() external view returns (uint256) {
        return providerList.length;
    }

    // ── Admin ─────────────────────────────────────────────────────────────

    function setTreasury(address _treasury) external onlyRole(DEFAULT_ADMIN_ROLE) {
        treasury = _treasury;
    }

    function setPaused(bool _paused) external onlyRole(PAUSER_ROLE) {
        paused = _paused;
    }

    // ── Internal ──────────────────────────────────────────────────────────

    function _tier(uint256 staked) internal pure returns (uint8) {
        if (staked >= TIER3_MIN_STAKE) return 3;
        if (staked >= TIER2_MIN_STAKE) return 2;
        if (staked >= TIER1_MIN_STAKE) return 1;
        return 0;
    }
}
