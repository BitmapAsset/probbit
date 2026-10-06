// puzzle.html: "Find the event". The engine work is PuzzleCore's (puzzle-core.js); this file loads the module, draws the board and
// wires the buttons. No dependencies, no server: probbit-wasm.js (the module in base64) and puzzle-personas.js come from
// playground/build.sh.
"use strict";
const $ = id => document.getElementById(id);
const C = PuzzleCore, NAMES = ["tutor", "ops-engineer", "trader-assistant"];
const TEXT = {}; for (const n of NAMES) TEXT[n] = JSON.stringify(PUZZLE_PERSONAS[n]);
let call = null, moduleBytes = 0, digests = {};
// The puzzle on the board: persona, seed, the two scripts, the solved branches, the guess, and where the expected digests come from.
let cur = null;
const sweeps = {};  // persona|scriptA|scriptB -> the sweep result

async function load() {
  const s = atob(PROBBIT_WASM_B64), bytes = new Uint8Array(s.length);
  for (let i = 0; i < s.length; i++) bytes[i] = s.charCodeAt(i);
  moduleBytes = bytes.length;
  const imports = { probbit: { now_ms: () => performance.now() } };
  let instance;  // synchronously where the browser allows it, else asynchronously (as index.html)
  try { instance = new WebAssembly.Instance(new WebAssembly.Module(bytes), imports); } catch (e) { ({ instance } = await WebAssembly.instantiate(bytes, imports)); }
  const ex = instance.exports;
  call = (op, text) => {
    const input = new TextEncoder().encode(text), p = ex.probbit_alloc(input.length);
    new Uint8Array(ex.memory.buffer, p, input.length).set(input);
    const n = ex.probbit_call(op, p, input.length);
    return { code: ex.probbit_out_code(), text: new TextDecoder().decode(new Uint8Array(ex.memory.buffer, ex.probbit_out_ptr(), n)) };
  };
  for (const n of NAMES) digests[n] = JSON.parse(call(4, '{"persona":' + TEXT[n] + ',"seed":0}').text).persona.digest;
}

const el = (tag, cls, text) => { const e = document.createElement(tag); if (cls) e.className = cls; if (text !== undefined) e.textContent = text; return e; };
const same = (x, y) => JSON.stringify(x) === JSON.stringify(y);
const sweepKey = (name, a, b) => name + "|" + JSON.stringify(a) + "|" + JSON.stringify(b);
const fmt = p => p.toFixed(3);
// An event (one turn's inputs) -> [short label, the persona's own words for it].
function eventLabel(name, inputs) {
  const ks = Object.keys(inputs);
  if (!ks.length) return ["quiet", "no flags"];
  const decl = PUZZLE_PERSONAS[name].inputs || [];
  return [ks.map(k => inputs[k] === true ? k : k + ": " + inputs[k]).join(", "),
    ks.map(k => { const d = decl.find(x => x.id === k); return d && typeof d.say === "string" ? d.say : ""; }).filter(Boolean).join("; ")];
}
// [trait or mood id, its levels] in the persona's order.
const vars = name => [...PUZZLE_PERSONAS[name].traits, ...(PUZZLE_PERSONAS[name].moods || [])].map(v => [v.id, v.levels]);
const odds = (stance, id) => (stance.stance[id] || stance.mood[id]);
function isDefault(c) { const d = C.DEFAULTS[c.name]; return c.seed === d.seed && same(c.a, d.a) && same(c.b, d.b); }

function setPuzzle(name, seed, a, b, opts) {
  opts = opts || {};
  cur = { name, seed, a, b, solved: C.solve(call, TEXT[name], seed, a, b), guess: null, revealed: false, picked: !!opts.picked, recipeExpect: opts.expect || null };
  $("persona").value = name; $("seed").value = seed;
  render();
}

function render() {
  const { name, seed, a, b, solved } = cur, changed = C.changedTurns(a, b), d = solved.diff;
  const def = isDefault(cur), sw = sweeps[sweepKey(name, a, b)];
  const tag = $("exampleTag");
  tag.className = "tag" + (def || cur.picked ? " sel" : "");
  tag.textContent = def || cur.picked ? "selected example" : "seed you chose";
  const n = def ? C.DEFAULTS[name].sweep.length : sw ? sw.changed.length : null;
  $("captionExample").textContent = (def || cur.picked ? "Selected example, picked after a sweep" : "Seed " + seed + ", your choice") +
    (n !== null ? "; with this edit " + n + " of " + C.SWEEP_SEEDS + " seeds change their final stance." : ".");
  $("ask").textContent = d.any
    ? "One of the earlier events differs between branch A and branch B. Click the card you think changed."
    : "These two branches end in the same stance: no selected stance change for this seed. That is a valid result; the sweep below counts how often it happens.";
  for (const [side, script, r] of [["A", a, solved.a], ["B", b, solved.b]]) {
    const box = $("cards" + side); box.textContent = ""; box.style.gridTemplateColumns = "repeat(" + Math.min(script.length, 4) + ", 1fr)";
    script.forEach((inputs, i) => {
      const last = i === script.length - 1, open = last || cur.revealed, [lab, say] = eventLabel(name, inputs);
      const card = el(open ? "div" : "button", "card" + (open ? "" : " back") + (last ? " same" : "") + (cur.revealed && changed.includes(i) ? " changed" : "") + (cur.guess === i ? " guess" : ""));
      card.appendChild(el("span", "n", last ? "final message" : "card " + (i + 1)));
      card.appendChild(el("span", "ev", open ? lab : "?"));
      if (open) card.appendChild(el("span", "n", last && !changed.includes(i) ? "same in A and B" : say));
      if (!open) { card.type = "button"; card.setAttribute("aria-label", "guess card " + (i + 1)); card.onclick = () => { cur.guess = i; reveal(); }; }
      box.appendChild(card);
    });
    $("line" + side).textContent = r.final.line + (r.final.status !== "ok" ? "  (status: " + r.final.status + ")" : "");
    const lv = $("lv" + side); lv.textContent = "";
    for (const [id] of vars(name)) { lv.appendChild(el("span", d.traits.includes(id) ? "d" : "", id)); lv.appendChild(el("span", d.traits.includes(id) ? "d" : "", odds(r.final, id).level)); }
  }
  $("after").classList.toggle("hidden", !cur.revealed);
  $("reveal").classList.toggle("hidden", cur.revealed);
  if (cur.revealed) drawAfter();
  $("ablation").classList.add("hidden"); $("replayOut").classList.add("hidden");
  drawSweep(); drawRecipe(); syncEditor();
}

function reveal() { cur.revealed = true; render(); $("after").scrollIntoView({ behavior: "smooth", block: "start" }); }

function drawAfter() {
  const { name, a, b, solved } = cur, changed = C.changedTurns(a, b), d = solved.diff, cards = changed.map(i => "card " + (i + 1)).join(" and ");
  const v = $("verdict"); v.textContent = "";
  if (cur.guess === null) v.append("The change was ", el("b", "", cards), ".");
  else if (changed.includes(cur.guess)) v.append("Yes: ", el("b", "", cards), " changed.");
  else v.append("Not card " + (cur.guess + 1) + ": the change was ", el("b", "", cards), ".");
  const what = changed.map(i => "Card " + (i + 1) + " was " + eventLabel(name, a[i])[0] + " in branch A and " + eventLabel(name, b[i])[0] + " in branch B.").join(" ");
  let shift = null;  // the largest odds difference at the final turn
  for (const [id, lvls] of vars(name)) for (const l of lvls) {
    const x = odds(solved.a.final, id).odds[l], y = odds(solved.b.final, id).odds[l];
    if (!shift || Math.abs(y - x) > Math.abs(shift[3] - shift[2])) shift = [id, l, x, y];
  }
  const lv = id => [odds(solved.a.final, id).level, odds(solved.b.final, id).level];
  $("changeText").textContent = what + " " + (d.any
    ? "At the identical final message: " + d.traits.map(id => id + " " + lv(id)[0] + " → " + lv(id)[1]).join(", ") + (d.line ? "; the stance line changed." : ".")
    : "The final stances are the same.") +
    " Largest odds shift: P(" + shift[0] + " = " + shift[1] + ") " + fmt(shift[2]) + " → " + fmt(shift[3]) + ".";
  const box = $("odds"); box.textContent = "";
  for (const [id, lvls] of vars(name)) {
    box.appendChild(el("span", "name" + (d.traits.includes(id) ? " d" : ""), id));
    const pair = el("div", "pair");
    for (const [side, r] of [["A", solved.a], ["B", solved.b]]) {
      const o = odds(r.final, id), bar = el("div", "ob"); bar.appendChild(el("em", "", side));
      lvls.forEach((l, k) => {
        const p = o.odds[l], seg = el("i", "s" + k + (l === o.level ? " pick" : ""), p >= 0.14 ? l : "");
        seg.style.width = (p * 100) + "%"; seg.title = side + ": " + id + " = " + l + ", p " + p; bar.appendChild(seg);
      });
      pair.appendChild(bar);
    }
    box.appendChild(pair);
  }
}

function ablate() {
  const { solved } = cur, d0 = solved.diff0, box = $("ablation"); box.textContent = ""; box.classList.remove("hidden");
  box.appendChild(el("p", d0.any ? "bad" : "ok", d0.any
    ? "With inertia off the final stances still differ (" + (d0.traits.join(", ") || "the line") + "): here the difference does not come from the carried mood alone."
    : "With inertia off the final stances match: every level and the line. In this example the difference came from the mood carried from turn to turn."));
  const t = el("table"); t.innerHTML = "<tr><th></th><th>with inertia</th><th>inertia off</th></tr>";
  for (const [side, r, r0] of [["A", solved.a, solved.a0], ["B", solved.b, solved.b0]]) {
    const tr = el("tr"); tr.append(el("th", "", side), el("td", "", r.final.line), el("td", "", r0.final.line)); t.appendChild(tr);
  }
  box.appendChild(t);
  box.appendChild(el("p", "muted", "The earlier turns still differ with inertia off (the events still change those turns' stances); the claim is about the final stance."));
}

// The digests to compare a replay against: the native CLI's pinned values (shipped examples) or an imported recipe's.
function expected() {
  if (isDefault(cur)) { const e = C.DEFAULTS[cur.name].expect; return { from: "native CLI, pinned", a: e.a, b: e.b }; }
  if (cur.recipeExpect) return { from: "the recipe", a: cur.recipeExpect.a, b: cur.recipeExpect.b };
  return null;
}
function replayBoth() {
  const box = $("replayOut"); box.textContent = ""; box.classList.remove("hidden");
  const ex = expected(), t = el("table");
  t.innerHTML = "<tr><th>branch</th><th>trace sha256, run 1</th><th>run 2</th><th>same bytes</th>" + (ex ? "<th>" + ex.from + "</th>" : "") + "</tr>";
  let all = true;
  for (const side of ["a", "b"]) {
    const r1 = C.replay(call, TEXT[cur.name], cur.seed, cur[side], false), r2 = C.replay(call, TEXT[cur.name], cur.seed, cur[side], false);
    const ok = r1.trace === r2.trace && r1.trace === cur.solved[side].trace, tr = el("tr");
    const code = x => { const td = el("td"); td.appendChild(el("code", "", x)); return td; };
    tr.append(el("th", "", side.toUpperCase()), code(r1.digests.trace), code(r2.digests.trace), el("td", ok ? "ok" : "bad", ok ? "yes" : "no"));
    if (ex) { const m = ex[side].trace === r1.digests.trace && ex[side].state === r1.digests.state; all = all && m; tr.appendChild(el("td", m ? "ok" : "bad", m ? "matches" : "differs")); }
    all = all && ok; t.appendChild(tr);
  }
  box.appendChild(t);
  box.appendChild(el("p", "muted", "Final state digests: A " + cur.solved.a.digests.state + ", B " + cur.solved.b.digests.state + ". " +
    (ex ? (all ? "Both branches repeat byte for byte and equal " + ex.from + "." : "A value differs from " + ex.from + ".") : "Both branches repeat byte for byte" + (all ? "." : ": no.") + " Download the recipe to pin these values.")));
}

const tick = () => new Promise(r => setTimeout(r, 0));
async function runSweep(name, a, b) {
  const key = sweepKey(name, a, b); if (sweeps[key]) return sweeps[key];
  const out = { n: C.SWEEP_SEEDS, changed: [], survives: [] };
  for (let s = 0; s < C.SWEEP_SEEDS; s++) {
    const r = C.sweepOne(call, TEXT[name], a, b, s); if (r.changed) out.changed.push(s); if (r.survives) out.survives.push(s);
    if (s % 5 === 4) { $("sweepProg").textContent = "seed " + (s + 1) + " of " + C.SWEEP_SEEDS + "..."; await tick(); }
  }
  $("sweepProg").textContent = "";
  return (sweeps[key] = out);
}
function drawSweep() {
  const sw = cur && sweeps[sweepKey(cur.name, cur.a, cur.b)];
  $("sweepOut").classList.toggle("hidden", !sw); if (!sw) return;
  const big = $("sweepBig"); big.textContent = ""; big.append(el("b", "", sw.changed.length + " of " + sw.n), " individuals change their final stance");
  const cards = C.changedTurns(cur.a, cur.b).map(i => "card " + (i + 1) + ": " + eventLabel(cur.name, cur.a[i])[0] + " → " + eventLabel(cur.name, cur.b[i])[0]).join(", ");
  $("sweepSub").textContent = "Persona " + cur.name + ", seeds 0-49, the edit " + cards + ". " + (sw.n - sw.changed.length) + " of " + sw.n + " end in the same stance. " +
    "With inertia off, " + sw.survives.length + " of the " + sw.changed.length + " still differ.";
  const g = $("grid50"); g.textContent = "";
  for (let s = 0; s < sw.n; s++) {
    const bt = el("button", (sw.changed.includes(s) ? "hit" : "") + (s === cur.seed ? " cur" : ""), String(s)); bt.type = "button";
    bt.onclick = () => setPuzzle(cur.name, s, cur.a, cur.b, { picked: sw.changed.includes(s) }); g.appendChild(bt);
  }
}

async function busy(fn) { const bs = [...document.querySelectorAll("button")]; bs.forEach(b => b.disabled = true); try { await fn(); } finally { bs.forEach(b => b.disabled = false); } }

// "New puzzle": card k of three praise events turns to error, at a seed where that changes the final stance.
async function shuffle() {
  const name = cur.name, order = [0, 1, 2].sort(() => Math.random() - 0.5);
  for (const k of order) {
    const a = [{ praise: true }, { praise: true }, { praise: true }, {}], b = a.map((x, i) => i === k ? { error: true } : x);
    const sw = await runSweep(name, a, b), pool = sw.changed.filter(s => !(s === cur.seed && same(b, cur.b)));
    if (pool.length) { setPuzzle(name, pool[Math.floor(Math.random() * pool.length)], a, b, { picked: true }); return; }
  }
  $("ask").textContent = "No seed in 0-49 changes its final stance under these edits for this persona.";
}

function eventOptions(name) { return [{}].concat((PUZZLE_PERSONAS[name].inputs || []).filter(x => x.kind === "flag").map(x => ({ [x.id]: true }))); }
function syncEditor() {
  const box = $("edit"), name = cur.name, opts = eventOptions(name); box.textContent = "";
  const sel = (label, id, values, pick) => {
    const l = el("label", "", label), s = el("select"); s.id = id;
    values.forEach((v, i) => { const o = el("option", "", typeof v === "string" ? v : eventLabel(name, v)[0]); o.value = i; if (pick(v)) o.selected = true; s.appendChild(o); });
    l.appendChild(s); box.appendChild(l);
  };
  const k = C.changedTurns(cur.a, cur.b)[0] ?? 2, three = cur.a.length === 4;
  for (let i = 0; i < 3; i++) sel("card " + (i + 1) + " (both)", "ed" + i, opts, v => three && same(v, cur.a[i]));
  sel("card to change", "edk", ["card 1", "card 2", "card 3"], v => v === "card " + (k + 1));
  sel("in branch B it becomes", "edv", opts, v => three && same(v, cur.b[k]));
}
function build() {
  const opts = eventOptions(cur.name), a = [0, 1, 2].map(i => opts[+$("ed" + i).value]).concat([{}]), k = +$("edk").value, b = a.slice();
  b[k] = opts[+$("edv").value];
  if (same(a[k], b[k])) { $("buildMsg").textContent = "Pick a different event for branch B on the changed card."; return; }
  $("buildMsg").textContent = "";
  setPuzzle(cur.name, cur.seed, a, b);
}

function recipeNow() { return C.recipe(PROBBIT_VERSION, cur.name, PUZZLE_PERSONAS[cur.name], cur.seed, cur.a, cur.b, cur.solved); }
function drawRecipe() { const r = recipeNow(); $("recipe").value = JSON.stringify(r, null, 1); $("cli").textContent = r.cli.join("\n"); }
function download() {
  const blob = new Blob([JSON.stringify(recipeNow(), null, 1) + "\n"], { type: "application/json" }), u = URL.createObjectURL(blob), link = el("a");
  link.href = u; link.download = "probbit-puzzle-" + cur.name + "-seed" + cur.seed + ".json"; document.body.appendChild(link); link.click(); link.remove();
  setTimeout(() => URL.revokeObjectURL(u), 1000);
}
function importRecipe(text) {
  const msg = $("importMsg"); let r;
  try { r = JSON.parse(text); } catch (e) { msg.textContent = "Not JSON: " + e.message; msg.className = "bad"; return; }
  const chk = C.checkRecipe(r, digests);
  if (chk.problems.length) { msg.textContent = chk.problems.join("; "); msg.className = "bad"; return; }
  const exp = { a: { trace: r.a.expect && r.a.expect.trace_sha256, state: r.a.expect && r.a.expect.final_state_digest }, b: { trace: r.b.expect && r.b.expect.trace_sha256, state: r.b.expect && r.b.expect.final_state_digest } };
  setPuzzle(chk.name, r.seed, r.a.script, r.b.script, { expect: exp });
  const ok = ["a", "b"].every(x => exp[x].trace === cur.solved[x].digests.trace && exp[x].state === cur.solved[x].digests.state);
  msg.className = ok ? "ok" : "bad";
  msg.textContent = ok ? "Loaded and recomputed in this tab: both traces and final states equal the recipe's digests." : "Loaded, but a recomputed digest differs from the recipe's (or the recipe carries none).";
  window.scrollTo({ top: 0, behavior: "smooth" });
}

async function main() {
  const t = performance.now(); await load();
  for (const n of NAMES) { const o = el("option", "", n + " (" + PUZZLE_PERSONAS[n].identity.name + ")"); o.value = n; $("persona").appendChild(o); }
  const d = C.DEFAULTS.tutor; setPuzzle("tutor", d.seed, d.a, d.b);
  $("engineInfo").textContent = "probbit " + PROBBIT_VERSION + " as a " + moduleBytes.toLocaleString("en-US") + "-byte WebAssembly module, no threads; loaded and the puzzle solved in " +
    Math.round(performance.now() - t) + " ms. Each branch is one persona init and four turns.";
  $("persona").onchange = () => { const n = $("persona").value, x = C.DEFAULTS[n]; setPuzzle(n, x.seed, x.a, x.b); };
  $("go").onclick = () => { const s = Number($("seed").value); if (Number.isInteger(s) && s >= 0) setPuzzle(cur.name, s, cur.a, cur.b); };
  $("seed").onkeydown = e => { if (e.key === "Enter") $("go").click(); };
  $("reveal").onclick = reveal;
  $("ablate").onclick = ablate;
  $("replay").onclick = replayBoth;
  $("sweep").onclick = () => busy(async () => { await runSweep(cur.name, cur.a, cur.b); render(); $("sweepOut").scrollIntoView({ behavior: "smooth", block: "center" }); });
  $("shuffle").onclick = () => busy(shuffle);
  $("build").onclick = build;
  $("export").onclick = download;
  $("import").onclick = () => importRecipe($("recipe").value);
  $("importFile").onchange = async e => { const f = e.target.files[0]; if (f) { $("recipe").value = await f.text(); importRecipe($("recipe").value); } };
}
main().catch(e => {
  document.querySelector(".sub").textContent = typeof PROBBIT_WASM_B64 === "undefined" || typeof PUZZLE_PERSONAS === "undefined"
    ? "The module is not built yet: run sh playground/build.sh, then reload this page." : "Error: " + e.message;
});
