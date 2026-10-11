// Real Chromium UI checks, with no npm dependencies. Node >=22 and Chrome/Chromium are test-only requirements.
// Build first: sh playground/build.sh. Then: node playground/check-browser.mjs [screenshot-directory]
import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { createServer } from 'node:http';
import { existsSync, mkdtempSync, mkdirSync, readFileSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, extname, join, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { once } from 'node:events';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const profile = mkdtempSync(join(tmpdir(), 'probbit-browser-'));
const output = resolve(process.argv[2] || mkdtempSync(join(tmpdir(), 'probbit-browser-shots-')));
mkdirSync(output, { recursive: true });
const executable = process.env.CHROME_BIN || [
  '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  '/usr/bin/google-chrome', '/usr/bin/chromium', '/usr/bin/chromium-browser',
].find(existsSync) || spawnSync('which', ['google-chrome'], { encoding: 'utf8' }).stdout.trim();
assert.ok(executable, 'Set CHROME_BIN to an installed Chrome or Chromium executable');
assert.equal(typeof WebSocket, 'function', 'This test requires Node >=22 (built-in WebSocket)');

const server = createServer((req, res) => {
  const pathname = decodeURIComponent(new URL(req.url, 'http://localhost').pathname);
  const file = resolve(root, '.' + pathname);
  if (!file.startsWith(root + sep)) { res.writeHead(403); res.end(); return; }
  try {
    const body = readFileSync(file);
    const type = { '.html': 'text/html', '.js': 'text/javascript', '.wasm': 'application/wasm', '.json': 'application/json' }[extname(file)] || 'application/octet-stream';
    res.writeHead(200, { 'Content-Type': type }); res.end(body);
  } catch { res.writeHead(404); res.end('Not found'); }
});
server.listen(0, '127.0.0.1');
await once(server, 'listening');
const base = `http://127.0.0.1:${server.address().port}`;
const chrome = spawn(executable, ['--headless=new', '--no-first-run', '--no-default-browser-check',
  '--disable-background-networking', '--remote-debugging-port=0', `--user-data-dir=${profile}`, 'about:blank'],
{ stdio: ['ignore', 'ignore', 'pipe'] });
let socket;
const pending = new Map();
const errors = [];
let nextId = 0;
try {
  const url = await new Promise((ok, fail) => {
    const timer = setTimeout(() => fail(new Error('Chrome debugger did not start within 20 seconds')), 20000);
    let stderr = '';
    chrome.on('error', e => { clearTimeout(timer); fail(e); });
    chrome.on('exit', code => { clearTimeout(timer); fail(new Error(`Chrome exited: ${code}\n${stderr}`)); });
    chrome.stderr.on('data', b => { stderr += b; const match = stderr.match(/DevTools listening on (ws:\/\/\S+)/); if (match) { clearTimeout(timer); ok(match[1]); } });
  });
  socket = new WebSocket(url);
  await new Promise((ok, fail) => { socket.addEventListener('open', ok, { once: true }); socket.addEventListener('error', fail, { once: true }); });
  socket.addEventListener('message', event => {
    const msg = JSON.parse(event.data);
    if (msg.method === 'Runtime.exceptionThrown') errors.push(msg.params.exceptionDetails);
    if (msg.id && pending.has(msg.id)) {
      const { ok, fail, timer } = pending.get(msg.id); pending.delete(msg.id); clearTimeout(timer);
      if (msg.error) fail(new Error(JSON.stringify(msg.error))); else ok(msg.result);
    }
  });
  function call(method, params = {}, sessionId) {
    const id = ++nextId;
    return new Promise((ok, fail) => {
      const timer = setTimeout(() => { pending.delete(id); fail(new Error(`CDP timeout: ${method}`)); }, 30000);
      pending.set(id, { ok, fail, timer });
      socket.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }));
    });
  }
  const { targetId } = await call('Target.createTarget', { url: 'about:blank' });
  const { sessionId } = await call('Target.attachToTarget', { targetId, flatten: true });
  const page = (method, params) => call(method, params, sessionId);
  await page('Runtime.enable');
  await page('Page.enable');
  async function evaluate(expression) {
    const r = await page('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
    if (r.exceptionDetails) throw new Error(JSON.stringify(r.exceptionDetails));
    return r.result.value;
  }
  async function until(expression) {
    const deadline = Date.now() + 25000;
    while (Date.now() < deadline) {
      if (await evaluate(expression)) return;
      await new Promise(r => setTimeout(r, 100));
    }
    throw new Error(`UI timeout: ${expression}`);
  }
  async function screenshot(name) {
    const shot = await page('Page.captureScreenshot', { format: 'png', captureBeyondViewport: false });
    writeFileSync(join(output, name + '.png'), Buffer.from(shot.data, 'base64'));
  }
  async function viewport(width) {
    await page('Emulation.setDeviceMetricsOverride', { width, height: 1000, deviceScaleFactor: 1, mobile: false });
  }
  await viewport(1440);
  await page('Page.navigate', { url: base + '/playground/index.html' });
  await until('document.title === "probbit playground: ready"');
  let result = await evaluate('JSON.parse(document.querySelector("#raw").textContent)');
  assert.equal(result.verdict, 'exact'); assert.equal(result.violations, 0);
  assert.equal(result.answers.team.probbit.value, 'technical');
  assert.match(await evaluate('document.querySelector("#meet-status").textContent'), /turns/);
  await screenshot('playground-desktop');
  // Drive the public controls, including an error followed by recovery.
  await evaluate('document.querySelector("#input").value = "{bad"; document.querySelector("#go").click()');
  await until('!document.querySelector("#go").disabled && document.querySelector("#verdict").textContent.startsWith("ERROR")');
  await evaluate('document.querySelector("#ex-demo").click(); document.querySelector("#go").click()');
  await until('!document.querySelector("#go").disabled && JSON.parse(document.querySelector("#raw").textContent).tasks === 300');
  result = await evaluate('JSON.parse(document.querySelector("#raw").textContent)');
  assert.equal(result.verdict, 'diagnostics_passed'); assert.equal(result.violations, 0);
  await viewport(390);
  await screenshot('playground-mobile');
  const playgroundWidth = await evaluate('({page: document.documentElement.scrollWidth, viewport: innerWidth})');
  assert.ok(playgroundWidth.page <= playgroundWidth.viewport + 1, `Playground overflow: ${JSON.stringify(playgroundWidth)}`);
  console.log(JSON.stringify({ check: 'playground browser controls, errors, recovery, personas, 390px layout', passed: true }));

  await viewport(1440);
  await page('Page.navigate', { url: base + '/playground/puzzle.html' });
  await until('document.querySelector("#lineA")?.textContent.includes("Stance:")');
  await evaluate('document.querySelector("#reveal").click(); document.querySelector("#replay").click()');
  await until('document.querySelector("#replayOut").textContent.includes("same")');
  const replay = await evaluate('document.querySelector("#replayOut").textContent');
  assert.match(replay, /Both branches repeat byte for byte and equal the native CLI's pinned values/);
  assert.equal(await evaluate('document.querySelectorAll("#replayOut .bad").length'), 0);
  await screenshot('puzzle-desktop');
  await viewport(390);
  await screenshot('puzzle-mobile');
  const puzzleWidth = await evaluate('({page: document.documentElement.scrollWidth, viewport: innerWidth})');
  assert.ok(puzzleWidth.page <= puzzleWidth.viewport + 1, `Puzzle overflow: ${JSON.stringify(puzzleWidth)}`);
  assert.equal(errors.length, 0, JSON.stringify(errors));
  console.log(JSON.stringify({ check: 'puzzle browser reveal and replay, 390px layout, no uncaught JS errors', passed: true }));
  console.log(JSON.stringify({ screenshots: output, browser: (await call('Browser.getVersion')).product }));
} finally {
  for (const { timer } of pending.values()) clearTimeout(timer);
  if (socket) socket.close();
  chrome.kill('SIGTERM');
  await new Promise(resolve => { if (chrome.exitCode !== null) return resolve(); const timer = setTimeout(() => { chrome.kill('SIGKILL'); resolve(); }, 3000); chrome.once('exit', () => { clearTimeout(timer); resolve(); }); });
  server.closeAllConnections();
  await new Promise(resolve => server.close(resolve));
  rmSync(profile, { recursive: true, force: true });
}
