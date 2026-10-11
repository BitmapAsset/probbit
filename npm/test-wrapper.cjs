'use strict';
// Dependency-free unit checks for platform routing and native-process contracts.
const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const { EventEmitter } = require('node:events');

function installer(platform, arch, glibc, env = {}) {
  const urls = [];
  const module = { exports: {} };
  const context = {
    require, module, __dirname, Buffer, AbortSignal,
    process: { platform, arch, env, report: { getReport: () => ({ header: { glibcVersionRuntime: glibc } }) } },
    fetch: async url => { urls.push(url); throw new Error('fixture: network disabled'); },
  };
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, 'install.js'), 'utf8'), context);
  return { ...module.exports, urls };
}

for (const [platform, arch, glibc, target] of [
  ['darwin', 'arm64', null, 'aarch64-apple-darwin'],
  ['darwin', 'x64', null, 'x86_64-apple-darwin'],
  ['linux', 'x64', '2.35', 'x86_64-unknown-linux-gnu'],
  ['linux', 'x64', null, 'x86_64-unknown-linux-musl'],
  ['linux', 'arm64', '2.35', 'aarch64-unknown-linux-musl'],
  ['win32', 'x64', null, 'x86_64-pc-windows-msvc'],
  ['win32', 'arm64', null, 'x86_64-pc-windows-msvc'],
]) {
  test(`archive selection: ${platform}/${arch}/${glibc || 'no glibc'}`, async () => {
    const fixture = installer(platform, arch, glibc);
    await assert.rejects(fixture.install(() => {}), /network disabled/);
    assert.ok(fixture.urls[0].includes(`-${target}.`), fixture.urls[0]);
    assert.equal(fixture.urls.length, 2);
  });
}

test('unsupported architecture fails without fetching', async () => {
  const fixture = installer('linux', 'riscv64', null);
  await assert.rejects(fixture.install(() => {}), /no prebuilt/);
  assert.equal(fixture.urls.length, 0);
});

for (const env of [{ PROBBIT_VERSION: '../bad' }, { PROBBIT_TARGET: '../../bad' }]) {
  test(`invalid download selector: ${Object.keys(env)[0]}`, async () => {
    const fixture = installer('darwin', 'arm64', null, env);
    await assert.rejects(fixture.install(() => {}), /invalid/);
    assert.equal(fixture.urls.length, 0);
  });
}

function wrapper(platform = 'linux') {
  const child = new EventEmitter();
  const signals = [];
  child.kill = signal => signals.push(signal);
  const process = new EventEmitter();
  Object.assign(process, { env: { PROBBIT_BINARY: '/fixture/probbit' }, argv: ['node', 'wrapper', 'monitor', '--follow'],
    platform, pid: 42, stderr: { write() {} }, exit(code) { this.exitCode = code; }, kill(pid, signal) { this.killed = [pid, signal]; } });
  let invocation;
  const requireFixture = id => {
    if (id === 'child_process') return { spawn: (binary, args, opts) => { invocation = { binary, args: [...args], stdio: opts.stdio }; return child; } };
    if (id === '../install.js') return { binaryPath: () => '/unused', install: () => { throw new Error('unexpected download'); } };
    return require(id);
  };
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, 'bin/probbit.js'), 'utf8'), { require: requireFixture, process });
  return { child, process, signals, invocation };
}

test('wrapper preserves arguments, streams, and every documented exit code', () => {
  for (const code of [0, 1, 2, 3, 4]) {
    const fixture = wrapper();
    assert.deepEqual(fixture.invocation, { binary: '/fixture/probbit', args: ['monitor', '--follow'], stdio: 'inherit' });
    fixture.child.emit('exit', code, null);
    assert.equal(fixture.process.exitCode, code);
  }
});

test('wrapper forwards the exact signal even though Node signal events carry no arguments', () => {
  const fixture = wrapper();
  for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP']) fixture.process.emit(signal);
  assert.deepEqual(fixture.signals, ['SIGINT', 'SIGTERM', 'SIGHUP']);
  fixture.child.emit('exit', null, 'SIGINT');
  assert.deepEqual(fixture.process.killed, [42, 'SIGINT']);
  assert.equal(fixture.process.listenerCount('SIGINT'), 0);
});

test('wrapper reports native spawn failures as 127', () => {
  const fixture = wrapper();
  fixture.child.emit('error', new Error('ENOENT'));
  assert.equal(fixture.process.exitCode, 127);
});
