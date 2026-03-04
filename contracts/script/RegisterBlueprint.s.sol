// SPDX-License-Identifier: UNLICENSE
pragma solidity ^0.8.26;

import "forge-std/Script.sol";
import "tnt-core/libraries/Types.sol";

/// @notice Minimal interface for Tangle blueprint registration.
interface ITangle {
    function createBlueprint(Types.BlueprintDefinition calldata def) external returns (uint64);
}

/// @title RegisterBlueprint
/// @notice Registers the OpenClaw instance and TEE instance blueprints on local Tangle.
/// @dev Run via:
/// forge script contracts/script/RegisterBlueprint.s.sol --rpc-url $RPC_URL --broadcast --slow
contract RegisterBlueprint is Script {
    // Anvil deterministic deployer account.
    uint256 constant DEPLOYER_KEY = 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80;

    // Local Tangle snapshot addresses.
    address constant TANGLE = 0xCf7Ed3AccA5a467e9e704C703E8D87F634fB0Fc9;

    function run() external {
        ITangle tangle = ITangle(TANGLE);

        vm.startBroadcast(DEPLOYER_KEY);
        uint64 instanceId = tangle.createBlueprint(_buildInstanceDefinition(false));
        uint64 teeInstanceId = tangle.createBlueprint(_buildInstanceDefinition(true));
        vm.stopBroadcast();

        console.log("DEPLOY_INSTANCE_BLUEPRINT_ID=%s", vm.toString(instanceId));
        console.log("DEPLOY_TEE_INSTANCE_BLUEPRINT_ID=%s", vm.toString(teeInstanceId));
    }

    function _buildJobs() internal pure returns (Types.JobDefinition[] memory jobs) {
        jobs = new Types.JobDefinition[](4);
        jobs[0] = Types.JobDefinition("create", "Provision a new claw instance", "", "", "");
        jobs[1] = Types.JobDefinition("start", "Start a claw instance", "", "", "");
        jobs[2] = Types.JobDefinition("stop", "Stop a claw instance", "", "", "");
        jobs[3] = Types.JobDefinition("delete", "Delete a claw instance", "", "", "");
    }

    function _buildInstanceDefinition(bool tee) internal pure returns (Types.BlueprintDefinition memory def) {
        def.metadataUri = "https://github.com/tangle-network/openclaw-sandbox-blueprint";
        def.manager = address(0);
        def.masterManagerRevision = 0;
        def.hasConfig = true;

        def.config = Types.BlueprintConfig({
            membership: Types.MembershipModel.Fixed,
            pricing: Types.PricingModel.EventDriven,
            minOperators: 1,
            maxOperators: 10,
            subscriptionRate: 0,
            subscriptionInterval: 0,
            eventRate: 0
        });

        def.metadata = Types.BlueprintMetadata({
            name: tee ? "OpenClaw TEE Instance Blueprint" : "OpenClaw Instance Blueprint",
            description: tee
                ? "TEE execution target for OpenClaw/NanoClaw/IronClaw instance runtime"
                : "OpenClaw/NanoClaw/IronClaw instance runtime blueprint",
            author: "Tangle",
            category: "AI/Compute",
            codeRepository: "https://github.com/tangle-network/openclaw-sandbox-blueprint",
            logo: "",
            website: "https://tangle.network",
            license: "UNLICENSE",
            profilingData: ""
        });

        def.jobs = _buildJobs();
        def.registrationSchema = "";
        def.requestSchema = "";

        def.sources = new Types.BlueprintSource[](1);
        Types.BlueprintBinary[] memory bins = new Types.BlueprintBinary[](1);
        bins[0] = Types.BlueprintBinary({
            arch: Types.BlueprintArchitecture.Amd64,
            os: Types.BlueprintOperatingSystem.Linux,
            name: tee ? "openclaw-tee-instance-blueprint" : "openclaw-instance-blueprint",
            sha256: bytes32(uint256(0xdeadbeef))
        });
        def.sources[0] = Types.BlueprintSource({
            kind: Types.BlueprintSourceKind.Native,
            container: Types.ImageRegistrySource("", "", ""),
            wasm: Types.WasmSource(Types.WasmRuntime.Unknown, Types.BlueprintFetcherKind.None, "", ""),
            native: Types.NativeSource(
                Types.BlueprintFetcherKind.None,
                tee
                    ? "file:///target/release/openclaw-tee-instance-blueprint"
                    : "file:///target/release/openclaw-instance-blueprint",
                tee ? "./target/release/openclaw-tee-instance-blueprint" : "./target/release/openclaw-instance-blueprint"
            ),
            testing: Types.TestingSource(
                tee ? "openclaw-tee-instance-blueprint-bin" : "openclaw-instance-blueprint-bin",
                tee ? "openclaw-tee-instance-blueprint" : "openclaw-instance-blueprint",
                "."
            ),
            binaries: bins
        });

        def.supportedMemberships = new Types.MembershipModel[](1);
        def.supportedMemberships[0] = Types.MembershipModel.Fixed;
    }
}
