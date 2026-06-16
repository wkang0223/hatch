// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.26;

import "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import "@openzeppelin/contracts/access/AccessControl.sol";
import "./NMCToken.sol";
import "./ProviderNFT.sol";

/**
 * @title Escrow — NeuralMesh job payment escrow
 *
 * Flow:
 *   1. Consumer calls createEscrow() — NMC locked in this contract
 *   2. Provider accepts via lockEscrow() — deadline set
 *   3. Oracle calls releaseEscrow(actualCost) — distributes NMC:
 *        provider gets (actualCost - fee)
 *        platform gets fee
 *        consumer refunded (locked - actualCost)
 *   4. Either party / oracle can cancelEscrow() — full refund
 *
 * Security:
 *   - Only verified providers (ProviderNFT holders) can lock jobs
 *   - Oracle role settles disputes
 *   - ReentrancyGuard on all state-changing calls
 *   - Deadline enforcement: consumer can cancel after deadline passes
 */
contract Escrow is ReentrancyGuard, AccessControl {

    // ── Roles ─────────────────────────────────────────────────────────────
    bytes32 public constant ORACLE_ROLE = keccak256("ORACLE_ROLE");

    // ── State enums ───────────────────────────────────────────────────────
    enum EscrowState { Open, Locked, Settled, Cancelled }

    struct Job {
        address consumer;
        address provider;
        uint256 lockedNmc;
        uint256 pricePerHourNmc;
        uint32  maxDurationSecs;
        uint64  deadline;
        EscrowState state;
        uint256 actualCost;
        uint256 feeCollected;
        uint64  createdAt;
        uint64  settledAt;
    }

    // ── Config ────────────────────────────────────────────────────────────
    NMCToken   public immutable nmc;
    ProviderNFT public immutable providerNft;
    address    public feeCollector;
    uint16     public platformFeeBps; // 800 = 8% (provider receives 92%)
    uint16     public constant MAX_FEE_BPS = 1_000;

    // ── Storage ───────────────────────────────────────────────────────────
    mapping(bytes32 => Job) public jobs;

    // ── Events ────────────────────────────────────────────────────────────
    event EscrowCreated(
        bytes32 indexed jobId,
        address indexed consumer,
        address indexed provider,
        uint256 lockedNmc
    );
    event EscrowLocked(bytes32 indexed jobId, uint64 deadline);
    event EscrowSettled(
        bytes32 indexed jobId,
        uint256 actualCost,
        uint256 fee,
        uint256 consumerRefund
    );
    event EscrowCancelled(bytes32 indexed jobId, uint256 refundAmount);

    // ── Errors ────────────────────────────────────────────────────────────
    error JobNotFound();
    error WrongState(EscrowState current);
    error ProviderNotVerified();
    error CostExceedsLocked();
    error DeadlineNotPassed();
    error CallerNotConsumer();
    error FeeTooHigh();
    error AlreadyExists();

    // ── Constructor ───────────────────────────────────────────────────────

    constructor(
        address _nmc,
        address _providerNft,
        address _feeCollector,
        uint16  _platformFeeBps,
        address admin,
        address oracle
    ) {
        require(_platformFeeBps <= MAX_FEE_BPS, "Escrow: fee too high");
        nmc = NMCToken(_nmc);
        providerNft = ProviderNFT(_providerNft);
        feeCollector = _feeCollector;
        platformFeeBps = _platformFeeBps;
        _grantRole(DEFAULT_ADMIN_ROLE, admin);
        _grantRole(ORACLE_ROLE, oracle);
    }

    // ── Instructions ──────────────────────────────────────────────────────

    /**
     * @notice Consumer locks NMC for a job. Provider must hold a valid ProviderNFT.
     * @param jobId    Off-chain UUID (keccak256 is the on-chain key)
     * @param provider Provider wallet
     * @param amount   NMC to lock (consumer's max budget)
     * @param pricePerHourNmc  Provider's floor price
     * @param maxDurationSecs  Job timeout
     */
    function createEscrow(
        string calldata jobId,
        address provider,
        uint256 amount,
        uint256 pricePerHourNmc,
        uint32  maxDurationSecs
    ) external nonReentrant {
        bytes32 key = keccak256(bytes(jobId));
        if (jobs[key].createdAt != 0) revert AlreadyExists();
        if (!providerNft.isProvider(provider)) revert ProviderNotVerified();

        nmc.transferFrom(msg.sender, address(this), amount);

        jobs[key] = Job({
            consumer: msg.sender,
            provider: provider,
            lockedNmc: amount,
            pricePerHourNmc: pricePerHourNmc,
            maxDurationSecs: maxDurationSecs,
            deadline: 0,
            state: EscrowState.Open,
            actualCost: 0,
            feeCollected: 0,
            createdAt: uint64(block.timestamp),
            settledAt: 0
        });

        emit EscrowCreated(key, msg.sender, provider, amount);
    }

    /// Provider accepts the job — transitions Open → Locked.
    function lockEscrow(string calldata jobId) external nonReentrant {
        bytes32 key = keccak256(bytes(jobId));
        Job storage job = jobs[key];
        if (job.createdAt == 0) revert JobNotFound();
        if (job.state != EscrowState.Open) revert WrongState(job.state);
        require(msg.sender == job.provider, "Escrow: not provider");
        if (!providerNft.isProvider(msg.sender)) revert ProviderNotVerified();

        job.state = EscrowState.Locked;
        job.deadline = uint64(block.timestamp) + job.maxDurationSecs;

        emit EscrowLocked(key, job.deadline);
    }

    /**
     * @notice Oracle settles escrow after job completion.
     *         actualCost must be <= lockedNmc.
     */
    function releaseEscrow(
        string calldata jobId,
        uint256 actualCost
    ) external onlyRole(ORACLE_ROLE) nonReentrant {
        bytes32 key = keccak256(bytes(jobId));
        Job storage job = jobs[key];
        if (job.createdAt == 0) revert JobNotFound();
        if (job.state != EscrowState.Locked) revert WrongState(job.state);
        if (actualCost > job.lockedNmc) revert CostExceedsLocked();

        uint256 fee = (actualCost * platformFeeBps) / 10_000;
        uint256 providerAmount = actualCost - fee;
        uint256 consumerRefund = job.lockedNmc - actualCost;

        job.state = EscrowState.Settled;
        job.actualCost = actualCost;
        job.feeCollected = fee;
        job.settledAt = uint64(block.timestamp);

        if (providerAmount > 0) nmc.transfer(job.provider, providerAmount);
        if (fee > 0)            nmc.transfer(feeCollector, fee);
        if (consumerRefund > 0) nmc.transfer(job.consumer, consumerRefund);

        emit EscrowSettled(key, actualCost, fee, consumerRefund);
    }

    /**
     * @notice Cancel escrow — full refund to consumer.
     *         Can be called by consumer anytime when Open,
     *         or by anyone after deadline has passed when Locked.
     */
    function cancelEscrow(string calldata jobId) external nonReentrant {
        bytes32 key = keccak256(bytes(jobId));
        Job storage job = jobs[key];
        if (job.createdAt == 0) revert JobNotFound();

        if (job.state == EscrowState.Open) {
            if (msg.sender != job.consumer && !hasRole(ORACLE_ROLE, msg.sender)) {
                revert CallerNotConsumer();
            }
        } else if (job.state == EscrowState.Locked) {
            bool deadlinePassed = block.timestamp > job.deadline;
            bool isOracleOrConsumer = msg.sender == job.consumer
                || hasRole(ORACLE_ROLE, msg.sender);
            if (!deadlinePassed && !isOracleOrConsumer) revert DeadlineNotPassed();
        } else {
            revert WrongState(job.state);
        }

        uint256 refund = job.lockedNmc;
        job.state = EscrowState.Cancelled;
        nmc.transfer(job.consumer, refund);

        emit EscrowCancelled(key, refund);
    }

    // ── Admin ─────────────────────────────────────────────────────────────

    function setFee(uint16 feeBps) external onlyRole(DEFAULT_ADMIN_ROLE) {
        if (feeBps > MAX_FEE_BPS) revert FeeTooHigh();
        platformFeeBps = feeBps;
    }

    function setFeeCollector(address collector) external onlyRole(DEFAULT_ADMIN_ROLE) {
        feeCollector = collector;
    }
}
