import path from "node:path";
import { fileURLToPath } from "node:url";
import { loadTemplatePacks } from "../config/templateLoader.js";
import { registerLifecycleJobs } from "../jobs/lifecycleJobs.js";
import { InMemoryJobRunner } from "../jobs/inMemoryJobRunner.js";
import { MockSandboxRuntimeAdapter } from "../runtime/mockSandboxRuntimeAdapter.js";
import { createHttpService } from "./createHttpService.js";
import { HostedInstanceService } from "../services/hostedInstanceService.js";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const repoRoot = path.resolve(__dirname, "..", "..", "..");

export async function bootstrap({ port = 8787 } = {}) {
  const templatesRoot = path.join(repoRoot, "config", "templates");
  const uiDir = path.join(repoRoot, "control-plane-ui");

  const templatePacks = await loadTemplatePacks(templatesRoot);
  const hostedInstanceService = new HostedInstanceService({
    runtimeAdapter: new MockSandboxRuntimeAdapter(),
    templatePacks
  });

  const jobRunner = new InMemoryJobRunner(registerLifecycleJobs(hostedInstanceService));
  const server = createHttpService({ hostedInstanceService, jobRunner, uiDir });

  await new Promise((resolve, reject) => {
    server.listen(port, () => resolve());
    server.on("error", reject);
  });

  return { server, hostedInstanceService, jobRunner };
}
