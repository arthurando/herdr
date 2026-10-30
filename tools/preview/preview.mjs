import { createServer } from 'node:http';
import { randomBytes } from 'node:crypto';
import { readFile, realpath, stat, mkdir, mkdtemp } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawn, execFileSync } from 'node:child_process';

const mime = { '.html': 'text/html', '.htm': 'text/html', '.js': 'text/javascript', '.mjs': 'text/javascript', '.css': 'text/css', '.svg': 'image/svg+xml', '.png': 'image/png', '.jpg': 'image/jpeg', '.webp': 'image/webp', '.woff2': 'font/woff2' };
const inside = (root, file) => { const rel = path.relative(root, file); return rel === '' || (!rel.startsWith(`..${path.sep}`) && rel !== '..' && !path.isAbsolute(rel)); };

export function localUrl(value) {
  const url = new URL(value);
  if (!['http:', 'https:'].includes(url.protocol) || !['localhost', '127.0.0.1', '[::1]'].includes(url.hostname) || url.username || url.password) throw new Error('Preview URL must use localhost or a loopback IP, without credentials.');
  return url.href;
}

export async function serveFile(input) {
  const file = await realpath(path.resolve(input));
  if (!(await stat(file)).isFile() || !['.html', '.htm'].includes(path.extname(file).toLowerCase())) throw new Error('Select an existing .html or .htm file.');
  const root = path.dirname(file);
  const token = randomBytes(24).toString('hex');
  let port;
  const server = createServer(async (req, res) => {
    const deny = (code) => { res.writeHead(code); res.end(); };
    if (req.headers.host !== `127.0.0.1:${port}`) return deny(403);
    if (!['GET', 'HEAD'].includes(req.method)) return deny(405);
    try {
      const pathname = decodeURIComponent(new URL(req.url, 'http://localhost').pathname);
      if (!pathname.startsWith(`/${token}/`) || pathname.includes('\\') || pathname.includes('\0')) return deny(404);
      const resource = await realpath(path.resolve(root, pathname.slice(token.length + 2)));
      if (!inside(root, resource) || !(await stat(resource)).isFile()) return deny(404);
      const parts = path.relative(root, resource).split(path.sep);
      if (parts.some(part => part.startsWith('.')) || !Object.hasOwn(mime, path.extname(resource).toLowerCase())) return deny(404);
      const data = await readFile(resource);
      res.writeHead(200, {
        'Content-Type': mime[path.extname(resource).toLowerCase()] || 'application/octet-stream',
        'Cache-Control': 'no-store', 'X-Content-Type-Options': 'nosniff',
        'Referrer-Policy': 'no-referrer',
        'Content-Security-Policy': "default-src 'self' data: blob:; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; connect-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'",
      });
      res.end(req.method === 'HEAD' ? undefined : data);
    } catch { deny(404); }
  });
  await new Promise((resolve, reject) => { server.once('error', reject); server.listen(0, '127.0.0.1', resolve); });
  port = server.address().port;
  return { url: `http://127.0.0.1:${port}/${token}/${encodeURIComponent(path.basename(file))}`, close: () => new Promise(resolve => { server.close(resolve); server.closeAllConnections(); }) };
}

export function edgePath() {
  for (const base of [process.env['ProgramFiles(x86)'], process.env.ProgramFiles, process.env.LOCALAPPDATA]) {
    if (base) { const file = path.join(base, 'Microsoft', 'Edge', 'Application', 'msedge.exe'); if (existsSync(file)) return file; }
  }
  throw new Error('Microsoft Edge is required.');
}

function placement() {
  // Capture the foreground terminal before Edge opens. Never resize that terminal.
  const script = path.join(path.dirname(fileURLToPath(import.meta.url)), 'placement.ps1');
  return JSON.parse(execFileSync('powershell.exe', ['-NoProfile', '-File', script], { windowsHide: true, encoding: 'utf8' }));
}

export async function main(args) {
  if (args.length !== 1 || args[0] === '--help') {
    console.log('Usage: node tools/preview/preview.mjs <local.html|http://localhost:port/path>');
    return;
  }
  if (process.platform !== 'win32') throw new Error('This companion launcher supports Windows.');
  const edge = edgePath();
  const target = args[0];
  const local = /^https?:/i.test(target) ? null : await serveFile(target);
  try {
    const url = local ? local.url : localUrl(target);
    const bounds = placement();
    const profileRoot = path.join(process.env.LOCALAPPDATA, 'herdr', 'preview', 'profiles');
    await mkdir(profileRoot, { recursive: true });
    const profile = await mkdtemp(path.join(profileRoot, 'panel-'));
    // This is the requested GUI. SW_HIDE also hides Edge's app window, not only consoles.
    const child = spawn(edge, [`--app=${url}`, `--user-data-dir=${profile}`, '--guest', '--no-first-run', '--no-default-browser-check', '--disable-extensions', '--disable-background-mode', `--window-position=${bounds.x},${bounds.y}`, `--window-size=${bounds.width},${bounds.height}`], { windowsHide: false, stdio: 'ignore' });
    console.log('Herdr HTML preview launched. Close its window to stop this preview.');
    console.log(`Isolated browser profile: ${profile}`);
    await new Promise((resolve, reject) => { child.once('error', reject); child.once('exit', (code) => code === 0 ? resolve() : reject(new Error(`Edge exited with code ${code}`))); });
  } finally { if (local) await local.close(); }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main(process.argv.slice(2)).catch(error => { console.error(error.message); process.exitCode = 1; });
