export const SANDBOX_RUNTIME_CONTRACT_VERSION = "0.1.0";

export const SANDBOX_RUNTIME_CONTRACT_METHODS = Object.freeze([
  "createHostedInstance",
  "startHostedInstance",
  "stopHostedInstance",
  "deleteHostedInstance"
]);

export function assertSandboxRuntimeAdapter(adapter) {
  for (const method of SANDBOX_RUNTIME_CONTRACT_METHODS) {
    if (typeof adapter[method] !== "function") {
      throw new Error(`sandbox-runtime adapter is missing method: ${method}`);
    }
  }
}
