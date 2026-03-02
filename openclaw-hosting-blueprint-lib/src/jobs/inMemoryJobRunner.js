import { randomUUID } from "node:crypto";

function nowIso() {
  return new Date().toISOString();
}

export class InMemoryJobRunner {
  constructor(registry) {
    this.registry = registry;
    this.jobs = new Map();
  }

  getJob(jobId) {
    return this.jobs.get(jobId) ?? null;
  }

  async runJob(jobType, payload) {
    const handler = this.registry[jobType];
    if (!handler) {
      throw new Error(`unsupported job type: ${jobType}`);
    }

    const job = {
      id: randomUUID(),
      type: jobType,
      status: "running",
      payload,
      result: null,
      error: null,
      createdAt: nowIso(),
      updatedAt: nowIso()
    };

    this.jobs.set(job.id, job);

    try {
      job.result = await handler(payload);
      job.status = "completed";
      job.updatedAt = nowIso();
    } catch (error) {
      job.status = "failed";
      job.error = error instanceof Error ? error.message : `${error}`;
      job.updatedAt = nowIso();
    }

    return job;
  }
}
