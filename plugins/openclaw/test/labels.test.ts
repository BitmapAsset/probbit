import assert from "node:assert/strict";
import { chmodSync, existsSync, readFileSync, statSync, utimesSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import plugin, { registerStance } from "../src/index.js";
import { resolveConfig } from "../src/config.js";
import { labelArgs, laneLabels, parseLabelsConfig, sessionLabel, sidecarPath, syncSidecar } from "../src/labels.js";
import { cliEvent, createHost, GROUP_KEY, groupCtx, tempStateDir } from "./host.js";

const LANE = "team-room";
const DIRECT_KEY = "agent:main:telegram:direct:5550001234";

test("labels config: valid pairs kept, no warning, no schema failure", () => {
  const r = resolveConfig({ labels: { team: "support", "build.id": "b-7", host: "OpenClaw on the night node" }, agentName: "Assistant" });
  assert.deepEqual(r.config.labels, { "build.id": "b-7", host: "OpenClaw on the night node", team: "support" });
  assert.equal(r.config.agentName, "Assistant");
  assert.deepEqual(r.warnings, []);
  assert.deepEqual(r.notices, []);
  assert.deepEqual(resolveConfig({}).config.labels, {});
});

test("labels config: an invalid key or value is dropped with one warning; the config still validates", () => {
  const raw = { labels: { "bad key": "x", ok: "yes", empty: "", nl: "a\nb", tab: "a\tb", long: "x".repeat(65), num: 3, wide: "é".repeat(64) } };
  const r = resolveConfig(raw);
  assert.deepEqual(r.config.labels, { ok: "yes", wide: "é".repeat(64) }, "64 characters, not bytes");
  assert.deepEqual(r.warnings, []);
  assert.equal(r.notices.length, 1);
  assert.match(r.notices[0]!, /^labels: dropped bad key, empty, long, nl, num, tab \(/);
  assert.equal(plugin.configSchema.safeParse(raw).success, true, "dropped labels are not fatal");
  const host = createHost({ lanes: { [GROUP_KEY]: LANE }, stateDir: tempStateDir(), ...raw });
  registerStance(host.api);
  assert.equal(host.logs.filter((l) => l.level === "warn" && /labels: dropped/.test(l.message)).length, 1);
});

test("labels config: at most 4 free keys beside the reserved ones; reserved overrides do not count", () => {
  const { labels, dropped } = parseLabelsConfig({ a: "1", b: "2", c: "3", d: "4", e: "5", f: "6", host: "h", lane: "l", agent: "x", session: "s" });
  assert.deepEqual(Object.keys(labels).sort(), ["a", "agent", "b", "c", "d", "host", "lane", "session"]);
  assert.deepEqual(dropped, ["e", "f"]);
  assert.deepEqual(parseLabelsConfig(["x"]).dropped, ["(not an object)"]);
  assert.equal(resolveConfig({ agentName: "a\nb" }).warnings.length, 1);
  assert.equal(resolveConfig({ agentName: "" }).warnings.length, 1);
});

test("session label: channel, kind and 8 hex of the key's sha256; never the key or the numeric id", () => {
  const g = sessionLabel(GROUP_KEY);
  assert.match(g, /^telegram group #[0-9a-f]{8}$/);
  assert.equal(sessionLabel(GROUP_KEY), g, "stable");
  const d = sessionLabel(DIRECT_KEY);
  assert.match(d, /^telegram direct #[0-9a-f]{8}$/);
  for (const [key, label] of [
    [GROUP_KEY, g],
    [DIRECT_KEY, d],
  ] as const) {
    assert.ok(!label.includes(key));
    const id = key.split(":").pop()!;
    assert.ok(!label.includes(id) && !label.includes(id.replace(/^-/, "")), `${label} carries ${id}`);
  }
  assert.match(sessionLabel("agent:main:main"), /^#[0-9a-f]{8}$/);
  assert.match(sessionLabel("agent:main:slack:C0123ABC:thread"), /^#[0-9a-f]{8}$/, "an id in the kind slot is not shown");
  assert.match(sessionLabel("x"), /^#[0-9a-f]{8}$/);
});

test("lane labels: computed host, lane, agent, session; the config's labels override them", () => {
  const base = laneLabels({ lane: LANE, sessionKey: GROUP_KEY, hostVersion: "2026.9.7", extra: {} });
  assert.deepEqual(base, { agent: "main", host: "OpenClaw 2026.9.7", lane: LANE, session: sessionLabel(GROUP_KEY) });
  assert.equal(laneLabels({ lane: LANE, sessionKey: GROUP_KEY, extra: {} }).host, "OpenClaw");
  assert.equal(laneLabels({ lane: LANE, sessionKey: GROUP_KEY, agentName: "Assistant", extra: {} }).agent, "Assistant");
  assert.equal(laneLabels({ lane: LANE, sessionKey: "main", extra: {} }).agent, undefined);
  const over = laneLabels({ lane: LANE, sessionKey: GROUP_KEY, hostVersion: "2026.9.7", extra: { host: "gateway A", session: "night shift", team: "ops" } });
  assert.deepEqual(over, { agent: "main", host: "gateway A", lane: LANE, session: "night shift", team: "ops" });
  assert.deepEqual(labelArgs({ lane: "x", agent: "a b" }), ["--label", "agent=a b", "--label", "lane=x"]);
});

test("sidecar path: the last extension replaced by .labels.json", () => {
  assert.equal(sidecarPath("strands/support.strand"), "strands/support.labels.json");
  assert.equal(sidecarPath("foo"), "foo.labels.json");
  assert.equal(sidecarPath("a.b/life.v2.strand"), "a.b/life.v2.labels.json");
  assert.equal(sidecarPath("dir.x/foo"), "dir.x/foo.labels.json");
});

test("sidecar for an existing strand: sorted compact JSON + newline, merge keeps a hand-added key, no rewrite when equal, strand untouched", () => {
  const dir = tempStateDir();
  const strand = join(dir, "lane.strand");
  const strandBytes = '{"probbit_strand":1}\n{"n":1}\n';
  writeFileSync(strand, strandBytes);
  const side = join(dir, "lane.labels.json");
  assert.equal(syncSidecar(strand, { lane: "lane", host: "OpenClaw 2026.9.7", agent: "main", session: "#0011aabb" }), "written");
  assert.equal(readFileSync(side, "utf8"), '{"agent":"main","host":"OpenClaw 2026.9.7","lane":"lane","session":"#0011aabb"}\n');
  // a hand-added key stays; a computed key is put back
  writeFileSync(side, '{"note":"día 1","host":"edited","lane":"lane","agent":"main","session":"#0011aabb"}');
  assert.equal(syncSidecar(strand, { lane: "lane", host: "OpenClaw 2026.9.7", agent: "main", session: "#0011aabb" }), "written");
  const merged = readFileSync(side, "utf8");
  assert.equal(merged, '{"agent":"main","host":"OpenClaw 2026.9.7","lane":"lane","note":"día 1","session":"#0011aabb"}\n');
  // equal content: not rewritten (mtime unchanged)
  const past = new Date(Date.now() - 3_600_000);
  utimesSync(side, past, past);
  const before = statSync(side).mtimeMs;
  assert.equal(syncSidecar(strand, { lane: "lane", host: "OpenClaw 2026.9.7" }), "unchanged");
  assert.equal(statSync(side).mtimeMs, before);
  assert.equal(readFileSync(strand, "utf8"), strandBytes, "the strand is never touched");
  // a broken or overfull sidecar is left as is
  writeFileSync(side, "[1]");
  assert.throws(() => syncSidecar(strand, { lane: "lane" }), /not a flat JSON object/);
  writeFileSync(side, JSON.stringify(Object.fromEntries(["a", "b", "c", "d", "e", "f", "g", "h"].map((k) => [k, k]))));
  assert.throws(() => syncSidecar(strand, { lane: "lane" }), /at most 8/);
  assert.equal(readFileSync(strand, "utf8"), strandBytes);
});

/**
 * A stand-in for the probbit binary: logs its argv, answers `live --help` (with or without `--label`), writes a state on
 * `persona init`, and on `live` appends a header (new strand) and one event line, then prints a stance document.
 */
function fakeBinary(takesLabels: boolean): { bin: string; argv: () => string[] } {
  const dir = tempStateDir();
  const bin = join(dir, takesLabels ? "probbit-labels" : "probbit-old");
  const log = join(dir, "argv.log");
  const help = takesLabels ? "usage: probbit live PERSONA [--strand FILE [--label KEY=VALUE]...]" : "usage: probbit live PERSONA [--strand FILE]";
  const doc = JSON.stringify({
    line: "Stance: say it plainly; one line.",
    status: "ok",
    mood: { valence: { level: "even" }, pressure: { level: "steady" } },
    habits: { active: [], bound: [] },
    persona: { name: "Assistant", version: "1.0.0" },
  });
  writeFileSync(
    bin,
    [
      "#!/bin/sh",
      `printf '%s\\n' "$*" >> '${log}'`,
      `if [ "$1" = live ] && [ "$2" = --help ]; then echo '${help}'; exit 0; fi`,
      'prev=""; out=""; strand=""',
      'for a in "$@"; do [ "$prev" = --out ] && out="$a"; [ "$prev" = --strand ] && strand="$a"; prev="$a"; done',
      `if [ "$1" = persona ]; then echo '{}' > "$out"; exit 0; fi`,
      "cat > /dev/null",
      `[ -f "$strand" ] || echo '{"probbit_strand":1}' > "$strand"`,
      'n=$(wc -l < "$strand" | tr -d " ")',
      `echo "{\\"n\\":$n}" >> "$strand"`,
      `echo '${doc}'`,
      "",
    ].join("\n"),
  );
  chmodSync(bin, 0o755);
  return { bin, argv: () => (existsSync(log) ? readFileSync(log, "utf8").split("\n").filter(Boolean) : []) };
}

function setupFake(takesLabels: boolean, overrides: Record<string, unknown> = {}) {
  const fake = fakeBinary(takesLabels);
  const dir = tempStateDir();
  const host = createHost(
    { lanes: { [GROUP_KEY]: LANE, "agent:main:telegram:direct:*": "dm" }, stateDir: dir, binary: fake.bin, ...overrides },
    { runtime: { version: "2026.9.7" } },
  );
  registerStance(host.api);
  return { dir, host, fake };
}

test("hook, a binary without --label: no --label argv; the plugin writes the sidecar; one probe per process", async () => {
  const { dir, host, fake } = setupFake(false, { labels: { team: "ops" } });
  await host.promptBuild(cliEvent("thanks"), groupCtx("r1"));
  await host.promptBuild(cliEvent("thanks"), groupCtx("r2", { sessionKey: DIRECT_KEY }));
  await host.promptBuild(cliEvent("ok"), groupCtx("r3"));
  const argv = fake.argv();
  assert.equal(argv.filter((a) => a === "live --help").length, 1, argv.join("\n"));
  assert.ok(argv.every((a) => !a.includes("--label")), argv.join("\n"));
  const strand = join(dir, "strands", `${LANE}.strand`);
  assert.equal(
    readFileSync(join(dir, "strands", `${LANE}.labels.json`), "utf8"),
    `${JSON.stringify({ agent: "main", host: "OpenClaw 2026.9.7", lane: LANE, session: sessionLabel(GROUP_KEY), team: "ops" })}\n`,
  );
  assert.equal(readFileSync(strand, "utf8"), '{"probbit_strand":1}\n{"n":1}\n{"n":2}\n', "only the binary wrote the strand");
  const dm = JSON.parse(readFileSync(join(dir, "strands", "dm.labels.json"), "utf8"));
  assert.equal(dm.session, sessionLabel(DIRECT_KEY));
  assert.ok(!readFileSync(join(dir, "strands", "dm.labels.json"), "utf8").includes("5550001234"));
  assert.equal(host.logs.filter((l) => l.level === "warn").length, 0, JSON.stringify(host.logs));
});

test("hook, a binary with --label: the labels go to the creating `probbit live` only; later events sync the sidecar", async () => {
  const { dir, host, fake } = setupFake(true, { agentName: "Assistant", labels: { host: "gateway A" } });
  await host.promptBuild(cliEvent("thanks"), groupCtx("r1"));
  const lives = () => fake.argv().filter((a) => a.startsWith("live ") && a !== "live --help");
  assert.equal(lives().length, 1);
  const first = lives()[0]!;
  assert.ok(
    first.endsWith(`--clock fixed --label agent=Assistant --label host=gateway A --label lane=${LANE} --label session=${sessionLabel(GROUP_KEY)}`),
    first,
  );
  const side = join(dir, "strands", `${LANE}.labels.json`);
  assert.equal(existsSync(side), false, "the binary was given the labels; the plugin did not write them too");
  await host.promptBuild(cliEvent("ok"), groupCtx("r2"));
  assert.equal(lives().length, 2);
  assert.ok(!lives()[1]!.includes("--label"), "an existing strand gets no --label");
  assert.deepEqual(JSON.parse(readFileSync(side, "utf8")), { agent: "Assistant", host: "gateway A", lane: LANE, session: sessionLabel(GROUP_KEY) });
});
