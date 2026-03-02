#!/usr/bin/env node
import { bootstrap } from "../src/server/bootstrap.js";

const port = Number(process.env.PORT ?? 8787);
const { server } = await bootstrap({ port });

console.log(`[openclaw-hosting-blueprint] listening on http://localhost:${port}`);

const shutdown = () => {
  server.close(() => process.exit(0));
};

process.on("SIGINT", shutdown);
process.on("SIGTERM", shutdown);
