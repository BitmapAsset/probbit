// node playground/check.mjs [path/to/probbit_wasm.wasm] [--print]: load the module as the playground does (no dependencies, Node >= 18), run
// the 300-task demo at 3,200 sweeps, the 12-question evaluate example and the three example personas through the 20-turn workday
// (persona ops 4 / 5: every stance and the final state must equal examples/persona/golden/, the `probbit persona` documents; the
// playground's embedded personas must equal examples/persona/*.json); print one JSON line per call; exit 1 if an answer is off.
import { deepStrictEqual } from 'node:assert';
import { readFileSync } from 'node:fs';
const path = process.argv.slice(2).find(a => !a.startsWith('--')) || new URL('../target/wasm32-unknown-unknown/release/probbit_wasm.wasm', import.meta.url);
const bytes = readFileSync(path);
const { instance } = await WebAssembly.instantiate(bytes, { probbit: { now_ms: () => performance.now() } });
const ex = instance.exports;
function call(op, text) {
  const input = new TextEncoder().encode(text); const p = ex.probbit_alloc(input.length);
  new Uint8Array(ex.memory.buffer, p, input.length).set(input);
  const n = ex.probbit_call(op, p, input.length);
  return { code: ex.probbit_out_code(), out: new TextDecoder().decode(new Uint8Array(ex.memory.buffer, ex.probbit_out_ptr(), n)) };
}
const probbit = { decide: j => call(0, j), run: j => call(1, j), evaluate: j => call(2, j), demo: j => call(3, j), persona_init: j => call(4, j), persona_turn: j => call(5, j) };
const demo = JSON.parse(probbit.demo('{"tasks": 300, "seed": 7}').out);
let t = performance.now(); const d = probbit.decide(JSON.stringify({ ...demo, flags: { sweeps: 3200, polish_ms: 0 } })); const wall = performance.now() - t;
const doc = JSON.parse(d.out);
console.log(JSON.stringify({ call: 'decide 300 tasks, sweeps 3200, polish_ms 0', code: d.code, verdict: doc.verdict, violations: doc.violations, sweeps: doc.gate.sweeps, ms: doc.ms, wall_ms: Math.round(wall * 10) / 10, wasm_bytes: bytes.length }));
const ev = JSON.parse(readFileSync(new URL('../examples/evaluate/support-12.json', import.meta.url)));
t = performance.now(); const e = probbit.evaluate(JSON.stringify(ev)); const ewall = performance.now() - t; const edoc = JSON.parse(e.out);
console.log(JSON.stringify({ call: 'evaluate support-12', code: e.code, verdict: edoc.verdict, violations: edoc.violations, team: edoc.answers.team.probbit, wall_ms: Math.round(ewall * 100) / 100 }));
let personaOk = true;
const ex_ = p => readFileSync(new URL('../examples/persona/' + p, import.meta.url), 'utf8');
const page = readFileSync(new URL('./index.html', import.meta.url), 'utf8');
const embedded = JSON.parse(page.match(/^const PERSONAS = (.*);$/m)[1]);
const turns = JSON.parse(ex_('workday.json')).turns;
for (const name of ['ops-engineer', 'tutor', 'trader-assistant']) {
  const doc = JSON.parse(ex_(name + '.json')); let same = 0, n = 0;
  try { deepStrictEqual(embedded[name], doc); } catch (err) { personaOk = false; console.log(JSON.stringify({ call: 'playground persona ' + name, error: 'the embedded copy differs from examples/persona/' + name + '.json' })); }
  const gold = ex_('golden/' + name + '/workday.jsonl').trim().split('\n').map(l => JSON.parse(l));
  t = performance.now(); let state = JSON.parse(probbit.persona_init(JSON.stringify({ persona: doc })).out);
  for (const [i, inputs] of turns.entries()) {
    const r = probbit.persona_turn(JSON.stringify({ persona: doc, state, inputs })); const o = JSON.parse(r.out); n++;
    try { deepStrictEqual(o.stance, gold[i]); same++; } catch (err) { personaOk = false; }
    state = o.state;
  }
  const pwall = performance.now() - t;
  try { deepStrictEqual(state, JSON.parse(ex_('golden/' + name + '/final-state.json'))); } catch (err) { personaOk = false; }
  console.log(JSON.stringify({ call: 'persona ' + name + ', 20 workday turns', stances_equal_to_golden: same + '/' + n, wall_ms: Math.round(pwall * 10) / 10 }));
}
if (process.argv.includes('--print')) console.log(d.out);
if (!personaOk || d.code !== 0 || doc.violations !== 0 || doc.verdict !== 'diagnostics_passed' || e.code !== 0 || edoc.verdict !== 'exact' || edoc.answers.team.probbit.value !== 'technical') process.exit(1);
