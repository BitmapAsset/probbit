// Real-browser monitor regressions, with no npm dependencies. Requires Node 22+
// and Chrome/Chromium. Build `cargo build --release -p probbit-cli`, then run:
//   node probbit-cli/tests/monitor_browser.mjs
// Optional: CHROME_BIN, PROBBIT_BIN, PROBBIT_BROWSER_OUT. Artifacts default to a
// temporary directory, never the source tree. Only synthetic, local data is used.
import {spawn, spawnSync} from 'node:child_process';
import {appendFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {fileURLToPath} from 'node:url';
import assert from 'node:assert/strict';
import path from 'node:path';

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const bin = process.env.PROBBIT_BIN || path.join(repo, 'target/release', process.platform === 'win32' ? 'probbit.exe' : 'probbit');
const out = process.env.PROBBIT_BROWSER_OUT
  ? path.resolve(process.env.PROBBIT_BROWSER_OUT)
  : mkdtempSync(path.join(tmpdir(), 'probbit-monitor-browser-'));
mkdirSync(out, {recursive: true});
const children = [], sockets = [], servers = [], failures = [];
const report = {screenshots: [], follow: {}, demo: {}, browserErrors: failures};
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
const write = (name, text) => writeFileSync(path.join(out, name), text);

async function until(check, description, limit = 25000) {
  const start = Date.now();
  while (Date.now() - start < limit) {
    const value = await check();
    if (value) return value;
    await sleep(50);
  }
  throw Error('Timed out: ' + description);
}

function chromeBinary() {
  const candidates = process.env.CHROME_BIN ? [process.env.CHROME_BIN] : [
    '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
    '/Applications/Chromium.app/Contents/MacOS/Chromium',
    'chromium', 'chromium-browser', 'google-chrome', 'google-chrome-stable',
    ...[process.env.PROGRAMFILES, process.env['PROGRAMFILES(X86)'], process.env.LOCALAPPDATA]
      .filter(Boolean).map(root => path.join(root, 'Google/Chrome/Application/chrome.exe')),
  ];
  for (const candidate of candidates) {
    if (spawnSync(candidate, ['--version'], {encoding: 'utf8', timeout: 5000}).status === 0) return candidate;
  }
  throw Error('Chrome/Chromium is required; set CHROME_BIN to its executable.');
}

function command(args, input) {
  const result = spawnSync(bin, args, {
    cwd: repo, encoding: 'utf8', input, timeout: 20000,
    env: {...process.env, PROBBIT_THREADS: '2'},
  });
  assert.equal(result.status, 0, String(result.error || result.stderr));
  return result.stdout;
}

async function server(args, tag) {
  const child = spawn(bin, ['monitor', ...args, '--serve'], {
    cwd: repo, env: {...process.env, PROBBIT_THREADS: '2'},
  });
  children.push(child);
  let stdout = '', stderr = '', error;
  child.on('error', value => { error = value; });
  child.stdout.on('data', data => { stdout += data; });
  child.stderr.on('data', data => { stderr += data; });
  servers.push(() => { write(tag + '-stdout.txt', stdout); write(tag + '-stderr.txt', stderr); });
  const url = await until(() => {
    if (error) throw error;
    if (child.exitCode !== null) throw Error('Monitor exited: ' + stderr);
    return stdout.match(/http:\/\/127\.0\.0\.1:\d+\//)?.[0];
  }, tag + ' server');
  return {url};
}

async function json(url) {
  const response = await fetch(url, {signal: AbortSignal.timeout(5000)});
  assert.equal(response.status, 200, url);
  return response.json();
}

async function cdp(url) {
  const ws = new WebSocket(url);
  sockets.push(ws);
  await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(Error('DevTools connection timed out')), 10000);
    ws.onopen = () => { clearTimeout(timer); resolve(); };
    ws.onerror = error => { clearTimeout(timer); reject(error); };
  });
  let id = 0;
  const pending = new Map();
  ws.onmessage = event => {
    const message = JSON.parse(event.data);
    if (message.id) {
      const request = pending.get(message.id);
      if (!request) return;
      clearTimeout(request.timer);
      pending.delete(message.id);
      if (message.error) request.reject(Error(JSON.stringify(message.error)));
      else request.resolve(message.result);
    } else if (message.method === 'Runtime.exceptionThrown') {
      failures.push(message.params);
    }
  };
  ws.onclose = () => {
    for (const request of pending.values()) {
      clearTimeout(request.timer);
      request.reject(Error('DevTools connection closed'));
    }
    pending.clear();
  };
  return (method, params = {}) => new Promise((resolve, reject) => {
    const requestId = ++id;
    const timer = setTimeout(() => {
      pending.delete(requestId);
      reject(Error('DevTools timed out: ' + method));
    }, 10000);
    pending.set(requestId, {resolve, reject, timer});
    ws.send(JSON.stringify({id: requestId, method, params}));
  });
}

try {
  assert.equal(typeof WebSocket, 'function', 'Node 22+ is required');
  assert.ok(existsSync(bin), 'Build the release CLI first, or set PROBBIT_BIN');
  const profile = mkdtempSync(path.join(out, 'chrome-profile-'));
  const chrome = spawn(chromeBinary(), [
    '--headless=new', '--remote-debugging-address=127.0.0.1', '--remote-debugging-port=0',
    '--disable-background-networking', '--disable-sync', '--disable-extensions',
    '--no-first-run', '--no-default-browser-check', '--user-data-dir=' + profile, 'about:blank',
  ]);
  children.push(chrome);
  let chromeError, chromeStderr = '';
  chrome.on('error', error => { chromeError = error; });
  chrome.stderr.on('data', data => { chromeStderr += data; });
  chrome.stdout.resume();
  servers.push(() => write('chrome-stderr.txt', chromeStderr));
  await until(() => {
    if (chromeError) throw chromeError;
    if (chrome.exitCode !== null) throw Error('Chrome exited: ' + chromeStderr);
    return existsSync(path.join(profile, 'DevToolsActivePort'));
  }, 'Chrome DevTools');
  const port = readFileSync(path.join(profile, 'DevToolsActivePort'), 'utf8').split('\n')[0];
  const tabs = await json('http://127.0.0.1:' + port + '/json/list');
  const send = await cdp(tabs.find(tab => tab.type === 'page').webSocketDebuggerUrl);
  await send('Page.enable');
  await send('Runtime.enable');
  await send('Emulation.setEmulatedMedia', {features: [{name: 'prefers-reduced-motion', value: 'reduce'}]});
  const value = async expression => {
    const result = await send('Runtime.evaluate', {expression, returnByValue: true, awaitPromise: true});
    assert.ok(!result.exceptionDetails, JSON.stringify(result.exceptionDetails));
    return result.result.value;
  };
  const viewport = (width, height) => send('Emulation.setDeviceMetricsOverride', {width, height, deviceScaleFactor: 1, mobile: false});
  const navigate = async url => {
    await send('Page.navigate', {url});
    await until(() => value(`location.href === ${JSON.stringify(url)} && document.readyState === 'complete'`), 'monitor page load');
  };
  const state = () => value(`({
    event: document.getElementById('ev')?.textContent,
    note: document.getElementById('note')?.textContent,
    badge: document.getElementById('badge')?.textContent,
    line: document.getElementById('line')?.textContent,
    why: document.getElementById('why')?.textContent,
    saidHidden: document.getElementById('said')?.hidden,
    emptyHidden: document.getElementById('empty')?.hidden,
    rawHidden: document.getElementById('raw')?.hidden,
    drives: document.getElementById('drives')?.hidden,
    scrollWidth: document.documentElement.scrollWidth, width: innerWidth,
    height: document.documentElement.scrollHeight
  })`);
  async function shot(name, width, height) {
    await viewport(width, height);
    await sleep(150);
    const frame = await state();
    assert.ok(frame.scrollWidth <= width, 'Horizontal overflow: ' + JSON.stringify(frame));
    const screenshot = await send('Page.captureScreenshot', {
      format: 'png', captureBeyondViewport: true,
      clip: {x: 0, y: 0, width, height: Math.max(height, frame.height), scale: 1},
    });
    write(name + '.png', Buffer.from(screenshot.data, 'base64'));
    report.screenshots.push({name, ...frame});
  }
  const event = number => until(async () => (await state()).event === 'event ' + number, 'event ' + number);
  const learnError = () => value(`[...document.querySelectorAll('#drives .row')]
    .find(row => row.querySelector('.id')?.textContent === 'learn')?.querySelector('.petxt')?.textContent`);

  const demo = await server(['--demo', 'drives'], 'drives');
  await viewport(1440, 1000);
  await navigate(demo.url);
  await event(3);
  report.demo.event3 = await state();
  assert.equal(report.demo.event3.drives, false);
  await shot('monitor-drives-desktop', 1440, 1000);
  await shot('monitor-drives-narrow', 390, 844);
  await event(4);
  report.demo.negativeError = await learnError();
  assert.ok(report.demo.negativeError.includes('below expectation'));
  await shot('monitor-drives-negative', 1440, 1000);
  await event(5);
  report.demo.clearedError = await learnError();
  assert.equal(report.demo.clearedError, 'prediction error 0.00');
  await until(async () => (await state()).note?.includes('complete; restarting'), 'demo completion cue');
  report.demo.completed = await state();
  await shot('monitor-drives-completed', 1440, 1000);
  await until(async () => (await state()).note?.includes('loop 2'), 'demo loop 2');
  report.demo.restarted = await state();
  assert.equal(report.demo.restarted.event, 'event 0');
  assert.equal(report.demo.restarted.line, '');
  assert.equal(report.demo.restarted.why, '');
  assert.equal(report.demo.restarted.saidHidden, true);
  assert.equal(report.demo.restarted.emptyHidden, false);
  assert.equal(report.demo.restarted.rawHidden, true);
  assert.equal(report.demo.restarted.drives, true);

  const week = await server(['--demo'], 'week');
  await navigate(week.url);
  await event(3);
  await shot('monitor-week-desktop', 1440, 1000);
  await shot('monitor-week-narrow', 390, 844);

  const full = path.join(out, 'follow-full-' + Date.now() + '.strand');
  const growing = path.join(out, 'follow-growing-' + Date.now() + '.strand');
  const outputs = command(['live', 'examples/persona/tutor.yaml', '--seed', '2', '--clock', 'fixed', '--strand', full],
    '{"praise":true,"elapsed_hours":1}\n{"loss":true,"elapsed_hours":1}\n{}\n').trim().split('\n').map(JSON.parse);
  const lines = readFileSync(full, 'utf8').match(/.*\n/g);
  writeFileSync(growing, lines.slice(0, 2).join(''));
  const follow = await server([growing], 'follow');
  await viewport(1440, 1000);
  await navigate(follow.url);
  await event(1);
  report.follow.before = await state();
  await shot('monitor-follow-before', 1440, 1000);
  appendFileSync(growing, lines[2].slice(0, -1));
  await sleep(350);
  assert.equal((await state()).event, 'event 1');
  report.follow.incompleteHeld = true;
  const appended = Date.now();
  appendFileSync(growing, '\n');
  await until(async () => (await state()).event === 'event 2', 'appended event', 2000);
  report.follow.appendLatencyMs = Date.now() - appended;
  assert.ok(report.follow.appendLatencyMs < 1000, 'Follow update exceeded one second');
  report.follow.after = await state();
  assert.equal(report.follow.after.line, outputs[1].line);
  assert.deepEqual(await json(follow.url + 'doc/2'), outputs[1]);
  await shot('monitor-follow-after', 1440, 1000);
  appendFileSync(growing, lines[3]);
  await event(3);
  assert.ok((await state()).why.includes('prior state still applies'));
  assert.equal((await json(follow.url + 'doc/3')).why, 'baseline (no live evidence)');
  report.follow.rawWhyUnchanged = true;
  command(['live', 'control', growing, 'pause', '--by', 'human:demo', '--reason', 'synthetic browser check']);
  await until(async () => (await state()).badge?.includes('paused'), 'paused status');
  report.follow.paused = await state();
  await shot('monitor-paused-narrow', 390, 844);

  const checkpoint = path.join(out, 'checkpoint-' + Date.now() + '.strand');
  command(['live', 'examples/persona/tutor.yaml', '--seed', '2', '--clock', 'fixed', '--checkpoint-every', '2', '--strand', checkpoint], '{}\n{}\n{}\n');
  const checkpointServer = await server([checkpoint], 'checkpoint');
  await navigate(checkpointServer.url);
  await event(3);
  report.checkpoint = await state();
  assert.ok(report.checkpoint.badge.includes('from checkpoint 2'));
  assert.equal(failures.length, 0, JSON.stringify(failures));
  report.ok = true;
} catch (error) {
  report.error = String(error.stack || error);
  process.exitCode = 1;
} finally {
  for (const save of servers) save();
  write('browser-report.json', JSON.stringify(report, null, 2) + '\n');
  for (const ws of sockets) ws.close();
  for (const child of children) child.kill('SIGTERM');
  console.log(JSON.stringify({artifacts: out, ...report}, null, 2));
}
