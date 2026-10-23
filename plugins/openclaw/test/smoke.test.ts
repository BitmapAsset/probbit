import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { cpSync, mkdtempSync, readdirSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import plugin from "../src/index.js";
import { resolveConfig } from "../src/config.js";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const manifest = JSON.parse(readFileSync(join(ROOT, "openclaw.plugin.json"), "utf8"));
const pkg = JSON.parse(readFileSync(join(ROOT, "package.json"), "utf8"));
const entries = JSON.parse(readFileSync(join(ROOT, "install", "openclaw-entries.json"), "utf8"));

test("entry, manifest and package agree", () => {
  assert.equal(plugin.id, manifest.id);
  assert.equal(pkg.name, manifest.id);
  assert.equal(typeof plugin.register, "function");
  assert.deepEqual(plugin.configSchema.jsonSchema, manifest.configSchema);
  assert.deepEqual(pkg.openclaw.extensions, ["./dist/src/index.js"]);
});

test("the proposed plugins.entries block validates and turns the gate hook on", () => {
  const entry = entries.plugins.entries["probbit-stance"];
  assert.equal(entry.hooks.allowConversationAccess, true);
  assert.deepEqual(plugin.configSchema.safeParse(entry.config), { success: true, data: entry.config });
  const { config, warnings } = resolveConfig(entry.config, "/home/u");
  assert.deepEqual(warnings, []);
  assert.deepEqual(config.lanes, { "agent:main:telegram:group:-1001234567890": "team-room", "agent:main:telegram:direct:*": "dm" });
  assert.equal(config.stateDir, "/home/u/.openclaw/probbit-stance");
  assert.equal(plugin.configSchema.safeParse({ lanes: { x: "../x" } }).success, false);
});

test("the built entry has no runtime dependency on the openclaw package", () => {
  const dist = join(ROOT, "dist", "src");
  for (const f of readdirSync(dist).filter((n) => n.endsWith(".js"))) {
    const src = readFileSync(join(dist, f), "utf8");
    assert.ok(!/from\s+["']openclaw/.test(src) && !/import\(["']openclaw/.test(src), `${f} imports openclaw at runtime`);
  }
});

test("native import from a clean copy (no node_modules), as the host loads packaged JS", () => {
  const dir = mkdtempSync(join(tmpdir(), "probbit-stance-deploy-"));
  cpSync(join(ROOT, "dist", "src"), join(dir, "dist", "src"), { recursive: true });
  cpSync(join(ROOT, "package.json"), join(dir, "package.json"));
  cpSync(join(ROOT, "openclaw.plugin.json"), join(dir, "openclaw.plugin.json"));
  const out = execFileSync(
    process.execPath,
    ["--input-type=module", "-e", `const m = await import(${JSON.stringify(join(dir, "dist", "src", "index.js"))}); console.log(JSON.stringify({ id: m.default.id, register: typeof m.default.register }));`],
    { encoding: "utf8", cwd: dir },
  );
  assert.deepEqual(JSON.parse(out), { id: "probbit-stance", register: "function" });
});
