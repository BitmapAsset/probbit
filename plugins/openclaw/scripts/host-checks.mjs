// Read-only checks against the installed OpenClaw 2026.9.7 internals (hashed chunk names: version-specific).
// No install, no config write, no gateway contact. Run after `npm run build`.
import { cpSync, mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const DIST = "/opt/homebrew/lib/node_modules/openclaw/dist";
const version = JSON.parse(readFileSync(join(DIST, "..", "package.json"), "utf8")).version;
const deploy = mkdtempSync(join(tmpdir(), "probbit-stance-hostcheck-"));
for (const p of ["openclaw.plugin.json", "package.json"]) cpSync(join(ROOT, p), join(deploy, p));
cpSync(join(ROOT, "dist", "src"), join(deploy, "dist", "src"), { recursive: true });

const manifestMod = await import(join(DIST, "manifest-BVZU0TgX.mjs"));
const validator = await import(join(DIST, "schema-validator-CbLrj3Mc.mjs"));
const loaded = manifestMod.r(deploy);
const schema = JSON.parse(readFileSync(join(ROOT, "openclaw.plugin.json"), "utf8")).configSchema;
const cfg = JSON.parse(readFileSync(join(ROOT, "install", "openclaw-entries.json"), "utf8")).plugins.entries["probbit-stance"].config;
const good = validator.r({ schema, value: cfg, cacheKey: "probbit-stance:good" });
const bad = validator.r({ schema, value: { ...cfg, gateMode: "enforce", sensesMode: "live", lanes: { x: "../x" } }, cacheKey: "probbit-stance:bad" });
const entry = await import(join(deploy, "dist", "src", "index.js"));
console.log(JSON.stringify({
  openclaw: version,
  manifest: { ok: loaded.ok, id: loaded.manifest?.id, activation: loaded.manifest?.activation, error: loaded.ok ? undefined : loaded.error },
  proposedConfig: { ok: good.ok },
  badConfig: { ok: bad.ok, errors: (bad.errors ?? []).map((e) => e.text) },
  entry: { id: entry.default.id, register: typeof entry.default.register },
}, null, 1));
