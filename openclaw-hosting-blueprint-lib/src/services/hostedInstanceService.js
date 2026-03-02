import { randomUUID } from "node:crypto";
import { assertSandboxRuntimeAdapter } from "../runtime/sandboxRuntimeContracts.js";

function nowIso() {
  return new Date().toISOString();
}

export class HostedInstanceService {
  constructor({ runtimeAdapter, templatePacks }) {
    assertSandboxRuntimeAdapter(runtimeAdapter);
    this.runtimeAdapter = runtimeAdapter;
    this.templatePacks = templatePacks;
    this.instances = new Map();
  }

  listTemplatePacks() {
    return this.templatePacks.map((pack) => ({
      id: pack.id,
      name: pack.name,
      mode: pack.mode,
      description: pack.description
    }));
  }

  listHostedInstances() {
    return [...this.instances.values()].sort((a, b) => (a.createdAt < b.createdAt ? 1 : -1));
  }

  getHostedInstance(instanceId) {
    return this.instances.get(instanceId) ?? null;
  }

  async createHostedInstance(payload) {
    const name = `${payload?.name ?? ""}`.trim();
    const templatePackId = `${payload?.templatePackId ?? ""}`.trim();

    if (!name) {
      throw new Error("name is required");
    }

    const templatePack = this.templatePacks.find((pack) => pack.id === templatePackId);
    if (!templatePack) {
      throw new Error(`unknown templatePackId: ${templatePackId}`);
    }

    const id = randomUUID();
    const instance = {
      id,
      name,
      templatePackId,
      runtimeId: null,
      status: "provisioning",
      createdAt: nowIso(),
      updatedAt: nowIso()
    };

    this.instances.set(id, instance);

    const runtime = await this.runtimeAdapter.createHostedInstance({
      instanceId: id,
      templatePack,
      config: payload?.config ?? null
    });

    instance.runtimeId = runtime.runtimeId;
    instance.status = runtime.state;
    instance.updatedAt = nowIso();
    return instance;
  }

  async startHostedInstance(payload) {
    const instance = this.mustGet(payload?.instanceId);
    const runtime = await this.runtimeAdapter.startHostedInstance({ runtimeId: instance.runtimeId });
    instance.status = runtime.state;
    instance.updatedAt = nowIso();
    return instance;
  }

  async stopHostedInstance(payload) {
    const instance = this.mustGet(payload?.instanceId);
    const runtime = await this.runtimeAdapter.stopHostedInstance({ runtimeId: instance.runtimeId });
    instance.status = runtime.state;
    instance.updatedAt = nowIso();
    return instance;
  }

  async deleteHostedInstance(payload) {
    const instance = this.mustGet(payload?.instanceId);
    const runtime = await this.runtimeAdapter.deleteHostedInstance({ runtimeId: instance.runtimeId });
    instance.status = runtime.state;
    instance.deletedAt = nowIso();
    instance.updatedAt = nowIso();
    return instance;
  }

  mustGet(instanceId) {
    const id = `${instanceId ?? ""}`.trim();
    if (!id) {
      throw new Error("instanceId is required");
    }

    const instance = this.instances.get(id);
    if (!instance) {
      throw new Error(`unknown instance: ${id}`);
    }

    return instance;
  }
}
