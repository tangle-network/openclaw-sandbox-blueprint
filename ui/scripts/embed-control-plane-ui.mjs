import { cp, mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const uiRoot = path.resolve(__dirname, '..');
const distDir = path.join(uiRoot, 'dist');
const targetDir = path.resolve(uiRoot, '..', 'control-plane-ui');

await mkdir(targetDir, { recursive: true });

for (const fileName of ['app.js', 'styles.css']) {
  await cp(path.join(distDir, fileName), path.join(targetDir, fileName));
}
await cp(path.join(distDir, 'assets'), path.join(targetDir, 'assets'), { recursive: true });

const indexPath = path.join(distDir, 'index.html');
let html = await readFile(indexPath, 'utf8');
html = html
  .replaceAll('/app.js', '/app.js')
  .replaceAll('/styles.css', '/styles.css');

await writeFile(path.join(targetDir, 'index.html'), html, 'utf8');

console.log(`Embedded control-plane assets copied to ${targetDir}`);
