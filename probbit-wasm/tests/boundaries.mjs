// Actual wasm32 boundary regressions and CLI parity. No npm packages.
// node probbit-wasm/tests/boundaries.mjs [module.wasm] [native-probbit]
import { readFileSync } from 'node:fs';
import { strict as assert } from 'node:assert';
import { spawnSync } from 'node:child_process';
const modulePath = process.argv[2] || new URL('../../target/wasm32-unknown-unknown/release/probbit_wasm.wasm', import.meta.url);
const native = process.argv[3];
const { instance: { exports: e } } = await WebAssembly.instantiate(readFileSync(modulePath), { probbit: { now_ms: () => performance.now() } });
let calls = 0;
function call(op, input) {
  const b = typeof input === 'string' ? new TextEncoder().encode(input) : input;
  const p = e.probbit_alloc(b.length);
  new Uint8Array(e.memory.buffer, p, b.length).set(b);
  const n = e.probbit_call(op, p, b.length);
  const out = JSON.parse(new TextDecoder().decode(new Uint8Array(e.memory.buffer, e.probbit_out_ptr(), n)));
  calls++;
  return { code: e.probbit_out_code(), out };
}
const run = j => call(1, JSON.stringify(j));
const base = { probbit_ir: 1, values: ['a', 'b'], vars: [{ id: 'x', clamp: 'b' }, { id: 'y', clamp: 'b' }] };
function error(result, code = 2) {
  assert.equal(result.code, code);
  assert.deepEqual(Object.keys(result.out), ['error']);
  assert.ok(result.out.error.code && typeof result.out.error.message === 'string');
}
// A u64-to-usize cast used to silently turn 2^32 fixed sweeps into zero;
// capacities saturated, changing the accepted request on 32-bit targets.
for (const flag of ['sweeps', 'polish_sweeps', 'frontier_states', 'mem_limit_mb']) {
  const r = run({ ...base, flags: { [flag]: 2 ** 32, budget_ms: 0, polish_ms: 0 } });
  error(r); assert.equal(r.out.error.path, 'flags.' + flag);
}
error(run({ ...base, caps: [{ value: 'a', limit: 2 ** 32 }] }));
error(call(1, '{"flags":{"sweeps":1,"sweeps":2}}'));
for (const text of ['01', '-.1', '1.', '1.e2', '"raw\nnewline"', '1e309']) error(call(1, text));
error(call(1, new Uint8Array([0xff])));
error(call(99, '{}'));
error(call(1, ''));
error(call(3, '{"tasks":3333334}'));
// The seed is genuinely u64, not a usize; the larger seed must survive wasm32.
const seed64 = call(3, '{"tasks":1,"seed":4294967296}'); assert.equal(seed64.code, 0);
const seed32 = call(3, '{"tasks":1,"seed":4294967295}'); assert.notDeepEqual(seed64.out, seed32.out);

// 1 + u32::MAX wrapped to zero, turning impossible precedence into exact success.
const precedence = { ...base, precedes: [{ before: 'x', after: 'y', gap: 4294967295 }] };
const gap = run(precedence); assert.equal(gap.code, 1); assert.equal(gap.out.verdict, 'infeasible'); assert.equal(gap.out.plan, undefined);

// 4,295 * 1,000,000 wrapped to 32,704, so compile_parts dropped a binding
// capacity whose limit was 5,000,000. Never emit an exact answer after that loss.
const weighted = { probbit_ir: 1, values: ['on'], vars: Array.from({ length: 4295 }, (_, i) => ({ id: 'x' + i })),
  linear: [{ limit: 5000000, terms: Array.from({ length: 4295 }, (_, i) => ['x' + i, 'on', 1000000]) }], flags: { exact_limit: 0 } };
error(run(weighted));
const warm = { ...weighted, start: Object.fromEntries(weighted.vars.map(v => [v.id, 'on'])) };
error(run(warm));

// 2048^3 wrapped to zero; reject the expansion before allocating its tuples.
const values = Array.from({ length: 2048 }, (_, i) => 'v' + i);
const table = { probbit_ir: 1, values, vars: [{ id: 'a' }, { id: 'b' }, { id: 'c' }], tables: [{ vars: ['a', 'b', 'c'], allow: [] }] };
const tr = run(table); error(tr); assert.equal(tr.out.error.code, 'limit');

// k rows existed but were empty: reserving k*k f64s before validating them
// trapped on wasm32 even though the malformed document itself was small.
const hugeAlphabet = Array.from({ length: 65535 }, (_, i) => 'v' + i);
const ragged = { probbit_ir: 1, values: hugeAlphabet, vars: [{ id: 'a' }, { id: 'b' }],
  pairs: [{ i: 'a', j: 'b', table: hugeAlphabet.map(() => []) }] };
const rr = run(ragged); error(rr); assert.equal(rr.out.error.path, 'pairs[0].table[0]');

function contract(r) {
  const j = r.out;
  if (!j.plan) { assert.equal(j.released_plan, undefined); return; }
  assert.equal(j.plan_status, ['exact', 'diagnostics_passed'].includes(j.verdict) ? 'released' : j.verdict === 'partial' ? 'partial' : 'diagnostic');
  assert.deepEqual(Object.keys(j.released_plan).sort(), [...j.released].sort());
  for (const [id, value] of Object.entries(j.released_plan)) assert.equal(value, j.plan[id]);
  if (j.verdict === 'refused') assert.deepEqual(j.released_plan, {});
}
const transient = new Set(['ms', 'sample_ms', 'gate_ms', 'site_updates_per_s', 'process_cpu_ms', 'peak_rss_mb', 'nice', 'phases', 'threads']);
function stable(j) {
  if (Array.isArray(j)) return j.map(stable);
  if (j && typeof j === 'object') return Object.fromEntries(Object.entries(j).filter(([k]) => !transient.has(k)).map(([k, v]) => [k, stable(v)]));
  return j;
}
let parity = 0;
function compare(op, doc, flags = {}, label = '') {
  const w = call(op, JSON.stringify({ ...doc, flags })); contract(w);
  if (!native) return w;
  const cmd = ['decide', 'run', 'evaluate'][op];
  const args = [cmd, '--threads', '4'];
  for (const [key, value] of Object.entries(flags)) args.push('--' + key.replaceAll('_', '-'), String(value));
  const { PROBBIT_CONFIG: _config, PROBBIT_THREADS: _threads, ...env } = process.env;
  const r = spawnSync(native, args, { input: JSON.stringify(doc), encoding: 'utf8', env });
  assert.equal(r.status, w.code, `${label}: ${r.stderr}`);
  assert.deepEqual(stable(JSON.parse(r.stdout)), stable(w.out), label);
  parity++; return w;
}
compare(1, base, {}, 'exact');
compare(1, base, { op: 'sample', sweeps: 1, polish_ms: 0 }, 'refusal');
compare(1, precedence, {}, 'infeasible');
const demo = call(3, '{"tasks":300,"seed":7}').out;
compare(0, demo, { mode: 'sample', sweeps: 1000, polish_sweeps: 50 }, 'router partial');
for (const [file, op] of [['knapsack-20.json', 1], ['agent-plan-6.json', 1], ['evaluate/support-12.json', 2]]) {
  const doc = JSON.parse(readFileSync(new URL('../../examples/' + file, import.meta.url)));
  compare(op, doc, {}, file + ' exact');
  compare(op, doc, { op: 'sample', sweeps: 1000, polish_sweeps: 50 }, file + ' fixed-work');
}
if (native) {
  const n = spawnSync(native, ['demo', '--tasks', '1', '--seed', '4294967296'], { encoding: 'utf8' });
  assert.equal(n.status, 0); assert.deepEqual(JSON.parse(n.stdout), seed64.out); parity++;
}
console.log(JSON.stringify({ wasm_calls: calls, native_parity_cases: parity, boundaries: 'passed' }));
