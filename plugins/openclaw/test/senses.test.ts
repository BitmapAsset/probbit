import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import plugin, { registerStance } from "../src/index.js";
import { CONFIG_JSON_SCHEMA, resolveConfig } from "../src/config.js";
import { buildEventLine } from "../src/probbit.js";
import { feedRefusal, heuristicTimePressure, parseDeclaredSenses, tempoFromGaps, tempoNow, type SensesLine } from "../src/senses.js";
import { cliEvent, createHost, GROUP_KEY, groupCtx, PERSONA_SENSES, PERSONA_SRC, PROBBIT, tempStateDir, toolCtx } from "./host.js";

const LANE = "team-room";
const MIN = 60_000;
const T0 = 1_791_320_000_000;

function lines(path: string): string[] {
  return existsSync(path) ? readFileSync(path, "utf8").split("\n").filter((l) => l.trim()) : [];
}

function strandEvents(dir: string): Array<{ n: number; inputs: Record<string, unknown> }> {
  return lines(join(dir, "strands", `${LANE}.strand`))
    .slice(1)
    .map((l) => JSON.parse(l) as { n: number; inputs: Record<string, unknown> });
}

function sensesLines(dir: string): SensesLine[] {
  return lines(join(dir, "strands", `${LANE}.senses.jsonl`)).map((l) => JSON.parse(l) as SensesLine);
}

function logMetas(dir: string): Array<Record<string, unknown>> {
  return lines(join(dir, "log", `${LANE}.jsonl`)).map((l) => JSON.parse(l.split("\t")[3]!) as Record<string, unknown>);
}

/** A replay's own tempo rule, written independently of src/senses.ts: whole micro-hours from the strand, a 60-minute window. */
function replayTempo(events: Array<{ inputs: Record<string, unknown> }>): number[] {
  const t: number[] = [];
  let acc = 0;
  return events.map((e) => {
    acc += Math.round(Number(e.inputs.elapsed_hours ?? 0) * 1e6);
    t.push(acc);
    return t.filter((x) => acc - x <= 1_000_000).length;
  });
}

/** Drives one lane through `turns` ([text, minutes since the previous turn]); returns the dir. */
async function drive(overrides: Record<string, unknown>, turns: Array<[string, number]>, between?: (i: number, host: ReturnType<typeof createHost>) => Promise<void>) {
  const dir = tempStateDir();
  let clock = T0;
  const host = createHost({ lanes: { [GROUP_KEY]: LANE }, stateDir: dir, binary: PROBBIT, ...overrides });
  registerStance(host.api, { now: () => clock });
  for (const [i, [text, minutes]] of turns.entries()) {
    clock += minutes * MIN;
    if (between) await between(i, host);
    const { result, threw } = await host.promptBuild(cliEvent(text), groupCtx(`run-${i + 1}`));
    assert.equal(threw, false);
    assert.ok((result as { appendContext?: string } | undefined)?.appendContext, `turn ${i + 1}: no block (${JSON.stringify(host.logs)})`);
  }
  return { dir, host };
}

const PLAIN: Array<[string, number]> = [
  ["thanks, that looks right", 0],
  ["can you check the build logs when you get a chance?", 10],
  ["fix it asap, the demo is in 20 minutes!", 10],
  ["ok", 30],
  ["the second run looks fine to me", 25],
  ["one more question about the docs", 65],
];

test("senses: the declared senses are read from the persona (fixture yes, bundled persona no block)", () => {
  const s = parseDeclaredSenses(readFileSync(PERSONA_SENSES, "utf8"));
  assert.deepEqual(Object.fromEntries(s ?? []), {
    tempo: { kind: "number", range: [0, 200], src: ["env"] },
    tool_fail: { kind: "flag", src: ["env"] },
  });
  assert.equal(parseDeclaredSenses(readFileSync(PERSONA_SRC, "utf8")), undefined);
  assert.equal(feedRefusal("tempo", s), undefined);
  assert.equal(feedRefusal("tool_fail", s), undefined);
  assert.equal(feedRefusal("tempo", undefined), "not declared");
  const human = parseDeclaredSenses("senses:\n  - id: tempo\n    kind: number\n    range: [0, 50]\n    src: [human]\n  - id: tool_fail\n    kind: number\n    range: [0, 1]\n");
  assert.equal(feedRefusal("tempo", human), "its src does not include env");
  assert.match(feedRefusal("tool_fail", human) ?? "", /declared as number/);
});

test("senses: the tempo rule on a scripted gap sequence, the 60-minute boundary inclusive", () => {
  // minutes after the previous event: 0, 10, 10, 30, 25, 65 -> times 0, 10, 20, 50, 75, 140
  const gaps = [0, 10, 10, 30, 25, 65].map((m) => Math.round((m / 60) * 1e6) / 1e6);
  const tempos = gaps.map((_, i) => tempoFromGaps(gaps.slice(0, i + 1)));
  assert.deepEqual(tempos, [1, 2, 3, 4, 3, 1]);
  assert.equal(tempoFromGaps([0, 1]), 2, "exactly 60 minutes earlier counts");
  assert.equal(tempoFromGaps([0, 1.000001]), 1, "one micro-hour beyond does not");
  assert.equal(tempoFromGaps([0, 0.5, 0.5]), 3);
  assert.equal(tempoFromGaps([0, 0.5, 0.500001]), 2);
  assert.equal(tempoFromGaps([]), 0);
});

test("senses: the time-pressure heuristic on fixed sentences", () => {
  const table: Array<[string, number]> = [
    ["thanks, that looks good", 0],
    ["can you take a look at the logs when you get a chance?", 0],
    ["fix the build", 0.3],
    ["please ship it today", 0.5],
    ["URGENT: the site is down, fix it asap!", 0.6],
    ["restart the gateway right now!", 0.8],
    ["I wrote a long note about the migration plan and what we learned from the last two releases; we need it by tomorrow", 0.4],
    ["we should look at this soon, there are a few open questions about the schema and the importer", 0.2],
  ];
  for (const [text, want] of table) assert.equal(heuristicTimePressure(text).value, want, text);
  assert.deepEqual(heuristicTimePressure("restart the gateway right now!").features, {
    deadline: "strong",
    imperative: true,
    emphasis: true,
    short: true,
  });
});

test("senses config: off and question by default; bad values warn; the schema and the manifest list both keys", () => {
  const d = resolveConfig({}).config;
  assert.deepEqual([d.sensesMode, d.timePressureSource], ["off", "question"]);
  const s = resolveConfig({ sensesMode: "shadow", timePressureSource: "heuristic" }).config;
  assert.deepEqual([s.sensesMode, s.timePressureSource], ["shadow", "heuristic"]);
  const bad = resolveConfig({ sensesMode: "on", timePressureSource: "model" });
  assert.deepEqual([bad.config.sensesMode, bad.config.timePressureSource], ["off", "question"]);
  assert.equal(bad.warnings.length, 2);
  assert.deepEqual(CONFIG_JSON_SCHEMA.properties.sensesMode.enum, ["off", "shadow", "feed"]);
  assert.deepEqual(CONFIG_JSON_SCHEMA.properties.timePressureSource.enum, ["question", "heuristic", "off"]);
  assert.equal(plugin.configSchema.safeParse({ lanes: {}, sensesMode: "feed" }).success, true);
  assert.equal(plugin.configSchema.safeParse({ lanes: {}, sensesMode: "live" }).success, false);
});

test("senses: buildEventLine adds a sorted senses object only when there is a reading", () => {
  assert.equal(buildEventLine(["praise"], 0.5), '{"elapsed_hours":0.5,"praise":true}');
  assert.equal(
    buildEventLine([], 0, {}, { tool_fail: { v: false, src: "env:x" }, tempo: { v: 3, src: "env:x" } }),
    '{"elapsed_hours":0,"senses":{"tempo":{"src":"env:x","v":3},"tool_fail":{"src":"env:x","v":false}}}',
  );
});

test("registration: sensesMode shadow or feed adds the after_tool_call hook; off (default) does not", () => {
  for (const [mode, want] of [
    [undefined, ["before_prompt_build", "before_tool_call"]],
    ["off", ["before_prompt_build", "before_tool_call"]],
    ["shadow", ["before_prompt_build", "before_tool_call", "after_tool_call"]],
    ["feed", ["before_prompt_build", "before_tool_call", "after_tool_call"]],
  ] as const) {
    const host = createHost({ lanes: { [GROUP_KEY]: LANE }, stateDir: tempStateDir(), binary: PROBBIT, ...(mode ? { sensesMode: mode } : {}) });
    registerStance(host.api);
    assert.deepEqual(host.registered, want, String(mode));
  }
});

test("shadow (real probbit): readings logged beside the strand, nothing fed; the strand is byte-identical to sensesMode off", async () => {
  const base = { personaPath: PERSONA_SENSES, timePressureSource: "heuristic" };
  const off = await drive({ ...base, sensesMode: "off" }, PLAIN);
  const shadow = await drive({ ...base, sensesMode: "shadow" }, PLAIN, async (i, host) => {
    if (i === 2) await host.afterToolCall({ toolName: "exec", params: {}, error: "exit 1" }, toolCtx({ runId: "run-2" }));
  });
  const strandOff = readFileSync(join(off.dir, "strands", `${LANE}.strand`), "utf8");
  const strandShadow = readFileSync(join(shadow.dir, "strands", `${LANE}.strand`), "utf8");
  assert.equal(strandShadow, strandOff, "shadow must not change the life");
  assert.equal(existsSync(join(off.dir, "strands", `${LANE}.senses.jsonl`)), false);
  for (const e of strandEvents(shadow.dir)) assert.equal(e.inputs.senses, undefined);

  const sl = sensesLines(shadow.dir);
  assert.equal(sl.length, PLAIN.length);
  assert.deepEqual(sl.map((l) => l.n), [1, 2, 3, 4, 5, 6]);
  assert.deepEqual(sl.map((l) => l.tempo), [1, 2, 3, 4, 3, 1]);
  assert.deepEqual(sl.map((l) => l.tempo), replayTempo(strandEvents(shadow.dir)), "the strand alone reproduces the tempo");
  assert.deepEqual(sl.map((l) => l.tool_fail), [false, false, true, false, false, false]);
  assert.deepEqual(sl.map((l) => l.time_pressure_src), ["none", "none", "heuristic", "none", "none", "none"]);
  assert.equal(sl[2]!.time_pressure, 0.7);
  assert.equal(heuristicTimePressure(PLAIN[2]![0]).value, 0.7);
  assert.ok(sl.every((l) => l.mode === "shadow" && l.fed.length === 0 && l.tempo_src === "strand" && l.tool_fail_src === "after_tool_call"));
  assert.deepEqual(sl[0]!.declared, ["tempo", "tool_fail", "time_pressure"]);
  assert.match(sl[0]!.ts, /^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ$/);
  for (const m of logMetas(shadow.dir)) assert.equal(m.senses, undefined);
  console.log("EVIDENCE shadow " + JSON.stringify(sl));
});

test("feed (real probbit): the declared senses reach the strand, the tempo replays from the strand, verify passes", async () => {
  const { dir, host } = await drive({ personaPath: PERSONA_SENSES, sensesMode: "feed" }, PLAIN, async (i, h) => {
    if (i === 1) await h.afterToolCall({ toolName: "read", params: {}, result: { ok: true } }, toolCtx({ runId: "run-1" }));
    if (i === 3) {
      await h.afterToolCall({ toolName: "exec", params: {}, error: "command failed: exit 2" }, toolCtx({ runId: "run-3" }));
      await h.afterToolCall({ toolName: "exec", params: {}, error: "timeout" }, toolCtx({ runId: "cron-run-9" }));
    }
    if (i === 4) await h.afterToolCall({ toolName: "exec", params: {}, error: "from a cron run" }, toolCtx({ runId: "cron-run-9" }));
  });
  const events = strandEvents(dir);
  const fedTempo = events.map((e) => (e.inputs.senses as Record<string, { v: unknown }>).tempo!.v);
  const fedFail = events.map((e) => (e.inputs.senses as Record<string, { v: unknown }>).tool_fail!.v);
  assert.deepEqual(fedTempo, [1, 2, 3, 4, 3, 1]);
  assert.deepEqual(fedTempo, replayTempo(events));
  assert.deepEqual(fedFail, [false, false, false, true, false, false], "only the event run's failure counts, at the next event");
  for (const e of events) {
    const s = e.inputs.senses as Record<string, { src: unknown }>;
    assert.equal(s.tempo!.src, "env:probbit-stance");
    assert.equal(e.inputs.time_pressure, undefined, "timePressureSource question (default): no heuristic value");
  }
  const verify = JSON.parse(execFileSync(PROBBIT, ["live", "verify", join(dir, "strands", `${LANE}.strand`)], { encoding: "utf8" })) as {
    ok: boolean;
    events: number;
    normals?: Record<string, { n: number }>;
  };
  assert.equal(verify.ok, true);
  assert.equal(verify.events, 6);
  assert.equal(verify.normals?.tempo?.n, 6);
  assert.equal(verify.normals?.tool_fail?.n, 6);
  const sl = sensesLines(dir);
  assert.ok(sl.every((l) => l.mode === "feed" && l.fed.join() === "tempo,tool_fail"));
  assert.deepEqual(sl.map((l) => l.tool_calls), [0, 1, 0, 1, 0, 0]);
  assert.deepEqual(logMetas(dir)[3]!.senses, { tempo: 4, tool_fail: true });
  assert.equal(host.logs.filter((l) => l.level === "warn").length, 0);
  console.log("EVIDENCE feed " + JSON.stringify({ events: events.map((e) => e.inputs), verify }));
});

test("feed (real probbit) without a senses block: the event lines equal sensesMode off; one warn per sense", async () => {
  const off = await drive({ sensesMode: "off" }, PLAIN);
  const feed = await drive({ sensesMode: "feed" }, PLAIN, async (i, h) => {
    if (i === 2) await h.afterToolCall({ toolName: "exec", params: {}, error: "exit 1" }, toolCtx({ runId: "run-2" }));
  });
  assert.equal(
    readFileSync(join(feed.dir, "strands", `${LANE}.strand`), "utf8"),
    readFileSync(join(off.dir, "strands", `${LANE}.strand`), "utf8"),
  );
  const warns = feed.host.logs.filter((l) => l.level === "warn").map((l) => l.message);
  assert.equal(warns.length, 2, JSON.stringify(warns));
  assert.match(warns[0]!, /sense "tempo": not declared/);
  assert.match(warns[1]!, /sense "tool_fail": not declared/);
  const sl = sensesLines(feed.dir);
  assert.ok(sl.every((l) => l.fed.length === 0 && l.declared.join() === "time_pressure"));
  assert.equal(sl[2]!.tool_fail, true, "measured and logged even though this persona does not take it");
});

test("feed + timePressureSource heuristic (real probbit): the heuristic value reaches the event line and the footer", async () => {
  const { dir } = await drive({ sensesMode: "feed", timePressureSource: "heuristic" }, PLAIN);
  const events = strandEvents(dir);
  assert.deepEqual(events.map((e) => e.inputs.time_pressure), [undefined, undefined, 0.7, undefined, undefined, undefined]);
  const metas = logMetas(dir);
  assert.equal(metas[2]!.timePressure, 0.7);
  assert.equal(metas[2]!.timePressureSrc, "heuristic");
  assert.deepEqual(metas[2]!.inputs, ["time 0.70"]);
  assert.deepEqual(sensesLines(dir)[2]!.fed, ["time_pressure"]);
  const shadow = await drive({ sensesMode: "shadow", timePressureSource: "heuristic" }, PLAIN);
  assert.equal(strandEvents(shadow.dir)[2]!.inputs.time_pressure, undefined, "shadow logs the heuristic, never feeds it");
  assert.equal(sensesLines(shadow.dir)[2]!.time_pressure, 0.7);
});

test("feed (real probbit): a tempo above the declared range is sent at the range's top (the engine refuses, never clips)", async () => {
  assert.equal(tempoNow(join(tempStateDir(), "missing.strand"), 0), 1, "no strand yet: this event alone");
  const persona = join(tempStateDir(), "narrow.yaml");
  writeFileSync(persona, readFileSync(PERSONA_SENSES, "utf8").replace("range: [0, 200]", "range: [0, 2]"));
  const { dir } = await drive({ personaPath: persona, sensesMode: "feed" }, [["a", 0], ["b", 1], ["c", 1], ["d", 1]]);
  assert.deepEqual(strandEvents(dir).map((e) => (e.inputs.senses as Record<string, { v: unknown }>).tempo!.v), [1, 2, 2, 2]);
  assert.deepEqual(sensesLines(dir).map((l) => l.tempo), [1, 2, 3, 4], "the senses line keeps the measured count");
});

test("after_tool_call: unknown sessions, a run before any event and a throwing ctx change nothing", async () => {
  const dir = tempStateDir();
  let clock = T0;
  const host = createHost({ lanes: { [GROUP_KEY]: LANE }, stateDir: dir, binary: PROBBIT, personaPath: PERSONA_SENSES, sensesMode: "feed" });
  registerStance(host.api, { now: () => clock });
  assert.equal((await host.afterToolCall({ toolName: "exec", params: {}, error: "x" }, toolCtx({ runId: "early" }))).threw, false);
  assert.equal((await host.afterToolCall({ toolName: "exec", params: {}, error: "x" }, toolCtx({ sessionKey: "agent:other" }))).threw, false);
  assert.equal((await host.afterToolCall(null as never, null as never)).threw, false);
  await host.promptBuild(cliEvent("hello"), groupCtx("run-1"));
  clock += MIN;
  await host.promptBuild(cliEvent("and again"), groupCtx("run-2"));
  const events = strandEvents(dir);
  assert.deepEqual(events.map((e) => (e.inputs.senses as Record<string, { v: unknown }>).tool_fail!.v), [false, false]);
});
