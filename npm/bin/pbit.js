#!/usr/bin/env node
'use strict';
// `pbit` for npm installs: runs the native binary with this process's stdin/stdout/stderr and exits with its exit code
// (0 answer, 1 infeasible, 2 bad input, 3 refused / declined; see `pbit decide --help`). PBIT_BINARY=/path/to/pbit
// runs that binary instead. A missing binary (an install with --ignore-scripts) is fetched on first run.
const fs = require('fs');
const os = require('os');
const { spawn } = require('child_process');
const { install, binaryPath } = require('../install.js');

async function main() {
  let bin = process.env.PBIT_BINARY || binaryPath();
  if (!process.env.PBIT_BINARY && !fs.existsSync(bin)) {
    process.stderr.write('pbit: the binary is not installed yet, fetching it now\n');
    bin = await install();
  }
  const child = spawn(bin, process.argv.slice(2), { stdio: 'inherit' });
  const signals = ['SIGINT', 'SIGTERM', 'SIGHUP'].filter((s) => process.platform !== 'win32' || s !== 'SIGHUP');
  const forward = (s) => { try { child.kill(s); } catch (e) { /* already gone */ } };
  for (const s of signals) process.on(s, forward);
  child.on('error', (e) => {
    process.stderr.write(`pbit: cannot run ${bin}: ${e.message}\n`);
    process.exit(127);
  });
  child.on('exit', (code, signal) => {
    if (signal) {
      for (const s of signals) process.removeListener(s, forward);
      process.exitCode = 128 + (os.constants.signals[signal] || 0); // if the signal is ignored here (SIGPIPE)
      process.kill(process.pid, signal); // die the same way, so the shell sees 128 + n
      return;
    }
    process.exit(code === null ? 1 : code);
  });
}

main().catch((e) => {
  process.stderr.write(`pbit: ${e.message}\n`);
  process.exit(127);
});
