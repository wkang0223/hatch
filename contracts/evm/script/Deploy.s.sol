// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.26;

import "forge-std/Script.sol";
import "../src/QuantumVerifier.sol";
import "../src/NMCToken.sol";
import "../src/ProviderNFT.sol";
import "../src/Escrow.sol";
import "../src/Registry.sol";

/**
 * @title Deploy — NeuralMesh full contract suite deployment
 *
 * Deploy order:
 *   1. QuantumVerifier (oracle multisig — no deps)
 *   2. NMCToken (ERC-20, bridge minter; depends on QV)
 *   3. ProviderNFT (soul-bound NFT; depends on QV)
 *   4. Registry (provider staking; depends on NMC)
 *   5. Escrow (job payment; depends on NMC + ProviderNFT)
 *
 * Usage:
 *   # Arbitrum Sepolia (testnet)
 *   forge script script/Deploy.s.sol:Deploy \
 *     --rpc-url arbitrum_sepolia \
 *     --broadcast --verify -vvvv
 *
 *   # Arbitrum One (mainnet)
 *   forge script script/Deploy.s.sol:Deploy \
 *     --rpc-url arbitrum_one \
 *     --broadcast --verify -vvvv
 *
 * Environment variables:
 *   PRIVATE_KEY      — deployer private key (needs ETH for gas)
 *   BRIDGE_ADDRESS   — bridge oracle wallet (mints/burns NMC)
 *   ORACLE_ADDRESS   — settlement oracle wallet (releases escrow, slashes)
 *   ORACLE_1/2/3     — QuantumVerifier oracle multisig members
 *   FEE_COLLECTOR    — treasury / fee recipient wallet
 */
contract Deploy is Script {
    function run() external {
        uint256 deployerKey  = vm.envUint("PRIVATE_KEY");
        address deployer     = vm.addr(deployerKey);
        address bridge       = vm.envOr("BRIDGE_ADDRESS",  deployer);
        address oracle       = vm.envOr("ORACLE_ADDRESS",  deployer);
        address feeCollector = vm.envOr("FEE_COLLECTOR",   deployer);

        // QV oracle multisig (2-of-3 default; all default to deployer for local dev)
        address[] memory qvOracles = new address[](3);
        qvOracles[0] = vm.envOr("ORACLE_1", deployer);
        qvOracles[1] = vm.envOr("ORACLE_2", deployer);
        qvOracles[2] = vm.envOr("ORACLE_3", deployer);
        uint8 threshold = 2; // 2-of-3

        console2.log("=== NeuralMesh EVM Deployment ===");
        console2.log("Deployer:    ", deployer);
        console2.log("Bridge:      ", bridge);
        console2.log("Oracle:      ", oracle);
        console2.log("Fee collect: ", feeCollector);

        vm.startBroadcast(deployerKey);

        // 1. QuantumVerifier — post-quantum attestation commitment registry
        QuantumVerifier qv = new QuantumVerifier(qvOracles, threshold);
        console2.log("QuantumVerifier: ", address(qv));

        // 2. NMCToken — ERC-20 with bridge mint/burn + max supply cap
        NMCToken nmc = new NMCToken(deployer, bridge, address(qv));
        console2.log("NMCToken:        ", address(nmc));

        // 3. ProviderNFT — soul-bound GPU provider identity (non-transferable)
        ProviderNFT nft = new ProviderNFT(deployer, oracle, address(qv));
        console2.log("ProviderNFT:     ", address(nft));

        // 4. Registry — provider staking (anti-Sybil, tier system)
        Registry registry = new Registry(
            address(nmc),
            feeCollector,   // treasury receives slashed NMC
            deployer,       // admin
            oracle          // slasher
        );
        console2.log("Registry:        ", address(registry));

        // 5. Escrow — trustless job payment (8% platform fee; provider 92%)
        Escrow escrow = new Escrow(
            address(nmc),
            address(nft),
            feeCollector,
            800,            // 800 bps = 8%
            deployer,       // admin
            oracle          // settlement oracle
        );
        console2.log("Escrow:          ", address(escrow));

        vm.stopBroadcast();

        // Print env vars for dashboard + coordinator
        console2.log("\n--- .env.local (dashboard) ---");
        console2.log("NEXT_PUBLIC_CHAIN_ID=", block.chainid);
        console2.log("NEXT_PUBLIC_NMC_ADDRESS=", address(nmc));
        console2.log("NEXT_PUBLIC_ESCROW_ADDRESS=", address(escrow));
        console2.log("NEXT_PUBLIC_REGISTRY_ADDRESS=", address(registry));
        console2.log("NEXT_PUBLIC_PROVIDER_NFT_ADDRESS=", address(nft));
        console2.log("NEXT_PUBLIC_QV_ADDRESS=", address(qv));

        console2.log("\n--- coordinator / ledger env ---");
        console2.log("NM_NMC_ADDRESS=", address(nmc));
        console2.log("NM_ESCROW_ADDRESS=", address(escrow));
        console2.log("NM_REGISTRY_ADDRESS=", address(registry));

        console2.log("\nPlatform fee: 8% (provider: 92%)");
        console2.log("QV threshold: 2-of-3 oracle multisig");
    }
}
