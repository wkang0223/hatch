// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.26;

import "forge-std/Test.sol";
import "../src/QuantumVerifier.sol";
import "../src/NMCToken.sol";
import "../src/ProviderNFT.sol";
import "../src/Escrow.sol";
import "../src/Registry.sol";

/**
 * @title NeuralMesh test suite
 *
 * Run:  forge test -vvv
 * Fuzz: forge test -vvv --fuzz-runs 10000
 */
contract NeuralMeshTest is Test {

    // ── Actors ─────────────────────────────────────────────────────────────
    address admin      = makeAddr("admin");
    address bridge     = makeAddr("bridge");
    address oracle     = makeAddr("oracle");
    address treasury   = makeAddr("treasury");
    address consumer   = makeAddr("consumer");
    address provider   = makeAddr("provider");
    address attacker   = makeAddr("attacker");

    // ── Contracts ──────────────────────────────────────────────────────────
    QuantumVerifier qv;
    NMCToken        nmc;
    ProviderNFT     nft;
    Escrow          escrow;
    Registry        registry;

    // ── Constants ──────────────────────────────────────────────────────────
    uint256 constant MINT_AMOUNT   = 10_000 * 1e18;
    uint256 constant STAKE_AMOUNT  = 500  * 1e18;  // Tier 2
    uint256 constant ESCROW_AMOUNT = 100  * 1e18;
    string  constant JOB_ID        = "job-test-123";

    bytes32 constant FAKE_COMMITMENT     = keccak256("commitment");
    bytes32 constant FAKE_DIL3_HASH      = keccak256("dil3pubkey");
    bytes   constant FAKE_ED25519_PUBKEY = abi.encodePacked(bytes32(uint256(0xdeadbeef)));

    // ── Setup ──────────────────────────────────────────────────────────────

    function setUp() public {
        // Deploy QuantumVerifier with oracle as single oracle, threshold=1 (test simplicity)
        address[] memory oracles = new address[](1);
        oracles[0] = oracle;

        vm.startPrank(admin);
        qv = new QuantumVerifier(oracles, 1);
        nmc = new NMCToken(admin, bridge, address(qv));
        nft = new ProviderNFT(admin, oracle, address(qv));
        vm.stopPrank();

        registry = new Registry(address(nmc), treasury, admin, oracle);

        escrow = new Escrow(
            address(nmc),
            address(nft),
            treasury,
            800, // 8% fee
            admin,
            oracle
        );

        // Bridge mint NMC to consumer and provider
        vm.prank(bridge);
        nmc.bridgeMint(consumer, MINT_AMOUNT, 1);

        vm.prank(bridge);
        nmc.bridgeMint(provider, MINT_AMOUNT, 2);

        // Register provider attestation in QV
        vm.prank(oracle);
        qv.approveAttestation(
            provider,
            FAKE_COMMITMENT,
            "apple",
            "M4 Pro",
            FAKE_DIL3_HASH,
            FAKE_ED25519_PUBKEY
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    // QuantumVerifier tests
    // ═══════════════════════════════════════════════════════════════════════

    function test_QV_RegistersAttestation() public view {
        assertTrue(qv.isRegistered(provider));
        assertEq(qv.getCommitment(provider), FAKE_COMMITMENT);
    }

    function test_QV_NonOracleCannotApprove() public {
        vm.prank(attacker);
        vm.expectRevert("QV: not oracle");
        qv.approveAttestation(
            attacker,
            bytes32(uint256(1)),
            "nvidia",
            "RTX 4090",
            FAKE_DIL3_HASH,
            FAKE_ED25519_PUBKEY
        );
    }

    function test_QV_OwnerCanRevokeAttestation() public {
        vm.prank(admin);
        qv.revokeAttestation(provider);
        assertFalse(qv.isRegistered(provider));
    }

    function test_QV_RevokedProviderIsBlocked() public {
        vm.prank(admin);
        qv.revokeAttestation(provider);

        // Re-approving a revoked provider should revert
        vm.prank(oracle);
        vm.expectRevert("QV: provider revoked");
        qv.approveAttestation(
            provider,
            FAKE_COMMITMENT,
            "apple",
            "M4 Pro",
            FAKE_DIL3_HASH,
            FAKE_ED25519_PUBKEY
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    // NMCToken tests
    // ═══════════════════════════════════════════════════════════════════════

    function test_NMC_BridgeMint() public view {
        assertEq(nmc.balanceOf(consumer), MINT_AMOUNT);
        assertEq(nmc.balanceOf(provider), MINT_AMOUNT);
    }

    function test_NMC_BridgeMintNonceReplay() public {
        vm.prank(bridge);
        vm.expectRevert(NMCToken.NonceAlreadyUsed.selector);
        nmc.bridgeMint(consumer, 100 * 1e18, 1); // nonce 1 already used in setUp
    }

    function test_NMC_BridgeBurn() public {
        uint256 burnAmount = 50 * 1e18;
        uint256 balBefore  = nmc.balanceOf(consumer);

        vm.prank(consumer);
        nmc.bridgeBurn(burnAmount, bytes32(uint256(uint160(consumer))));

        assertEq(nmc.balanceOf(consumer), balBefore - burnAmount);
    }

    function test_NMC_MaxSupplyCap() public {
        // Mint up to max supply and verify the next mint reverts
        uint256 remaining = nmc.MAX_SUPPLY() - nmc.totalSupply();
        // MAX_BRIDGE_MINT = 1M NMC; mint in chunks
        uint256 nonce = 100;
        while (remaining > 0) {
            uint256 chunk = remaining > nmc.MAX_BRIDGE_MINT()
                ? nmc.MAX_BRIDGE_MINT()
                : remaining;
            vm.prank(bridge);
            nmc.bridgeMint(admin, chunk, nonce++);
            remaining -= chunk;
        }
        // Now any mint should revert
        vm.prank(bridge);
        vm.expectRevert(NMCToken.MaxSupplyExceeded.selector);
        nmc.bridgeMint(admin, 1, nonce);
    }

    function test_NMC_NonBridgeCannotMint() public {
        vm.prank(attacker);
        vm.expectRevert();
        nmc.bridgeMint(attacker, 1000 * 1e18, 999);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // ProviderNFT tests
    // ═══════════════════════════════════════════════════════════════════════

    function test_NFT_MintProvider() public {
        vm.prank(oracle);
        uint256 tokenId = nft.mintProvider(
            provider,
            "ipfs://Qm...",
            "apple",
            "M4 Pro",
            64, // 64 GB
            20, // 20 GPU cores
            FAKE_COMMITMENT,
            FAKE_DIL3_HASH,
            keccak256("serial")
        );
        assertTrue(nft.isProvider(provider));
        assertEq(nft.providerTokenId(provider), tokenId);
    }

    function test_NFT_SoulBound() public {
        // Mint first
        vm.prank(oracle);
        uint256 tokenId = nft.mintProvider(
            provider,
            "ipfs://Qm...",
            "apple",
            "M4 Pro",
            64, 20,
            FAKE_COMMITMENT,
            FAKE_DIL3_HASH,
            keccak256("serial")
        );

        // Attempt transfer should revert
        vm.prank(provider);
        vm.expectRevert(ProviderNFT.SoulBound.selector);
        nft.transferFrom(provider, attacker, tokenId);
    }

    function test_NFT_SlashBurns() public {
        vm.prank(oracle);
        nft.mintProvider(
            provider, "ipfs://Qm...", "apple", "M4 Pro",
            64, 20, FAKE_COMMITMENT, FAKE_DIL3_HASH, keccak256("serial")
        );
        assertTrue(nft.isProvider(provider));

        vm.prank(admin);
        nft.slashProvider(provider);
        assertFalse(nft.isProvider(provider));
    }

    function test_NFT_CommitmentMismatchReverts() public {
        // Minting with a wrong commitment should revert
        bytes32 wrongCommitment = keccak256("wrong");
        vm.prank(oracle);
        vm.expectRevert("ProviderNFT: commitment mismatch");
        nft.mintProvider(
            provider, "ipfs://Qm...", "apple", "M4 Pro",
            64, 20, wrongCommitment, FAKE_DIL3_HASH, keccak256("serial")
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Registry tests
    // ═══════════════════════════════════════════════════════════════════════

    function test_Registry_StakeAndTier() public {
        vm.startPrank(provider);
        nmc.approve(address(registry), STAKE_AMOUNT);
        registry.registerAndStake(STAKE_AMOUNT, bytes20(provider));
        vm.stopPrank();

        assertTrue(registry.isActive(provider));
        assertEq(registry.tierOf(provider), 2); // 500 NMC = tier 2
    }

    function test_Registry_BelowMinimumReverts() public {
        uint256 tooLittle = 50 * 1e18; // below 100 NMC tier 1 minimum
        vm.startPrank(provider);
        nmc.approve(address(registry), tooLittle);
        vm.expectRevert(
            abi.encodeWithSelector(Registry.InsufficientStake.selector, tooLittle, registry.TIER1_MIN_STAKE())
        );
        registry.registerAndStake(tooLittle, bytes20(provider));
        vm.stopPrank();
    }

    function test_Registry_UnbondingPeriod() public {
        vm.startPrank(provider);
        nmc.approve(address(registry), STAKE_AMOUNT);
        registry.registerAndStake(STAKE_AMOUNT, bytes20(provider));

        // Queue full unstake (deactivates provider)
        registry.queueUnstake(STAKE_AMOUNT);
        assertFalse(registry.isActive(provider));

        // Try to claim before period ends
        vm.expectRevert(Registry.UnbondingNotComplete.selector);
        registry.claimUnstake();
        vm.stopPrank();

        // Warp 7 days + 1 second
        vm.warp(block.timestamp + 7 days + 1);

        uint256 balBefore = nmc.balanceOf(provider);
        vm.prank(provider);
        registry.claimUnstake();
        assertEq(nmc.balanceOf(provider), balBefore + STAKE_AMOUNT);
    }

    function test_Registry_Slash() public {
        uint256 stake = 500 * 1e18;
        vm.startPrank(provider);
        nmc.approve(address(registry), stake);
        registry.registerAndStake(stake, bytes20(provider));
        vm.stopPrank();

        uint256 treasuryBefore = nmc.balanceOf(treasury);
        uint256 slashAmt = 100 * 1e18;

        vm.prank(oracle);
        registry.slash(provider, slashAmt, "missed heartbeat");

        assertEq(nmc.balanceOf(treasury), treasuryBefore + slashAmt);
        (uint256 remainingStake,,,,,) = registry.providers(provider);
        assertEq(remainingStake, stake - slashAmt);
    }

    function test_Registry_SlashBelowTierDeactivates() public {
        uint256 stake = 100 * 1e18; // exactly tier 1 minimum
        vm.startPrank(provider);
        nmc.approve(address(registry), stake);
        registry.registerAndStake(stake, bytes20(provider));
        vm.stopPrank();

        // Slash 1 wei below tier 1 → should deactivate
        vm.prank(oracle);
        registry.slash(provider, 1, "test");
        assertFalse(registry.isActive(provider));
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Escrow tests
    // ═══════════════════════════════════════════════════════════════════════

    function _mintNftForProvider() internal {
        vm.prank(oracle);
        nft.mintProvider(
            provider, "ipfs://Qm...", "apple", "M4 Pro",
            64, 20, FAKE_COMMITMENT, FAKE_DIL3_HASH, keccak256("serial")
        );
    }

    function test_Escrow_FullLifecycle() public {
        _mintNftForProvider();

        // Consumer approves and creates escrow
        vm.startPrank(consumer);
        nmc.approve(address(escrow), ESCROW_AMOUNT);
        escrow.createEscrow(JOB_ID, provider, ESCROW_AMOUNT, 10 * 1e18, 3600);
        vm.stopPrank();

        // Provider locks the job
        vm.prank(provider);
        escrow.lockEscrow(JOB_ID);

        uint256 providerBalBefore  = nmc.balanceOf(provider);
        uint256 consumerBalBefore  = nmc.balanceOf(consumer);
        uint256 treasuryBalBefore  = nmc.balanceOf(treasury);

        // Actual cost = 80 NMC (< 100 locked)
        uint256 actualCost = 80 * 1e18;
        uint256 fee = (actualCost * 800) / 10_000; // 8% = 6.4 NMC
        uint256 providerExpected = actualCost - fee;
        uint256 consumerRefund   = ESCROW_AMOUNT - actualCost;

        vm.prank(oracle);
        escrow.releaseEscrow(JOB_ID, actualCost);

        assertEq(nmc.balanceOf(provider),  providerBalBefore  + providerExpected);
        assertEq(nmc.balanceOf(consumer),  consumerBalBefore  + consumerRefund);
        assertEq(nmc.balanceOf(treasury),  treasuryBalBefore  + fee);
    }

    function test_Escrow_CancelWhenOpen() public {
        _mintNftForProvider();

        vm.startPrank(consumer);
        nmc.approve(address(escrow), ESCROW_AMOUNT);
        escrow.createEscrow(JOB_ID, provider, ESCROW_AMOUNT, 10 * 1e18, 3600);
        vm.stopPrank();

        uint256 balBefore = nmc.balanceOf(consumer);
        vm.prank(consumer);
        escrow.cancelEscrow(JOB_ID);

        assertEq(nmc.balanceOf(consumer), balBefore + ESCROW_AMOUNT);
    }

    function test_Escrow_CancelAfterDeadline() public {
        _mintNftForProvider();

        vm.startPrank(consumer);
        nmc.approve(address(escrow), ESCROW_AMOUNT);
        escrow.createEscrow(JOB_ID, provider, ESCROW_AMOUNT, 10 * 1e18, 3600);
        vm.stopPrank();

        vm.prank(provider);
        escrow.lockEscrow(JOB_ID);

        // Warp past deadline
        vm.warp(block.timestamp + 3601);

        uint256 balBefore = nmc.balanceOf(consumer);
        vm.prank(consumer); // consumer cancels after deadline
        escrow.cancelEscrow(JOB_ID);

        assertEq(nmc.balanceOf(consumer), balBefore + ESCROW_AMOUNT);
    }

    function test_Escrow_NonProviderCannotLock() public {
        _mintNftForProvider();

        vm.startPrank(consumer);
        nmc.approve(address(escrow), ESCROW_AMOUNT);
        escrow.createEscrow(JOB_ID, provider, ESCROW_AMOUNT, 10 * 1e18, 3600);
        vm.stopPrank();

        vm.prank(attacker); // not the provider
        vm.expectRevert("Escrow: not provider");
        escrow.lockEscrow(JOB_ID);
    }

    function test_Escrow_ActualCostExceedsLockedReverts() public {
        _mintNftForProvider();

        vm.startPrank(consumer);
        nmc.approve(address(escrow), ESCROW_AMOUNT);
        escrow.createEscrow(JOB_ID, provider, ESCROW_AMOUNT, 10 * 1e18, 3600);
        vm.stopPrank();

        vm.prank(provider);
        escrow.lockEscrow(JOB_ID);

        vm.prank(oracle);
        vm.expectRevert(Escrow.CostExceedsLocked.selector);
        escrow.releaseEscrow(JOB_ID, ESCROW_AMOUNT + 1);
    }

    function test_Escrow_DuplicateJobReverts() public {
        _mintNftForProvider();

        vm.startPrank(consumer);
        nmc.approve(address(escrow), ESCROW_AMOUNT * 2);
        escrow.createEscrow(JOB_ID, provider, ESCROW_AMOUNT, 10 * 1e18, 3600);
        vm.expectRevert(Escrow.AlreadyExists.selector);
        escrow.createEscrow(JOB_ID, provider, ESCROW_AMOUNT, 10 * 1e18, 3600);
        vm.stopPrank();
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Fuzz tests
    // ═══════════════════════════════════════════════════════════════════════

    /// Fuzz: any valid escrow amount ≥ 1 NMC and actualCost ≤ amount
    /// must always settle correctly (no underflow, no overcharge).
    function testFuzz_Escrow_Settlement(uint256 amount, uint256 actualCost) public {
        amount = bound(amount, 1 * 1e18, MINT_AMOUNT / 2);
        actualCost = bound(actualCost, 0, amount);
        _mintNftForProvider();

        vm.prank(bridge);
        nmc.bridgeMint(consumer, amount, 9999);

        vm.startPrank(consumer);
        nmc.approve(address(escrow), amount);
        escrow.createEscrow("fuzz-job", provider, amount, 1e18, 3600);
        vm.stopPrank();

        vm.prank(provider);
        escrow.lockEscrow("fuzz-job");

        uint256 fee            = (actualCost * 800) / 10_000;
        uint256 providerExpect = actualCost - fee;
        uint256 consumerRefund = amount - actualCost;

        uint256 pb = nmc.balanceOf(provider);
        uint256 cb = nmc.balanceOf(consumer);
        uint256 tb = nmc.balanceOf(treasury);

        vm.prank(oracle);
        escrow.releaseEscrow("fuzz-job", actualCost);

        assertEq(nmc.balanceOf(provider), pb + providerExpect);
        assertEq(nmc.balanceOf(consumer), cb + consumerRefund);
        assertEq(nmc.balanceOf(treasury), tb + fee);
    }

    /// Fuzz: any stake amount below tier 1 min must revert registration.
    function testFuzz_Registry_BelowMinimumAlwaysReverts(uint256 stake) public {
        stake = bound(stake, 0, registry.TIER1_MIN_STAKE() - 1);
        if (stake == 0) {
            // 0 amount → ZeroAmount revert
            vm.prank(provider);
            vm.expectRevert();
            registry.registerAndStake(stake, bytes20(provider));
        } else {
            vm.startPrank(provider);
            nmc.approve(address(registry), stake);
            vm.expectRevert();
            registry.registerAndStake(stake, bytes20(provider));
            vm.stopPrank();
        }
    }
}
