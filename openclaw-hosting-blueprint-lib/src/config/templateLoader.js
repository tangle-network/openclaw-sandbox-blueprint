import { readdir, readFile } from "node:fs/promises";
import path from "node:path";

async function existsDir(dirPath) {
  try {
    const entries = await readdir(dirPath);
    return entries.length >= 0;
  } catch {
    return false;
  }
}

export async function loadTemplatePacks(templatesRoot) {
  if (!(await existsDir(templatesRoot))) {
    return [];
  }

  const dirs = (await readdir(templatesRoot, { withFileTypes: true }))
    .filter((entry) => entry.isDirectory())
    .map((entry) => entry.name)
    .sort();

  const packs = [];
  for (const dir of dirs) {
    const dirPath = path.join(templatesRoot, dir);
    const metadata = await readMetadata(dirPath, dir);
    packs.push(metadata);
  }

  return packs;
}

async function readMetadata(dirPath, fallbackId) {
  const metadataPath = path.join(dirPath, "template.json");
  let metadata = {
    id: fallbackId,
    name: fallbackId,
    mode: fallbackId === "custom" ? "custom" : "pack",
    description: "",
    files: {}
  };

  try {
    const raw = await readFile(metadataPath, "utf8");
    metadata = { ...metadata, ...JSON.parse(raw) };
  } catch {
    // Metadata is optional for local development.
  }

  metadata.files = {
    SOUL: await safeRead(path.join(dirPath, "SOUL.md")),
    USER: await safeRead(path.join(dirPath, "USER.md")),
    TOOLS: await safeRead(path.join(dirPath, "TOOLS.md"))
  };

  return metadata;
}

async function safeRead(filePath) {
  try {
    return await readFile(filePath, "utf8");
  } catch {
    return "";
  }
}
