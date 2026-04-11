// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.26;

import "forge-std/Script.sol";
import "../src/QuantumVerifier.sol";
import "../src/NMCToken.sol";
import "../src/ProviderNFT.sol";
import "../src/Escrow.sol";

/**
 * Deploy order:
 *   1. QuantumVerifier (oracle multisig)
 *   2. NMCToken (ERC-20, bridge minter)
 *   3. ProviderNFT (soul-bound NFT, minter = deployer initially)
 *   4. Escrow (job payment, oracle = deployer initially)
 *
 * Usage:
 *   forge script script/Deploy.s.sol:Deploy \
 *     --rpc-url arbitrum_sepolia \
 *     --broadcast \
 *     --verify \
 *     -vvvv
 */
contract Deploy is Script {
    function run() external {
        uint256 deployerKey = vm.envUint("PRIVATE_KEY");
        address deployer    = vm.addr(deployerKey);
        address bridge      = vm.envOr("BRIDGE_ADDRESS", deployer);
        address feeCollector = vm.envOr("FEE_COLLECTOR", deployer);

        // Oracle multisig addresses (set via env for production)
        address[] memory oracles = new address[](3);
        oracles[0] = vm.envOr("ORACLE_1", deployer);
        oracles[1] = vm.envOr("ORACLE_2", deployer);
        oracles[2] = vm.envOr("ORACLE_3", deployer);
        uint8 threshold = 2; // 2-of-3

        vm.startBroadcast(deployerKey);

        // 1. Deploy QuantumVerifier (2-of-3 oracle multisig)
        QuantumVerifier qv = new QuantumVerifier(oracles, threshold);
        console2.log("QuantumVerifier:", address(qv));

        // 2. Deploy NMCToken
        NMCToken nmc = new NMCToken(deployer, bridge, address(qv));
        console2.log("NMCToken:", address(nmc));

        // 3. Deploy ProviderNFT
        ProviderNFT nft = new ProviderNFT(deployer, deployer, address(qv));
        console2.log("ProviderNFT:", address(nft));

        // 4. Deploy Escrow (8% platform fee; provider receives 92%)
        Escrow escrow = new Escrow(
            address(nmc),
            address(nft),
            feeCollector,
            800, // 8%
            deployer,
            deployer  // oracle = deployer initially; replace with multisig
        );
        console2.log("Escrow:", address(escrow));

        vm.stopBroadcast();

        // Print summary
        console2.log("\n=== NeuralMesh EVM Deployment ===");
        console2.log("Network:          Arbitrum");
        console2.log("QuantumVerifier:  ", address(qv));
        console2.log("NMCToken:         ", address(nmc));
        console2.log("ProviderNFT:      ", address(nft));
        console2.log("Escrow:           ", address(escrow));
        console2.log("Platform fee:     8% (provider: 92%)");
        console2.log("Oracle threshold: 2-of-3");
    }
}
