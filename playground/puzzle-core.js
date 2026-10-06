// The engine side of puzzle.html, shared with check.mjs (which loads this file in a Node vm context): pure functions over a
// `call(op, text)` that runs one probbit-wasm op and returns its output text. No dependencies; a classic script (no modules), so
// the page also works opened from disk.
//
// "The final stance changes" means a trait's selected level or the stance line differs at the last turn (moods are shown, not
// counted). A puzzle is two 4-turn persona replays (docs/persona.md section 7, `probbit persona replay`) of one individual (persona + seed)
// whose scripts differ in exactly one earlier event. A branch's trace is the CLI's stdout: one canonical stance per line, each
// ending in "\n"; its sha256 is the one `probbit persona replay` prints on stderr, next to the final state digest.
var PuzzleCore = (function () {
  "use strict";

  // The shipped puzzles: one per example persona, card 3 changed from praise to error, the seed picked after the seeds 0-49 sweep
  // (a seed where the final stance changes, so a selected example). `expect` holds the native CLI's values, pinned with
  // `cargo run --release -p probbit-cli -- persona replay examples/persona/NAME.json --seed S --script SCRIPT [--no-inertia]`:
  // trace = sha256 of stdout, stance = sha256 of the last stdout line (no newline), state = the final state digest (stderr).
  // `sweep` = the seeds of 0-49 whose final stance changes under this edit (check.mjs recomputes it).
  var PRAISE3 = [{ praise: true }, { praise: true }, { praise: true }, {}], ERROR3 = [{ praise: true }, { praise: true }, { error: true }, {}];
  var DEFAULTS = {
    "tutor": { seed: 21, a: PRAISE3, b: ERROR3, sweep: [21, 43], expect: {
      a: { trace: "2fc0ce4f0770e00ecb7af912aadbe8c6cc742423eadd94734bf055c362bd11f9", stance: "b7d36da5ffd3d5c6a4ce30930de2880a3327f6282b0873b7bf54744557de4019", state: "sha256:bfae3adac61a7c145be71b299c2bc0098675d56a2fc74502824836bef17616e2" },
      b: { trace: "791a32dd686074eb60fb6b3c7b16740dff5181a063470c68e2caa31a64e14bcb", stance: "274462110b484a0169f2adf982f49563196ec1f9a0c0d10232945ce534db1463", state: "sha256:4a5f5c330a0c7b93402b5f7c74aeb910f8ff3e796bee7f63f008a1e96d04a028" },
      a_no_inertia: { trace: "d11c77d25db2eab4051e889a9c5a4c45bacae39d357da3b938112702824a25b0", stance: "cad7798e894e950536296201ffd04a1dcc3de9a8fd715ac3d856dc338ca74a27", state: "sha256:d5d57d067e51522fdd5948c1abca67190708e291bd3a7024cc41758d75d073fe" },
      b_no_inertia: { trace: "2aaa721ff5ed93811acea4c56c613431cf732218b248cced5819ec01fe730cc5", stance: "1311d64bc6bffa51652b631ee75e22509017563da2eb6a959cbbe6455abd7a4c", state: "sha256:6590176cc724eaddb775e2429da233953b40f870e2b1cca23ffe67b3baa22ede" } } },
    "ops-engineer": { seed: 16, a: PRAISE3, b: ERROR3, sweep: [0, 16, 45], expect: {
      a: { trace: "138281d13055d0465046bb80d658426d9e55ad1bca3dd59e67dcd6b6ca03da52", stance: "c9eba4328bfadd0b0e1011198085d51e4593d24b9d18c4030689c796692fad14", state: "sha256:08189bb0169a7027395c5d1c41fcbe080c6cf10bb96cde5ac6c6243a36273e67" },
      b: { trace: "bed713578ae838c8f3073aa3efd85947cf608c5186c108f25c5bebd85939ac4e", stance: "c77e7879fd35bd89a372b10e610a6b80dce166413352d84a119c09081aef5c92", state: "sha256:f1314d1cdb3220d8f9d09d7048b3299a5ac1c3198b9a9eccdc2f54fd05397ad4" },
      a_no_inertia: { trace: "98aede345c9d7734769165b7c06da14c838a7dd0236045f957fefd52da50e0ea", stance: "36d29254c6699b337f811559819f59a6f638aa558a029ce8deac89363f68a0f4", state: "sha256:722bbd9e31f5e1e55bb2c93db1e642de0e8ef93ad2af164574666cc480ff36c1" },
      b_no_inertia: { trace: "436e7cf8bf68c3e9185c0741336de4554711b8ed8e0f172acbb984829056966a", stance: "e035878452400742f3d7c867edff5abd75ba55c32912c78b23c966d40a06e8a9", state: "sha256:d4c3c2cb1782e521c9f7eab3d8bb9a37f0294f81dda21281f6c1a0453e71231b" } } },
    "trader-assistant": { seed: 36, a: PRAISE3, b: ERROR3, sweep: [36, 42], expect: {
      a: { trace: "5e65ab2bb3f3dd2f8851b26324f4a9cff5999fc87dbf3b35e295b8d02c1ac6e5", stance: "caf7cb7c708f2bb4dd7744e213aafa0961fce8dfa53e20e1bb8d8307e69c27ac", state: "sha256:03dfb7a4f5478f0e451fedaacd0d2f218052c745ca8645045361bd1314ebf89d" },
      b: { trace: "ff2fd42fa894af733ddaa887667f508f90e217e2ea0ff3117538aff6462d375e", stance: "e8f0951c1208e1b93afa57d6775a5776ececc98e47a2c6c159921d3a715d5631", state: "sha256:6384e42232b4c11af3f93a8996cd5d9c137ba9e82477b799049803cf5a1e7fce" },
      a_no_inertia: { trace: "38baae4fa5b62cbf1dfc09f8efc67f1147a2fca2587b5b02eb77b53f7d546558", stance: "203c44f3e68a719c032648da0812de3aaa95ed83cf1fcef774983d7972b7f8cf", state: "sha256:dfbfdf44fffb36538ec718666483a5e07db485e04e9817c0cb2997bf9cbb8432" },
      b_no_inertia: { trace: "0d10ed9f4ae332396a437eef4d9f4ac07ab3fb6953d9732cfaa99156003d7f31", stance: "7770e1be2bc31e90abd0f356d9ac46fc829441a0dd13b356ed0c15e43fd5a9f7", state: "sha256:edba94fdd7cd25df18dba248f4bf862b8527344c3836ed7fbdb24da1d0f93573" } } }
  };
  var SWEEP_SEEDS = 50;

  // SHA-256 of a string's UTF-8 bytes, as lowercase hex (FIPS 180-4; crypto.subtle is not available to every page opened from disk).
  var K = [0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
    0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
    0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
    0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2];
  function sha256(text) {
    var m = new TextEncoder().encode(text), n = m.length, len = ((n + 9 + 63) >> 6) << 6, b = new Uint8Array(len), w = new Uint32Array(64);
    b.set(m); b[n] = 0x80; var bits = n * 8;
    for (var i = 0; i < 8; i++) b[len - 1 - i] = Math.floor(bits / Math.pow(2, 8 * i)) & 0xff;
    var h = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    var r = function (x, k) { return (x >>> k) | (x << (32 - k)); };
    for (var o = 0; o < len; o += 64) {
      for (var t = 0; t < 16; t++) w[t] = (b[o + 4 * t] << 24) | (b[o + 4 * t + 1] << 16) | (b[o + 4 * t + 2] << 8) | b[o + 4 * t + 3];
      for (t = 16; t < 64; t++) {
        var s0 = r(w[t - 15], 7) ^ r(w[t - 15], 18) ^ (w[t - 15] >>> 3), s1 = r(w[t - 2], 17) ^ r(w[t - 2], 19) ^ (w[t - 2] >>> 10);
        w[t] = (w[t - 16] + s0 + w[t - 7] + s1) | 0;
      }
      var a = h[0], c = h[1], d = h[2], e = h[3], f = h[4], g = h[5], hh = h[6], k = h[7];
      for (t = 0; t < 64; t++) {
        var t1 = (k + (r(f, 6) ^ r(f, 11) ^ r(f, 25)) + ((f & g) ^ (~f & hh)) + K[t] + w[t]) | 0;
        var t2 = ((r(a, 2) ^ r(a, 13) ^ r(a, 22)) + ((a & c) ^ (a & d) ^ (c & d))) | 0;
        k = hh; hh = g; g = f; f = (e + t1) | 0; e = d; d = c; c = a; a = (t1 + t2) | 0;
      }
      h[0] = (h[0] + a) | 0; h[1] = (h[1] + c) | 0; h[2] = (h[2] + d) | 0; h[3] = (h[3] + e) | 0;
      h[4] = (h[4] + f) | 0; h[5] = (h[5] + g) | 0; h[6] = (h[6] + hh) | 0; h[7] = (h[7] + k) | 0;
    }
    return h.map(function (x) { return (x >>> 0).toString(16).padStart(8, "0"); }).join("");
  }

  // `probbit persona replay PERSONA --seed SEED --script SCRIPT [--no-inertia]` on probbit-wasm ops 4 (init) and 5 (turn). The
  // state passes from turn to turn as the module's own text, and each stance is cut from the turn's canonical output
  // {"stance":...,"state":...} (keys sorted, so the state is last), so the trace is byte for byte what the CLI prints.
  function replay(call, personaText, seed, script, noInertia) {
    var r = call(4, '{"persona":' + personaText + ',"seed":' + seed + "}");
    if (r.code !== 0) throw new Error("persona init: " + r.text);
    var state = r.text, stances = [];
    for (var i = 0; i < script.length; i++) {
      var t = call(5, '{"persona":' + personaText + ',"state":' + state + ',"inputs":' + JSON.stringify(script[i]) + (noInertia ? ',"flags":{"no_inertia":true}' : "") + "}");
      if (t.code !== 0) throw new Error("persona turn " + (i + 1) + ": " + t.text);
      var cut = t.text.lastIndexOf(',"state":{');
      stances.push(t.text.slice('{"stance":'.length, cut)); state = t.text.slice(cut + ',"state":'.length, -1);
    }
    var trace = stances.map(function (s) { return s + "\n"; }).join(""), last = JSON.parse(stances[stances.length - 1]);
    return { stances: stances, final: last, trace: trace, digests: { trace: sha256(trace), stance: sha256(stances[stances.length - 1]), state: JSON.parse(state).digest } };
  }

  // The selected levels: traits (the stance) and moods (the carried state), each by id.
  function levels(stance) {
    var o = {}, k;
    for (k in stance.stance) o[k] = stance.stance[k].level;
    for (k in stance.mood) o[k] = stance.mood[k].level;
    return o;
  }
  // Two final turns compared. The stance changes when a trait's selected level or the line differs (moods are reported, not
  // counted: they are the state carried into the stance).
  function differs(sa, sb) {
    var traits = [], moods = [], k;
    for (k in sa.stance) if (sa.stance[k].level !== sb.stance[k].level) traits.push(k);
    for (k in sa.mood) if (sa.mood[k].level !== sb.mood[k].level) moods.push(k);
    return { traits: traits, moods: moods, line: sa.line !== sb.line, any: traits.length > 0 || sa.line !== sb.line };
  }
  // The script positions (0-based) where the two scripts differ.
  function changedTurns(a, b) {
    var out = [];
    for (var i = 0; i < Math.max(a.length, b.length); i++) if (JSON.stringify(a[i]) !== JSON.stringify(b[i])) out.push(i);
    return out;
  }

  // Both branches of one individual, with and without inertia.
  function solve(call, personaText, seed, a, b) {
    var A = replay(call, personaText, seed, a, false), B = replay(call, personaText, seed, b, false);
    var A0 = replay(call, personaText, seed, a, true), B0 = replay(call, personaText, seed, b, true);
    return { a: A, b: B, a0: A0, b0: B0, diff: differs(A.final, B.final), diff0: differs(A0.final, B0.final) };
  }

  // The same edit at one seed: does the final stance change, and does the difference survive with inertia off?
  function sweepOne(call, personaText, a, b, seed) {
    if (!differs(replay(call, personaText, seed, a, false).final, replay(call, personaText, seed, b, false).final).any) return { changed: false, survives: false };
    return { changed: true, survives: differs(replay(call, personaText, seed, a, true).final, replay(call, personaText, seed, b, true).final).any };
  }
  // ... at every seed 0..n-1: which individuals change their final stance.
  function sweep(call, personaText, a, b, n) {
    var changed = [], survives = [];
    for (var s = 0; s < n; s++) { var r = sweepOne(call, personaText, a, b, s); if (r.changed) changed.push(s); if (r.survives) survives.push(s); }
    return { n: n, changed: changed, no_inertia_still_differs: survives };
  }

  // A POSIX shell word.
  function sq(s) { return "'" + String(s).replace(/'/g, "'\\''") + "'"; }
  function cli(file, seed, script, noInertia) {
    return "probbit persona replay " + file + " --seed " + seed + " --script " + sq(JSON.stringify(script)) + (noInertia ? " --no-inertia" : "");
  }

  // A recipe: everything a stranger needs to reproduce the puzzle, with the values to compare against.
  function recipe(version, personaName, personaDoc, seed, a, b, solved) {
    var file = "examples/persona/" + personaName + ".json", pick = function (r) { return { trace_sha256: r.digests.trace, final_stance_sha256: r.digests.stance, final_state_digest: r.digests.state }; };
    return {
      probbit_puzzle: 1, engine: "probbit " + version,
      persona: { file: file, name: personaDoc.identity.name, version: personaDoc.identity.version, digest: solved.a.final.persona.digest },
      seed: seed, changed_turns: changedTurns(a, b).map(function (i) { return i + 1; }),
      a: { script: a, expect: pick(solved.a), expect_no_inertia: pick(solved.a0) },
      b: { script: b, expect: pick(solved.b), expect_no_inertia: pick(solved.b0) },
      final_stance_differs: solved.diff.any, no_inertia_final_stance_differs: solved.diff0.any,
      cli: [cli(file, seed, a, false), cli(file, seed, b, false), cli(file, seed, a, true), cli(file, seed, b, true)]
    };
  }
  // Recipe fields -> problems found ([] when the recipe is usable). `digests` maps persona name -> digest.
  function checkRecipe(r, digests) {
    var bad = [];
    if (!r || r.probbit_puzzle !== 1) bad.push("not a puzzle recipe (probbit_puzzle: 1)");
    else {
      var name = null;
      for (var k in digests) if (r.persona && digests[k] === r.persona.digest) name = k;
      if (!name) bad.push("persona digest " + (r.persona && r.persona.digest) + " is none of this page's example personas");
      if (!(Number.isInteger(r.seed) && r.seed >= 0 && r.seed <= 9007199254740992)) bad.push("seed must be a whole number from 0");
      ["a", "b"].forEach(function (x) { if (!r[x] || !Array.isArray(r[x].script) || !r[x].script.length) bad.push(x + ".script must be a list of inputs"); });
      if (!bad.length && changedTurns(r.a.script, r.b.script).length === 0) bad.push("the two scripts are identical");
    }
    return { name: name, problems: bad };
  }

  return { DEFAULTS: DEFAULTS, SWEEP_SEEDS: SWEEP_SEEDS, sha256: sha256, replay: replay, levels: levels, differs: differs, changedTurns: changedTurns,
    solve: solve, sweepOne: sweepOne, sweep: sweep, recipe: recipe, checkRecipe: checkRecipe, cli: cli };
})();
if (typeof module !== "undefined") module.exports = PuzzleCore;
