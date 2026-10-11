#!/usr/bin/env node
'use strict';
// `probbit` for npm installs: runs the native binary with this process's stdin/stdout/stderr and exits with its exit code
// (0 answer, 1 infeasible, 2 bad input, 3 refused / declined; see `probbit decide --help`). PROBBIT_BINARY=/path/to/probbit
// runs that binary instead. A missing binary (an install with --ignore-scripts) is fetched on first run.
const fs = require('fs');
const os = require('os');
const { spawn } = require('child_process');
const { install, binaryPath } = require('../install.js');

async function main() {
  let bin = process.env.PROBBIT_BINARY || binaryPath();
  if (!process.env.PROBBIT_BINARY && !fs.existsSync(bin)) {
    process.stderr.write('probbit: the binary is not installed yet, fetching it now\n');
    bin = await install();
  }
  const child = spawn(bin, process.argv.slice(2), { stdio: 'inherit' });
  const signals = ['SIGINT', 'SIGTERM', 'SIGHUP'].filter((s) => process.platform !== 'win32' || s !== 'SIGHUP');
  // Node's signal events carry no argument. Bind each signal explicitly instead of accidentally forwarding SIGTERM.
  const handlers = new Map(signals.map(s => [s, () => { try { child.kill(s); } catch (e) { /* already gone */ } }]));
  for (const [s, handler] of handlers) process.on(s, handler);
  child.on('error', (e) => {
    process.stderr.write(`probbit: cannot run ${bin}: ${e.message}\n`);
    process.exit(127);
  });
  child.on('exit', (code, signal) => {
    if (signal) {
      for (const [s, handler] of handlers) process.removeListener(s, handler);
      process.exitCode = 128 + (os.constants.signals[signal] || 0); // if the signal is ignored here (SIGPIPE)
      process.kill(process.pid, signal); // die the same way, so the shell sees 128 + n
      return;
    }
    process.exit(code === null ? 1 : code);
  });
}

main().catch((e) => {
  process.stderr.write(`probbit: ${e.message}\n`);
  process.exit(127);
});
