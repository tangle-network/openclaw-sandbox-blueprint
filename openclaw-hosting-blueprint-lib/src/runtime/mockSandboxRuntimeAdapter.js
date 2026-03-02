function delay(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

export class MockSandboxRuntimeAdapter {
  async createHostedInstance({ instanceId }) {
    await delay(40);
    return {
      runtimeId: `runtime-${instanceId}`,
      state: "stopped"
    };
  }

  async startHostedInstance() {
    await delay(30);
    return { state: "running" };
  }

  async stopHostedInstance() {
    await delay(30);
    return { state: "stopped" };
  }

  async deleteHostedInstance() {
    await delay(20);
    return { state: "deleted" };
  }
}
