import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { mkdir, mkdtemp, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';
import { edgePath, serveFile } from './preview.mjs';

const output = process.argv[2];
if (!output) throw new Error('Usage: node tools/preview/verify-render.mjs <evidence-directory>');
await mkdir(output, { recursive: true });
const profile = await mkdtemp(path.join(tmpdir(), 'herdr-preview-render-'));
const preview = await serveFile(fileURLToPath(new URL('./sample.html', import.meta.url)));
try {
  const { stdout } = await promisify(execFile)(edgePath(), [
    '--headless=new', '--disable-gpu', '--no-first-run', '--disable-extensions', '--disable-background-mode',
    `--user-data-dir=${profile}`, '--window-size=640,800', '--virtual-time-budget=1500',
    `--screenshot=${path.resolve(output, 'preview.png')}`, '--dump-dom', preview.url+'#verify',
  ], { windowsHide: true, timeout: 30000, maxBuffer: 2 * 1024 * 1024 });
  assert.match(stdout, /<output id="result">HTML, CSS and JavaScript work<\/output>/);
  assert.match(stdout, /data-background="rgb\(19, 32, 43\)"/);
  await writeFile(path.join(output, 'render.html'), stdout);
  console.log('PASS: Edge rendered local HTML and CSS; JavaScript button interaction changed the page.');
  console.log(`Screenshot: ${path.resolve(output, 'preview.png')}`);
} finally { await preview.close(); }
