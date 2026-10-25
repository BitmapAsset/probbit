import assert from "node:assert/strict";
import { existsSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { footerLine, FOOTER_MAX, movedNames } from "../src/block.js";
import { classify, timePressureFrom, type Flag } from "../src/classify.js";
import { resolveConfig } from "../src/config.js";
import { registerStance } from "../src/index.js";
import { createJevEvaluate, JEV_DEFAULT_MODEL, JEV_URL, jevEvaluate, loadJevKey, toJevQuestions, type FetchLike } from "../src/jev.js";
import { cliEvent, createHost, GROUP_KEY, groupCtx, PROBBIT, tempStateDir } from "./host.js";

const FAKE_KEY = "sk-or-test-not-a-real-key";

type Call = { url: string; headers: Record<string, string>; body: Record<string, unknown> };

/** A fetch that answers every question: noul p from `probs` (default 0.05), score from `score` (default 0). */
function mockFetch(opts: { probs?: Record<string, number>; score?: number; status?: number; body?: string; cost?: number } = {}) {
  const calls: Call[] = [];
  const fetch: FetchLike = async (url, init) => {
    const body = JSON.parse(init.body) as { questions: Record<string, { type: string }> };
    calls.push({ url, headers: init.headers, body: body as unknown as Record<string, unknown> });
    if (opts.body !== undefined || opts.status !== undefined) {
      return { ok: (opts.status ?? 200) < 300, status: opts.status ?? 200, text: async () => opts.body ?? "{}" };
    }
    const answers: Record<string, unknown> = {};
    for (const [id, q] of Object.entries(body.questions)) {
      if (q.type === "noul") answers[id] = { type: "noul", noul: opts.probs?.[id] ?? 0.05 };
      if (q.type === "score") answers[id] = { type: "score", score: opts.score ?? 0, probabilities: { "0": 1 } };
    }
    return { ok: true, status: 200, text: async () => JSON.stringify({ answers, usage: { cost: opts.cost ?? 0.00002 } }) };
  };
  return { fetch, calls };
}

/** A fetch that never answers until its signal aborts. */
const hangingFetch: FetchLike = (_url, init) =>
  new Promise((_resolve, reject) => {
    init.signal.addEventListener("abort", () => reject(Object.assign(new Error("aborted"), { name: "AbortError" })));
  });

function keyFile(content: string): string {
  const dir = mkdtempSync(join(tmpdir(), "jev-key-"));
  const p = join(dir, "openrouter.env");
  writeFileSync(p, content);
  return p;
}

test("jev key: plain, export-prefixed and quoted lines; missing file or line -> undefined", () => {
  assert.equal(loadJevKey(keyFile(`OTHER=1\nOPENROUTER_API_KEY=${FAKE_KEY}\n`)), FAKE_KEY);
  assert.equal(loadJevKey(keyFile(`export OPENROUTER_API_KEY="${FAKE_KEY}"\n`)), FAKE_KEY);
  assert.equal(loadJevKey(keyFile(`OPENROUTER_API_KEY='${FAKE_KEY}'`)), FAKE_KEY);
  assert.equal(loadJevKey(keyFile("OTHER=1\n")), undefined);
  assert.equal(loadJevKey(keyFile("OPENROUTER_API_KEY=\n")), undefined);
  assert.equal(loadJevKey("/nonexistent/openrouter.env"), undefined);
});

test("jev questions: boolean -> noul, score passes through, unknown types skipped", () => {
  const out = toJevQuestions({
    praise: { type: "boolean", instructions: "Praise?", criteria: { true: "yes", false: "no" } },
    time_pressure: { type: "score", instructions: "Urgent?", criteria: ["none", "low"] },
    odd: { type: "essay" },
  });
  assert.deepEqual(out, {
    praise: { type: "noul", instructions: "Praise?", criteria: { true: "yes", false: "no" } },
    time_pressure: { type: "score", instructions: "Urgent?", criteria: ["none", "low"] },
  });
});

test("jev adapter ok: pinned model, bearer header, noul -> probabilityTrue, score kept, cost and latency in meta", async () => {
  const { fetch, calls } = mockFetch({ probs: { praise: 0.91 }, score: 3, cost: 0.000016 });
  const out = await jevEvaluate(
    {
      state: { message: "thanks, ship it" },
      questions: {
        praise: { type: "boolean", instructions: "Praise?", criteria: { true: "yes", false: "no" } },
        time_pressure: { type: "score", instructions: "Urgent?", criteria: ["none", "low", "moderate", "high", "urgent"] },
      },
    },
    { apiKey: FAKE_KEY, timeoutMs: 500, fetch },
  );
  assert.equal(out.status, "ok");
  assert.equal(out.result?.answers?.praise?.probabilityTrue, 0.91);
  assert.equal(out.result?.answers?.time_pressure?.score, 3);
  assert.equal(out.meta?.cost, 0.000016);
  assert.equal(out.meta?.model, JEV_DEFAULT_MODEL);
  assert.equal(typeof out.meta?.ms, "number");
  assert.equal(calls.length, 1);
  assert.equal(calls[0]!.url, JEV_URL);
  assert.equal(calls[0]!.body.model, "typesafe/jev-1.13");
  assert.equal(calls[0]!.headers.Authorization, `Bearer ${FAKE_KEY}`);
  assert.equal(JSON.stringify(out).includes(FAKE_KEY), false, "the key never appears in the outcome");
});

test("jev adapter failures -> status error with a reason (timeout, HTTP 402, malformed JSON, no answers, network, no key)", async () => {
  const q = { praise: { type: "boolean", instructions: "Praise?", criteria: { true: "yes", false: "no" } } };
  const req = { state: { message: "hi" }, questions: q };
  const t0 = Date.now();
  const timedOut = await jevEvaluate(req, { apiKey: FAKE_KEY, timeoutMs: 60, fetch: hangingFetch });
  assert.deepEqual([timedOut.status, timedOut.reason], ["error", "timeout"]);
  assert.ok(Date.now() - t0 < 1_000, "the timeout is honoured");
  const paid = await jevEvaluate(req, { apiKey: FAKE_KEY, timeoutMs: 500, fetch: mockFetch({ status: 402, body: '{"error":"credits"}' }).fetch });
  assert.deepEqual([paid.status, paid.reason], ["error", "http-402"]);
  const bad = await jevEvaluate(req, { apiKey: FAKE_KEY, timeoutMs: 500, fetch: mockFetch({ body: "<html>oops" }).fetch });
  assert.deepEqual([bad.status, bad.reason], ["error", "malformed-json"]);
  const empty = await jevEvaluate(req, { apiKey: FAKE_KEY, timeoutMs: 500, fetch: mockFetch({ body: '{"answers":{"praise":{"noul":"high"}}}' }).fetch });
  assert.deepEqual([empty.status, empty.reason], ["error", "malformed-answers"]);
  const net = await jevEvaluate(req, {
    apiKey: FAKE_KEY,
    timeoutMs: 500,
    fetch: async () => {
      throw new TypeError(`fetch failed for ${FAKE_KEY}`);
    },
  });
  assert.deepEqual([net.status, net.reason], ["error", "fetch-TypeError"]);
  assert.equal(JSON.stringify(net).includes(FAKE_KEY), false, "an error message carrying the key is not copied");
  const noKey = await jevEvaluate(req, { apiKey: "", timeoutMs: 500, fetch: mockFetch().fetch });
  assert.deepEqual([noKey.status, noKey.reason], ["error", "no-key"]);
});

test("time_pressure: expected index / 4, 2 decimals; probabilities when score is missing; out of range -> undefined", () => {
  assert.equal(timePressureFrom({ score: 3.99 }), 1);
  assert.equal(timePressureFrom({ score: 2 }), 0.5);
  assert.equal(timePressureFrom({ score: 0.4 }), 0.1);
  assert.equal(timePressureFrom({ score: 0 }), 0);
  assert.equal(timePressureFrom({ probabilities: { "0": 0.5, "2": 0.5 } }), 0.25);
  assert.equal(timePressureFrom({ score: 5 }), undefined);
  assert.equal(timePressureFrom({}), undefined);
  assert.equal(timePressureFrom(undefined), undefined);
});

const CLS = { timeoutMs: 500, threshold: 0.6 };

test("classify shadow: the engine gets the keyword flags; Jev's answers ride along in the report", async () => {
  const { fetch, calls } = mockFetch({ probs: { owner_upset: 0.9, praise: 0.1 }, score: 3 });
  const evaluate = createJevEvaluate({ apiKey: FAKE_KEY, fetch });
  const out = await classify("thanks, the build failed again, fix it now", { ...CLS, evaluate, named: "jev", shadow: true });
  assert.deepEqual(out.flags, ["praise", "failure"] satisfies Flag[]);
  assert.equal(out.method, "keyword");
  assert.equal(out.timePressure, undefined, "shadow never feeds time_pressure");
  assert.equal(out.classifier?.mode, "shadow");
  assert.equal(out.classifier?.ok, true);
  assert.deepEqual(out.classifier?.flags, ["failure", "owner_upset"]);
  assert.equal(out.classifier?.timePressure, 0.75);
  assert.equal(calls.length, 1);
  assert.ok("time_pressure" in (calls[0]!.body.questions as object));
  assert.equal("failure" in (calls[0]!.body.questions as object), false, "failure stays keyword-only");
});

test("classify live: Jev's flags plus keyword failure, and time_pressure, are fed", async () => {
  const evaluate = createJevEvaluate({ apiKey: FAKE_KEY, fetch: mockFetch({ probs: { owner_upset: 0.9 }, score: 4 }).fetch });
  const out = await classify("the build failed again, fix it now", { ...CLS, evaluate, named: "jev", shadow: false });
  assert.deepEqual(out.flags, ["failure", "owner_upset"]);
  assert.equal(out.method, "jev");
  assert.equal(out.timePressure, 1);
  assert.equal(out.probabilities?.owner_upset, 0.9);
});

test("classify: any Jev failure falls back to keywords (live and shadow), with the reason logged", async () => {
  for (const shadow of [false, true]) {
    const evaluate = createJevEvaluate({ apiKey: FAKE_KEY, fetch: mockFetch({ status: 402, body: "{}" }).fetch });
    const out = await classify("that's wrong", { ...CLS, evaluate, named: "jev", shadow });
    assert.deepEqual(out.flags, ["correction"]);
    assert.equal(out.method, "keyword");
    assert.equal(out.decisionNote, "unavailable:http-402");
    assert.equal(out.classifier?.ok, false);
    assert.equal(out.timePressure, undefined);
  }
  const slow = createJevEvaluate({ apiKey: FAKE_KEY, fetch: hangingFetch });
  const late = await classify("thanks", { timeoutMs: 60, threshold: 0.6, evaluate: slow, named: "jev", shadow: false });
  assert.deepEqual(late.flags, ["praise"]);
  assert.match(late.decisionNote ?? "", /timeout|deadline/);
});

test("classify: markers still win outright and Jev is not called; negations stay negated on the keyword path", async () => {
  const { fetch, calls } = mockFetch({ probs: { win: 0.99 } });
  const evaluate = createJevEvaluate({ apiKey: FAKE_KEY, fetch });
  const marked = await classify("[upset] who touched the keys", { ...CLS, evaluate, named: "jev", shadow: false });
  assert.deepEqual([marked.flags, marked.method], [["owner_upset"], "marker"]);
  assert.equal(calls.length, 0);
  const negated = await classify("not signed yet, maybe Friday", { ...CLS, evaluate, named: "jev", shadow: true });
  assert.deepEqual(negated.flags, []);
  assert.deepEqual(negated.classifier?.flags, ["win"], "shadow shows what Jev would have fed; the engine got none");
});

const DOC = {
  line: "Stance: say it plainly; warm; short and complete; celebrate with the numbers.",
  mood: { valence: { level: "even" }, pressure: { level: "steady" } },
  stance: { caution: { level: "measured" }, hype: { level: "grounded" } },
  persona: { name: "Assistant" },
};

test("footer: valence, pressure and line words; inputs and moves only when present", () => {
  assert.equal(footerLine(32, DOC), "Assistant stance #32 · valence even · pressure steady · say it plainly");
  assert.equal(
    footerLine(33, DOC, { inputs: ["praise", "time 0.75"], moved: ["valence", "caution"] }),
    "Assistant stance #33 · valence even · pressure steady · say it plainly · in: praise, time 0.75 · moved: valence, caution",
  );
  assert.equal(footerLine(34, DOC, { inputs: [], moved: [] }), "Assistant stance #34 · valence even · pressure steady · say it plainly");
});

test("footer: never longer than 120 characters; long lists shorten to +k", () => {
  const long = footerLine(1234, DOC, {
    inputs: ["praise", "correction", "failure", "owner_upset", "security", "win", "wants_energy", "time 0.75"],
    moved: ["valence", "pressure", "caution", "directness", "hype", "humour", "verbosity", "warmth", "emoji"],
  });
  assert.ok([...long].length <= FOOTER_MAX, `${[...long].length} chars: ${long}`);
  assert.match(long, /\+\d/);
  assert.match(long, / · in: .+ · moved: /u, "both parts stay visible");
  const tight = footerLine(2, { ...DOC, line: "Stance: no jokes about a miss; own it plainly." }, {
    inputs: ["correction", "failure", "owner_upset", "time 0.75"],
    moved: ["valence", "caution", "directness", "hype", "humour"],
  });
  assert.ok([...tight].length <= FOOTER_MAX, tight);
  assert.match(tight, / · in: correction, failure, .*\+\d · moved: .*\+\d$/u);
});

test("moved: mood then trait names whose level changed; undefined without a previous event", () => {
  const next = {
    ...DOC,
    mood: { valence: { level: "up" }, pressure: { level: "steady" } },
    stance: { caution: { level: "careful" }, hype: { level: "grounded" } },
  };
  assert.deepEqual(movedNames(DOC, next), ["valence", "caution"]);
  assert.deepEqual(movedNames(DOC, DOC), []);
  assert.equal(movedNames(undefined, DOC), undefined);
});

test("config: classifier keys, shadow on by default, 800 ms budget for jev unless set", () => {
  const d = resolveConfig({}, "/home/x").config;
  assert.deepEqual([d.classifier, d.classifierShadow, d.classifierTimeoutMs, d.jevModel], ["auto", true, 300, "typesafe/jev-1.13"]);
  assert.equal(d.jevKeyFile, "/home/x/.openclaw/probbit-stance/openrouter.env");
  const j = resolveConfig({ classifier: "jev" }, "/home/x").config;
  assert.equal(j.classifierTimeoutMs, 800);
  assert.equal(resolveConfig({ classifier: "jev", classifierTimeoutMs: 400 }).config.classifierTimeoutMs, 400);
  const bad = resolveConfig({ classifier: "llm", classifierShadow: "yes" });
  assert.equal(bad.config.classifier, "auto");
  assert.equal(bad.config.classifierShadow, true);
  assert.equal(bad.warnings.length, 2);
});

const LANE = "team-room";

function lastLog(dir: string): { event: Record<string, unknown>; meta: Record<string, unknown> } {
  const lines = readFileSync(join(dir, "log", `${LANE}.jsonl`), "utf8").split("\n").filter((l) => l.trim());
  const cols = lines[lines.length - 1]!.split("\t");
  return { event: JSON.parse(cols[1]!), meta: JSON.parse(cols[3]!) };
}

test("hook, jev shadow (default): keyword flags reach probbit; Jev answers, latency and cost are logged; key not logged", async () => {
  const dir = tempStateDir();
  const { fetch, calls } = mockFetch({ probs: { owner_upset: 0.95 }, score: 4 });
  const host = createHost({ lanes: { [GROUP_KEY]: LANE }, stateDir: dir, binary: PROBBIT, classifier: "jev", jevKeyFile: keyFile(`OPENROUTER_API_KEY=${FAKE_KEY}\n`) });
  registerStance(host.api, { fetch });
  const { result } = await host.promptBuild(cliEvent("thanks, now fix it right now"), groupCtx("r1"));
  const block = (result as { appendContext?: string }).appendContext ?? "";
  assert.match(block, /Assistant \p{Extended_Pictographic} stance #1 · valence \w+ · pressure \w+ · .+ · in: praise"/u);
  const { event, meta } = lastLog(dir);
  assert.deepEqual(Object.keys(event).sort(), ["elapsed_hours", "praise"]);
  assert.equal(meta.method, "keyword");
  const c = meta.classifier as Record<string, unknown>;
  assert.deepEqual([c.name, c.mode, c.ok, c.timePressure], ["jev", "shadow", true, 1]);
  assert.deepEqual(c.flags, ["owner_upset"]);
  assert.equal(typeof c.ms, "number");
  assert.equal(c.cost, 0.00002);
  assert.equal(calls.length, 1);
  assert.equal(readFileSync(join(dir, "log", `${LANE}.jsonl`), "utf8").includes(FAKE_KEY), false);
  assert.equal(host.logs.some((l) => l.message.includes(FAKE_KEY)), false);
});

test("hook, jev live: Jev's flags and time_pressure reach probbit; the second event reports what moved", async () => {
  const dir = tempStateDir();
  const { fetch } = mockFetch({ probs: { owner_upset: 0.95, correction: 0.9 }, score: 3 });
  const host = createHost({
    lanes: { [GROUP_KEY]: LANE },
    stateDir: dir,
    binary: PROBBIT,
    classifier: "jev",
    classifierShadow: false,
    jevKeyFile: keyFile(`OPENROUTER_API_KEY=${FAKE_KEY}\n`),
  });
  registerStance(host.api, { fetch });
  await host.promptBuild(cliEvent("hello"), groupCtx("r1"));
  const { result } = await host.promptBuild(cliEvent("you broke the footer again, fix it now"), groupCtx("r2"));
  const block = (result as { appendContext?: string }).appendContext ?? "";
  const { event, meta } = lastLog(dir);
  assert.deepEqual(event, { correction: true, elapsed_hours: event.elapsed_hours, failure: true, owner_upset: true, time_pressure: 0.75 });
  assert.equal(meta.method, "jev");
  assert.equal(meta.timePressure, 0.75);
  assert.deepEqual(meta.inputs, ["correction", "failure", "owner_upset", "time 0.75"], "the log keeps the full input list");
  assert.match(block, / · in: correction, failure/u);
  assert.ok(Array.isArray(meta.moved), "the second event knows the first");
  if ((meta.moved as string[]).length > 0) assert.match(block, / · moved: /u);
  const foot = /End the visible reply with one line: "(.+)"\.$/u.exec(block)?.[1] ?? "";
  assert.ok([...foot].length <= 120, foot);
});

test("hook, jev without a key file: one warn, keywords classify, the turn still gets its stance", async () => {
  const dir = tempStateDir();
  const { fetch, calls } = mockFetch();
  const host = createHost({ lanes: { [GROUP_KEY]: LANE }, stateDir: dir, binary: PROBBIT, classifier: "jev", jevKeyFile: join(dir, "missing.env") });
  registerStance(host.api, { fetch });
  const { result } = await host.promptBuild(cliEvent("thanks"), groupCtx("r1"));
  assert.ok((result as { appendContext?: string }).appendContext);
  assert.equal(calls.length, 0);
  assert.equal(host.logs.filter((l) => l.level === "warn" && l.message.includes("no OPENROUTER_API_KEY")).length, 1);
  assert.equal(existsSync(join(dir, "log", `${LANE}.jsonl`)), true);
  assert.equal(lastLog(dir).meta.classifier, undefined);
});

test("hook, classifier keyword: no classifier call even with a key", async () => {
  const dir = tempStateDir();
  const { fetch, calls } = mockFetch();
  const host = createHost({ lanes: { [GROUP_KEY]: LANE }, stateDir: dir, binary: PROBBIT, classifier: "keyword", jevKeyFile: keyFile(`OPENROUTER_API_KEY=${FAKE_KEY}\n`) });
  registerStance(host.api, { fetch });
  await host.promptBuild(cliEvent("thanks"), groupCtx("r1"));
  assert.equal(calls.length, 0);
  assert.equal(lastLog(dir).meta.method, "keyword");
});
