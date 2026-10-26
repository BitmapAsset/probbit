import assert from "node:assert/strict";
import { chmodSync, existsSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { registerStance } from "../src/index.js";
import { checkBinaryVersion, MIN_PROBBIT, parseProbbitVersion, PLUGIN_VERSION, versionBelow } from "../src/version.js";
import { cliEvent, createHost, GROUP_KEY, groupCtx, PROBBIT, tempStateDir } from "./host.js";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const pkg = JSON.parse(readFileSync(join(ROOT, "package.json"), "utf8")) as { version: string };
const LANE = "team-room";

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

/** A stand-in that prints `probbit <version>` for `--version`, writes a state on `persona init`, logs its argv, fails `live`. */
function fakeOld(version: string): { bin: string; argv: () => string[] } {
  const dir = tempStateDir();
  const bin = join(dir, `probbit-${version}`);
  const log = join(dir, "argv.log");
  writeFileSync(
    bin,
    [
      "#!/bin/sh",
      `printf '%s\\n' "$*" >> '${log}'`,
      `if [ "$1" = --version ]; then echo 'probbit ${version}'; exit 0; fi`,
      'prev=""; out=""',
      'for a in "$@"; do [ "$prev" = --out ] && out="$a"; prev="$a"; done',
      `if [ "$1" = persona ]; then echo '{}' > "$out"; exit 0; fi`,
      "echo 'ran an event' >&2",
      "exit 9",
      "",
    ].join("\n"),
  );
  chmodSync(bin, 0o755);
  return { bin, argv: () => (existsSync(log) ? readFileSync(log, "utf8").split("\n").filter(Boolean) : []) };
}

test("version: the plugin's constant is package.json's; the requirement is probbit 0.13.0", () => {
  assert.equal(PLUGIN_VERSION, pkg.version);
  assert.equal(MIN_PROBBIT, "0.13.0");
});

test("version: parse `probbit --version` and order versions (a pre-release sits below its release)", () => {
  assert.equal(parseProbbitVersion("probbit 0.13.0\n"), "0.13.0");
  assert.equal(parseProbbitVersion("probbit 0.13.1-dev\n"), "0.13.1-dev");
  assert.equal(parseProbbitVersion('{"error":{"code":"event"}}\n'), undefined);
  assert.equal(parseProbbitVersion(""), undefined);
  assert.equal(versionBelow("0.12.1", "0.13.0"), true);
  assert.equal(versionBelow("0.11.0", "0.13.0"), true);
  assert.equal(versionBelow("0.13.0-dev", "0.13.0"), true);
  assert.equal(versionBelow("0.13.0", "0.13.0"), false);
  assert.equal(versionBelow("0.13.1-dev", "0.13.0"), false);
  assert.equal(versionBelow("0.14.0", "0.13.0"), false);
  assert.equal(versionBelow("1.0.0", "0.13.0"), false);
  assert.equal(versionBelow("0.9.10", "0.13.0"), true);
});

test("version: the real binary meets the requirement", async () => {
  const check = await checkBinaryVersion(PROBBIT, 10_000);
  assert.equal(check.ok, true, JSON.stringify(check));
  assert.ok(check.ok && check.version && !versionBelow(check.version, MIN_PROBBIT), JSON.stringify(check));
});

test("version: probbit 0.12.1 -> a load-time warning naming both versions, no stance, no event, nothing left behind, one probe", async () => {
  const fake = fakeOld("0.12.1");
  const dir = tempStateDir();
  const host = createHost({ lanes: { [GROUP_KEY]: LANE }, stateDir: dir, binary: fake.bin });
  registerStance(host.api);
  // The load-time probe is cached per binary: wait for that same promise, then for the warning chained on it.
  await checkBinaryVersion(fake.bin, 10_000);
  await sleep(10);
  const atLoad = host.logs.filter((l) => l.level === "warn");
  assert.equal(atLoad.length, 1, JSON.stringify(host.logs));
  assert.match(atLoad[0]!.message, new RegExp(`probbit-stance ${PLUGIN_VERSION.replace(/\./g, "\\.")} needs probbit 0\\.13\\.0 or later`));
  assert.match(atLoad[0]!.message, /is probbit 0\.12\.1/);

  const first = await host.promptBuild(cliEvent("thanks"), groupCtx("r1"));
  const second = await host.promptBuild(cliEvent("and again"), groupCtx("r2"));
  assert.equal(first.result, undefined);
  assert.equal(second.result, undefined);
  assert.equal(first.threw, false);
  const turns = host.logs.filter((l) => l.level === "warn").slice(1);
  assert.equal(turns.length, 2, JSON.stringify(host.logs));
  for (const t of turns) assert.match(t.message, /no stance this turn \(probbit-stance .* needs probbit 0\.13\.0 or later/);
  const argv = fake.argv();
  assert.equal(argv.filter((a) => a === "--version").length, 1, argv.join("\n"));
  assert.ok(argv.every((a) => !a.startsWith("live ") || a === "live --help"), `never asked to run an event: ${argv.join("; ")}`);
  for (const p of ["state", "strands", "log"]) {
    const f = join(dir, p, `${LANE}${p === "state" ? ".json" : p === "strands" ? ".strand" : ".jsonl"}`);
    assert.equal(existsSync(f), false, `${f} written`);
  }
});

test("version: a binary that prints no `probbit X.Y.Z` is not judged by the check (the event decides)", async () => {
  const dir0 = tempStateDir();
  const odd = join(dir0, "odd-probbit");
  writeFileSync(odd, `#!/bin/sh\nif [ "$1" = --version ]; then echo 'something else'; exit 0; fi\nexec ${PROBBIT} "$@"\n`);
  chmodSync(odd, 0o755);
  const check = await checkBinaryVersion(odd, 10_000);
  assert.deepEqual(check, { ok: true });
});
