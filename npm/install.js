'use strict';
// Fetches the prebuilt pbit binary for this platform from the GitHub release that matches this package's version, checks its
// SHA-256 against the .sha256 file beside it and puts it in vendor/. Runs as `postinstall`; bin/pbit.js calls it again on
// first use if the binary is missing (installs with --ignore-scripts). No dependencies: Node >= 18 (global fetch) and the
// system `tar` (Windows 10+ ships tar.exe, which also unpacks .zip).
//
// Environment (same names as install.sh):
//   PBIT_VERSION        release tag to fetch (default: v<this package's version>)
//   PBIT_DOWNLOAD_BASE  archive URL = $PBIT_DOWNLOAD_BASE/<tag>/pbit-<tag>-<target>.<tar.gz|zip>
//                       (default: https://github.com/BitmapAsset/pbit/releases/download)
//   PBIT_TARGET         Rust target triple to fetch instead of the detected one
//   PBIT_BINARY         path to an existing pbit binary: copied into vendor/ instead of downloading
const fs = require('fs');
const os = require('os');
const path = require('path');
const crypto = require('crypto');
const { execFileSync } = require('child_process');
const pkg = require('./package.json');

const REPO = 'BitmapAsset/pbit';
const TARGETS = {
  'darwin-arm64': 'aarch64-apple-darwin',
  'darwin-x64': 'x86_64-apple-darwin',
  'linux-x64': 'x86_64-unknown-linux-gnu',
  'win32-x64': 'x86_64-pc-windows-msvc',
  'win32-arm64': 'x86_64-pc-windows-msvc', // x64 emulation on Windows on Arm
};
const EXE = process.platform === 'win32' ? 'pbit.exe' : 'pbit';

function binaryPath() {
  return path.join(__dirname, 'vendor', EXE);
}

class ChecksumError extends Error {}

async function get(url) {
  const res = await fetch(url, { redirect: 'follow' });
  if (!res.ok) throw new Error(`GET ${url}: HTTP ${res.status}`);
  return Buffer.from(await res.arrayBuffer());
}

// copy then rename, so a concurrent first run never sees a half-written binary
function place(src, dest) {
  fs.mkdirSync(path.dirname(dest), { recursive: true });
  const tmp = `${dest}.${process.pid}.tmp`;
  fs.copyFileSync(src, tmp);
  fs.chmodSync(tmp, 0o755);
  try {
    fs.renameSync(tmp, dest);
  } catch (e) {
    fs.rmSync(tmp, { force: true });
    if (!fs.existsSync(dest)) throw e; // another process put it there first
  }
}

async function install(log = (m) => process.stderr.write(`${m}\n`)) {
  const env = process.env;
  const dest = binaryPath();
  if (env.PBIT_BINARY) {
    const src = path.resolve(env.PBIT_BINARY);
    place(src, dest);
    log(`pbit: using ${src} (PBIT_BINARY)`);
    return dest;
  }
  const target = env.PBIT_TARGET || TARGETS[`${process.platform}-${process.arch}`];
  if (!target) {
    throw new Error(`no prebuilt pbit for ${process.platform}-${process.arch}; build one (cargo build --release -p pbit-cli) `
      + 'and reinstall with PBIT_BINARY=/path/to/pbit');
  }
  let tag = env.PBIT_VERSION || `v${pkg.version}`;
  if (!tag.startsWith('v')) tag = `v${tag}`;
  const base = (env.PBIT_DOWNLOAD_BASE || `https://github.com/${REPO}/releases/download`).replace(/\/+$/, '');
  const zip = target.includes('windows');
  const name = `pbit-${tag}-${target}`;
  const asset = `${name}.${zip ? 'zip' : 'tar.gz'}`;
  const url = `${base}/${tag}/${asset}`;
  log(`pbit: fetching ${url}`);
  const [archive, sums] = await Promise.all([get(url), get(`${url}.sha256`)]);
  const want = sums.toString('utf8').trim().split(/\s+/)[0].toLowerCase();
  const got = crypto.createHash('sha256').update(archive).digest('hex');
  if (!/^[0-9a-f]{64}$/.test(want) || want !== got) {
    throw new ChecksumError(`checksum mismatch for ${asset}: expected ${want}, got ${got}`);
  }
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'pbit-npm-'));
  try {
    const file = path.join(tmp, asset);
    fs.writeFileSync(file, archive);
    // Windows: call System32\tar.exe (bsdtar, reads .zip) by full path; a GNU tar earlier on PATH (Git) cannot
    const tar = process.platform === 'win32' ? path.join(env.SystemRoot || 'C:\\Windows', 'System32', 'tar.exe') : 'tar';
    execFileSync(tar, [zip ? '-xf' : '-xzf', file, '-C', tmp], { stdio: ['ignore', 'ignore', 'inherit'] });
    const src = path.join(tmp, name, EXE);
    if (!fs.existsSync(src)) throw new Error(`${asset} has no ${name}/${EXE}`);
    place(src, dest);
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
  log(`pbit: installed ${tag} (${target}), sha256 ${got}`);
  return dest;
}

module.exports = { install, binaryPath, ChecksumError };

if (require.main === module) {
  install().catch((e) => {
    if (e instanceof ChecksumError) {
      process.stderr.write(`pbit: ${e.message}; nothing was installed\n`);
      process.exit(1);
    }
    // offline or no release yet: do not fail the npm install; bin/pbit.js retries on first run
    process.stderr.write(`pbit: could not fetch the binary now (${e.message}); it will be fetched on first run, `
      + 'or reinstall with PBIT_BINARY=/path/to/pbit\n');
  });
}
