// Call probbit from Node with no dependencies: one child process per call, JSON on stdin, JSON on stdout, the exit code mapped.
//   node examples/node/decide.mjs                # decides a 12-task demo document, then shows each exit code once
//   node examples/node/decide.mjs router.json    # decides your router document (README, "Use it from anything")
// The binary: $PROBBIT_BIN, else `probbit` on PATH. On Windows point PROBBIT_BIN at probbit.exe: Node cannot spawn the npm `probbit.cmd`
// shim without a shell.
import { execFileSync, spawn } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';

const KIND = { 0: 'answer', 1: 'infeasible', 2: 'bad_input', 3: 'refused' };

/**
 * Run `probbit <args>` with `input` (an object or JSON text; omit for none) on stdin. Resolves to {kind, exitCode, output, stderr}:
 *   'answer'     exit 0: verdict exact | diagnostics_passed | partial (act on output.released, escalate output.escalated)
 *   'infeasible' exit 1: no plan satisfies the rules (a proof, not an error)
 *   'bad_input'  exit 2: output.error = {code, path, message} for a bad document; a bad flag leaves only a stderr line
 *   'refused'    exit 3: verdict refused / declined (escalate), or output.error.code === 'numeric'
 * Rejects on a missing binary, a crash or signal, unparseable output, or after `timeoutMs`.
 */
export function probbit(args, input, { bin = process.env.PROBBIT_BIN || 'probbit', timeoutMs } = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn(bin, args, { stdio: ['pipe', 'pipe', 'pipe'] });
    const out = [];
    const err = [];
    const timer = timeoutMs && setTimeout(() => { child.kill(); reject(new Error(`probbit ${args[0]} passed ${timeoutMs} ms`)); }, timeoutMs);
    child.stdout.on('data', (b) => out.push(b));
    child.stderr.on('data', (b) => err.push(b));
    child.on('error', (e) => { clearTimeout(timer); reject(e); }); // ENOENT: no binary
    child.on('close', (code, signal) => {
      clearTimeout(timer);
      const stdout = Buffer.concat(out).toString('utf8');
      const stderr = Buffer.concat(err).toString('utf8').trim();
      if (!(code in KIND)) return reject(new Error(`probbit exited ${code ?? signal}: ${stderr.slice(0, 300)}`));
      let output = null;
      try {
        if (stdout.trim()) output = JSON.parse(stdout);
      } catch {
        return reject(new Error(`probbit ${args[0]}: unparseable output (exit ${code})`));
      }
      resolve({ kind: KIND[code], exitCode: code, output, stderr });
    });
    child.stdin.on('error', () => {}); // EPIPE when probbit exits before reading all of it (a bad flag)
    child.stdin.end(input === undefined ? undefined : typeof input === 'string' || Buffer.isBuffer(input) ? input : JSON.stringify(input));
  });
}

function describe(r) {
  const o = r.output || {};
  if (r.kind === 'bad_input') return o.error ? `${o.error.code} at ${o.error.path}: ${o.error.message}` : r.stderr;
  const plan = o.plan ? Object.entries(o.plan).slice(0, 3).map(([t, w]) => `${t}->${w}`).join(', ') : '';
  const released = Array.isArray(o.released) ? `, ${o.released.length} of ${o.tasks} released` : '';
  return `verdict ${o.verdict}${released}${plan ? `, plan ${plan}, ...` : ''}${o.reason ? ` (${o.reason})` : ''}`;
}

async function main() {
  const bin = process.env.PROBBIT_BIN || 'probbit';
  try {
    console.log(`${execFileSync(bin, ['version'], { encoding: 'utf8' }).trim()} (${bin})`);
  } catch (e) {
    console.error(`cannot run ${bin} (${e.message}); install probbit or set PROBBIT_BIN`);
    process.exit(127);
  }
  if (process.argv[2]) {
    const r = await probbit(['decide', '--budget-ms', '200'], readFileSync(process.argv[2]));
    console.log(`exit ${r.exitCode} ${r.kind}: ${describe(r)}`);
    process.exit(r.exitCode);
  }
  const demo = (await probbit(['demo', '--tasks', '12'])).output;
  const cases = [
    ['answer', 'a 12-task demo document', ['decide'], demo],
    ['infeasible', 'two tasks, one worker with room for one', ['decide'],
      { workers: [{ id: 'a', cap: 1 }], tasks: [{ id: 't1', allowed: ['a'], scores: { a: 1 } }, { id: 't2', allowed: ['a'], scores: { a: 1 } }] }],
    ['bad_input', 'a malformed document', ['decide'], { workers: 1 }],
    ['refused', 'exact enumeration of a 96-variable program, capped at 50 ms', ['run', '--op', 'exact', '--exact-ms', '50'],
      readFileSync(new URL('../denoise-8x12.json', import.meta.url))],
  ];
  let ok = true;
  for (const [want, what, args, input] of cases) {
    const r = await probbit(args, input);
    ok &&= r.kind === want;
    console.log(`exit ${r.exitCode} ${r.kind.padEnd(10)} ${what}: ${describe(r)}`);
  }
  console.log(ok ? 'all four exit codes mapped as expected' : 'UNEXPECTED exit-code mapping');
  process.exit(ok ? 0 : 1);
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) main();
