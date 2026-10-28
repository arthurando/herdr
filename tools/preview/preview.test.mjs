import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, writeFile, mkdir, symlink } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { request } from 'node:http';
import { serveFile, localUrl } from './preview.mjs';

test('only explicit loopback URLs are accepted', () => {
  for (const url of ['http://localhost:3000/page', 'http://127.0.0.1:99', 'http://[::1]:90']) assert.ok(localUrl(url));
  for (const url of ['https://example.com', 'http://localhost.example.com', 'file:///secret', 'http://user:pass@localhost', 'javascript:alert(1)']) assert.throws(() => localUrl(url));
});

test('selected HTML directory serves assets, refreshes edits, denies escapes and credentials', async t => {
  const folder = await mkdtemp(path.join(tmpdir(), 'herdr-preview-test-'));
  const root = path.join(folder, 'public'); await mkdir(root);
  await writeFile(path.join(root, 'index.html'), '<h1>First</h1>');
  await writeFile(path.join(root, 'style.css'), 'body{color:red}');
  await writeFile(path.join(root, '.secret.html'), 'secret');
  await writeFile(path.join(root, 'credentials.json'), 'secret');
  await writeFile(path.join(folder, 'outside.html'), 'outside');
  // A directory junction requires no administrator or Developer Mode privilege on Windows.
  await symlink(folder, path.join(root, 'escape'), process.platform === 'win32' ? 'junction' : 'dir');
  const preview = await serveFile(path.join(root, 'index.html'));
  t.after(() => preview.close());
  const prefix = preview.url.slice(0, preview.url.lastIndexOf('/') + 1);
  const response = await fetch(preview.url); assert.equal(response.status, 200); assert.match(await response.text(), /First/);
  assert.equal(response.headers.get('access-control-allow-origin'), null);
  assert.equal(await (await fetch(prefix+'style.css')).text(), 'body{color:red}');
  await writeFile(path.join(root, 'index.html'), '<h1>Second</h1>');
  assert.match(await (await fetch(preview.url)).text(), /Second/);
  for (const relative of ['../outside.html', '%2e%2e%2foutside.html', 'escape/outside.html', '.secret.html', 'credentials.json', '', '%00.html']) assert.equal((await fetch(prefix+relative)).status, 404, relative);
  assert.equal((await fetch(preview.url, {method:'POST'})).status, 405);
  assert.equal(await new Promise(resolve => { const req = request(preview.url, {headers:{Host:'attacker.example'}}, res => {res.resume();resolve(res.statusCode);});req.end(); }), 403);
  await preview.close();
  await assert.rejects(fetch(preview.url));
});
