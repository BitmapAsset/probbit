// node playground/check.mjs [path/to/pbit_wasm.wasm] [--print]: load the module as the playground does (no dependencies, Node >= 18), run
// the 300-task demo at 3,200 sweeps and the 12-question evaluate example, print one JSON line per call; exit 1 if an answer is off.
import { readFileSync } from 'node:fs';
const path = process.argv.slice(2).find(a => !a.startsWith('--')) || new URL('../target/wasm32-unknown-unknown/release/pbit_wasm.wasm', import.meta.url);
const bytes = readFileSync(path);
const { instance } = await WebAssembly.instantiate(bytes, { pbit: { now_ms: () => performance.now() } });
const ex = instance.exports;
function call(op, text) {
  const input = new TextEncoder().encode(text); const p = ex.pbit_alloc(input.length);
  new Uint8Array(ex.memory.buffer, p, input.length).set(input);
  const n = ex.pbit_call(op, p, input.length);
  return { code: ex.pbit_out_code(), out: new TextDecoder().decode(new Uint8Array(ex.memory.buffer, ex.pbit_out_ptr(), n)) };
}
const pbit = { decide: j => call(0, j), run: j => call(1, j), evaluate: j => call(2, j), demo: j => call(3, j) };
const demo = JSON.parse(pbit.demo('{"tasks": 300, "seed": 7}').out);
let t = performance.now(); const d = pbit.decide(JSON.stringify({ ...demo, flags: { sweeps: 3200, polish_ms: 0 } })); const wall = performance.now() - t;
const doc = JSON.parse(d.out);
console.log(JSON.stringify({ call: 'decide 300 tasks, sweeps 3200, polish_ms 0', code: d.code, verdict: doc.verdict, violations: doc.violations, sweeps: doc.gate.sweeps, ms: doc.ms, wall_ms: Math.round(wall * 10) / 10, wasm_bytes: bytes.length }));
const ev = JSON.parse(readFileSync(new URL('../examples/evaluate/support-12.json', import.meta.url)));
t = performance.now(); const e = pbit.evaluate(JSON.stringify(ev)); const ewall = performance.now() - t; const edoc = JSON.parse(e.out);
console.log(JSON.stringify({ call: 'evaluate support-12', code: e.code, verdict: edoc.verdict, violations: edoc.violations, team: edoc.answers.team.pbit, wall_ms: Math.round(ewall * 100) / 100 }));
if (process.argv.includes('--print')) console.log(d.out);
if (d.code !== 0 || doc.violations !== 0 || doc.verdict !== 'diagnostics_passed' || e.code !== 0 || edoc.verdict !== 'exact' || edoc.answers.team.pbit.value !== 'technical') process.exit(1);
