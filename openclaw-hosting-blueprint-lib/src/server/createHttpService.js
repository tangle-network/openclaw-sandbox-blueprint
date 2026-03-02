import { createServer } from "node:http";
import path from "node:path";
import { readFile } from "node:fs/promises";

function sendJson(res, statusCode, body) {
  res.writeHead(statusCode, {
    "Content-Type": "application/json",
    "Cache-Control": "no-store"
  });
  res.end(JSON.stringify(body));
}

async function readJsonBody(req) {
  let body = "";
  for await (const chunk of req) {
    body += chunk;
  }
  if (!body) {
    return {};
  }
  return JSON.parse(body);
}

async function serveStatic(uiDir, pathname, res) {
  const map = new Map([
    ["/", "index.html"],
    ["/index.html", "index.html"],
    ["/app.js", "app.js"],
    ["/styles.css", "styles.css"]
  ]);

  const fileName = map.get(pathname);
  if (!fileName) {
    return false;
  }

  const filePath = path.join(uiDir, fileName);
  try {
    const content = await readFile(filePath);
    const contentType =
      fileName.endsWith(".html")
        ? "text/html; charset=utf-8"
        : fileName.endsWith(".css")
          ? "text/css; charset=utf-8"
          : "application/javascript; charset=utf-8";

    res.writeHead(200, {
      "Content-Type": contentType,
      "Cache-Control": "no-store"
    });
    res.end(content);
    return true;
  } catch {
    return false;
  }
}

export function createHttpService({ hostedInstanceService, jobRunner, uiDir }) {
  return createServer(async (req, res) => {
    try {
      const url = new URL(req.url ?? "/", "http://localhost");
      const pathname = url.pathname;

      if (req.method === "GET" && pathname === "/health") {
        return sendJson(res, 200, { ok: true });
      }

      // Read-only instance operations are exposed directly as HTTP service endpoints.
      if (req.method === "GET" && pathname === "/instances") {
        return sendJson(res, 200, { instances: hostedInstanceService.listHostedInstances() });
      }

      if (req.method === "GET" && pathname.startsWith("/instances/")) {
        const id = pathname.slice("/instances/".length);
        const instance = hostedInstanceService.getHostedInstance(id);
        if (!instance) {
          return sendJson(res, 404, { error: "instance not found" });
        }
        return sendJson(res, 200, { instance });
      }

      if (req.method === "GET" && pathname === "/templates") {
        return sendJson(res, 200, { templatePacks: hostedInstanceService.listTemplatePacks() });
      }

      if (req.method === "GET" && pathname.startsWith("/jobs/")) {
        const id = pathname.slice("/jobs/".length);
        const job = jobRunner.getJob(id);
        if (!job) {
          return sendJson(res, 404, { error: "job not found" });
        }
        return sendJson(res, 200, { job });
      }

      // State-changing operations are only exposed as jobs.
      if (req.method === "POST" && pathname.startsWith("/jobs/")) {
        const jobType = pathname.slice("/jobs/".length);
        const payload = await readJsonBody(req);
        const job = await jobRunner.runJob(jobType, payload);

        const status = job.status === "failed" ? 400 : 202;
        return sendJson(res, status, { job });
      }

      const served = await serveStatic(uiDir, pathname, res);
      if (served) {
        return;
      }

      return sendJson(res, 404, { error: "not found" });
    } catch (error) {
      const message = error instanceof Error ? error.message : `${error}`;
      return sendJson(res, 500, { error: message });
    }
  });
}
