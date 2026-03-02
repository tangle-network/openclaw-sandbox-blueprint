#!/usr/bin/env node
import { bootstrap } from "../src/server/bootstrap.js";

async function request(method, path, body) {
  const response = await fetch(`http://127.0.0.1:${PORT}${path}`, {
    method,
    headers: { "Content-Type": "application/json" },
    body: body ? JSON.stringify(body) : undefined
  });
  const json = await response.json();
  return { status: response.status, json };
}

const { server } = await bootstrap({ port: 0 });
const address = server.address();
const PORT = address && typeof address === "object" ? address.port : 0;

try {
  const templates = await request("GET", "/templates");
  if (templates.status !== 200 || !templates.json.templatePacks?.length) {
    throw new Error("template packs not loaded");
  }

  const createJob = await request("POST", "/jobs/create-hosted-instance", {
    name: "smoke-instance",
    templatePackId: "discord"
  });

  if (createJob.status !== 202) {
    throw new Error(`create job failed: ${JSON.stringify(createJob.json)}`);
  }

  const instanceId = createJob.json.job.result.id;

  const startJob = await request("POST", "/jobs/start-hosted-instance", { instanceId });
  const stopJob = await request("POST", "/jobs/stop-hosted-instance", { instanceId });
  const deleteJob = await request("POST", "/jobs/delete-hosted-instance", { instanceId });

  if (startJob.status !== 202 || stopJob.status !== 202 || deleteJob.status !== 202) {
    throw new Error("lifecycle jobs did not complete");
  }

  const list = await request("GET", "/instances");
  if (list.status !== 200 || !Array.isArray(list.json.instances)) {
    throw new Error("instance list endpoint failed");
  }

  console.log("smoke test passed");
} finally {
  await new Promise((resolve) => server.close(resolve));
}
