// node playground/check.mjs [path/to/probbit_wasm.wasm] [--print]: load the module as the playground does (no dependencies, Node >= 18), run
// the 300-task demo at 3,200 sweeps, the 12-question evaluate example and the three example personas through the 20-turn workday
// (persona ops 4 / 5: every stance and the final state must equal examples/persona/golden/, the `probbit persona` documents; the
// playground's embedded personas must equal examples/persona/*.json), then puzzle.html's shipped puzzles with the page's own
// puzzle-core.js (each default recipe's two branches, with and without inertia, must give the native CLI's pinned trace, final
// stance and final state digests; its seeds 0-49 sweep must give the pinned denominator); print one JSON line per call; exit 1 if
// an answer is off.
import { deepStrictEqual } from 'node:assert';
import { readFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import vm from 'node:vm';
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
// puzzle.html: the default recipe per persona, replayed by the page's code on this module, against the native CLI's digests.
const core = vm.runInNewContext(readFileSync(new URL('./puzzle-core.js', import.meta.url), 'utf8') + ';PuzzleCore', { TextEncoder });
const pcall = (op, text) => { const r = call(op, text); return { code: r.code, text: r.out }; };
let puzzleOk = readFileSync(new URL('./puzzle.html', import.meta.url), 'utf8').includes('Persona engine. No language model in this view.');
for (const name of ['tutor', 'ops-engineer', 'trader-assistant']) {
  const d = core.DEFAULTS[name], text = JSON.stringify(JSON.parse(ex_(name + '.json')));
  t = performance.now(); const s = core.solve(pcall, text, d.seed, d.a, d.b); let same = 0;
  for (const [k, r] of [['a', s.a], ['b', s.b], ['a_no_inertia', s.a0], ['b_no_inertia', s.b0]]) {
    const e = d.expect[k], ok = r.digests.trace === e.trace && r.digests.stance === e.stance && r.digests.state === e.state &&
      createHash('sha256').update(r.trace).digest('hex') === e.trace && r.trace.split('\n').length === d[k[0]].length + 1;
    if (ok) same++; else puzzleOk = false;
  }
  const sw = core.sweep(pcall, text, d.a, d.b, core.SWEEP_SEEDS), rec = core.recipe('0', name, JSON.parse(text), d.seed, d.a, d.b, s);
  const swOk = JSON.stringify(sw.changed) === JSON.stringify(d.sweep) && sw.changed.includes(d.seed);
  if (!swOk || !s.diff.any || s.diff0.any || core.checkRecipe(rec, { [name]: rec.persona.digest }).problems.length) puzzleOk = false;
  console.log(JSON.stringify({ call: 'puzzle ' + name + ' seed ' + d.seed + ', cards ' + rec.changed_turns.join(','), branches_equal_to_native: same + '/4',
    final_stance_differs: s.diff.any, with_no_inertia: s.diff0.any, sweep: sw.changed.length + '/' + sw.n, sweep_seeds: sw.changed, wall_ms: Math.round(performance.now() - t) }));
}
if (process.argv.includes('--print')) console.log(d.out);
if (!personaOk || !puzzleOk || d.code !== 0 || doc.violations !== 0 || doc.verdict !== 'diagnostics_passed' || e.code !== 0 || edoc.verdict !== 'exact' || edoc.answers.team.probbit.value !== 'technical') process.exit(1);
