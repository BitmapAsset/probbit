import { homedir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

/** The persona shipped with the plugin (`personas/assistant.yaml`, next to `dist/`): the default when `personaPath` is unset. */
export function bundledPersonaPath(): string {
  return fileURLToPath(new URL("../../personas/assistant.yaml", import.meta.url));
}

export type GateMode = "shadow" | "off";
export type InjectTarget = "appendContext" | "appendSystemContext";
/** auto: the host decision role when an agent decisionModel is configured, else keywords. jev: the Jev adapter. keyword: keywords only. */
export type ClassifierMode = "auto" | "jev" | "keyword";

/** One entry of the destructive-action list the shadow gate checks. */
export type DestructiveRule = {
  id: string;
  /** Canonical OpenClaw tool ids the rule applies to (Claude Code's Bash arrives as `exec`). */
  tools: string[];
  /** Regex over a string param (`param`, default `command`). */
  regex?: string;
  param?: string;
  /** Path prefix for file-writing tools; `~` expands to the home directory. */
  pathPrefix?: string;
};

export type StanceConfig = {
  /** Exact session key, or a prefix ending in `*`, mapped to a lane name. Unlisted sessions are untouched. */
  lanes: Record<string, string>;
  personaPath: string;
  stateDir: string;
  binary: string;
  seed: number;
  timeoutMs: number;
  gateMode: GateMode;
  footer: boolean;
  /** Opens the footer line (default: the persona's name as the stance document reports it). */
  footerPrefix?: string;
  inject: InjectTarget;
  classifierTimeoutMs: number;
  decisionThreshold: number;
  classifier: ClassifierMode;
  /** classifier "jev" only: log Jev's answers beside the keyword result but feed the engine the keyword flags. */
  classifierShadow: boolean;
  /** Pinned Jev model id. */
  jevModel: string;
  /** Env-style file holding OPENROUTER_API_KEY; read once at plugin start. */
  jevKeyFile: string;
  destructive: DestructiveRule[];
  /** When non-empty, only turns whose ctx.senderId is listed produce events (group members other than the operator are skipped). */
  senderIds: string[];
};

export const SHELL_TOOLS = ["exec", "bash", "shell"];
export const WRITE_TOOLS = ["write", "edit", "multi_edit", "apply_patch", "notebook_edit"];

export const DEFAULT_DESTRUCTIVE: DestructiveRule[] = [
  {
    id: "rm-rf",
    tools: SHELL_TOOLS,
    regex:
      "(^|[\\s;&|(])(sudo\\s+)?rm(?=[^;&|\\n]*\\s-[a-zA-Z]*[rR]|[^;&|\\n]*\\s--recursive)(?=[^;&|\\n]*\\s-[a-zA-Z]*f|[^;&|\\n]*\\s--force)\\s",
  },
  {
    id: "git-push-force",
    tools: SHELL_TOOLS,
    regex: "\\bgit\\b[^;&|\\n]*\\bpush\\b[^;&|\\n]*\\s(-f|--force|--force-with-lease)(?=\\s|=|$)",
  },
  {
    id: "curl-pipe-shell",
    tools: SHELL_TOOLS,
    regex: "\\b(curl|wget)\\b[^;&\\n]*\\|\\s*(sudo\\s+)?(ba|z|da|k)?sh\\b|\\b(ba|z)?sh\\s+-c\\s+[\"']?\\$\\((curl|wget)\\b",
  },
  {
    id: "openclaw-config-set",
    tools: SHELL_TOOLS,
    regex: "\\bopenclaw\\s+config\\s+(set|unset)\\b",
  },
  {
    id: "shell-write-openclaw-json",
    tools: SHELL_TOOLS,
    regex: "(>|\\btee\\b|\\bsed\\s+-i\\b|\\bmv\\b|\\bcp\\b)[^;&|\\n]*\\.openclaw/openclaw\\.json",
  },
  { id: "write-openclaw-json", tools: WRITE_TOOLS, pathPrefix: "~/.openclaw/openclaw.json" },
];

const LANE_NAME = /^[a-z0-9][a-z0-9._-]{0,63}$/;

export function expandHome(p: string, home = homedir()): string {
  return p === "~" ? home : p.startsWith("~/") ? join(home, p.slice(2)) : p;
}

export type ResolvedConfig = { config: StanceConfig; warnings: string[] };

/** Resolves `plugins.entries.probbit-stance.config` with defaults. Never throws; bad fields fall back and warn. */
export function resolveConfig(raw: unknown, home = homedir()): ResolvedConfig {
  const warnings: string[] = [];
  const src = raw && typeof raw === "object" && !Array.isArray(raw) ? (raw as Record<string, unknown>) : {};
  const str = (key: string, fallback: string): string => {
    const v = src[key];
    if (v === undefined) return fallback;
    if (typeof v === "string" && v.trim()) return v.trim();
    warnings.push(`${key}: expected a non-empty string`);
    return fallback;
  };
  const num = (key: string, fallback: number, min: number, max: number): number => {
    const v = src[key];
    if (v === undefined) return fallback;
    if (typeof v === "number" && Number.isFinite(v) && v >= min && v <= max) return v;
    warnings.push(`${key}: expected a number in [${min}, ${max}]`);
    return fallback;
  };

  const lanes: Record<string, string> = {};
  const rawLanes = src.lanes;
  if (rawLanes && typeof rawLanes === "object" && !Array.isArray(rawLanes)) {
    for (const [key, lane] of Object.entries(rawLanes as Record<string, unknown>)) {
      if (typeof lane === "string" && LANE_NAME.test(lane) && key.length > 0) lanes[key] = lane;
      else warnings.push(`lanes.${key}: lane must match ${LANE_NAME.source}`);
    }
  } else if (rawLanes !== undefined) warnings.push("lanes: expected an object");

  const stateDir = expandHome(str("stateDir", "~/.openclaw/probbit-stance"), home);
  let footerPrefix: string | undefined;
  if (typeof src.footerPrefix === "string" && src.footerPrefix.trim() && [...src.footerPrefix.trim()].length <= 24) footerPrefix = src.footerPrefix.trim();
  else if (src.footerPrefix !== undefined) warnings.push("footerPrefix: expected a non-empty string of at most 24 characters");
  const gateMode = src.gateMode === "off" ? "off" : "shadow";
  if (src.gateMode !== undefined && src.gateMode !== "off" && src.gateMode !== "shadow") warnings.push("gateMode: expected shadow or off");
  const inject = src.inject === "appendSystemContext" ? "appendSystemContext" : "appendContext";
  if (src.inject !== undefined && src.inject !== "appendContext" && src.inject !== "appendSystemContext") {
    warnings.push("inject: expected appendContext or appendSystemContext");
  }
  let footer = true;
  if (typeof src.footer === "boolean") footer = src.footer;
  else if (src.footer !== undefined) warnings.push("footer: expected a boolean");

  let destructive = DEFAULT_DESTRUCTIVE;
  if (Array.isArray(src.destructive)) {
    destructive = [];
    for (const [i, rule] of (src.destructive as unknown[]).entries()) {
      const r = rule && typeof rule === "object" ? (rule as Record<string, unknown>) : {};
      const tools = Array.isArray(r.tools) ? r.tools.filter((t): t is string => typeof t === "string") : [];
      const ok =
        typeof r.id === "string" &&
        tools.length > 0 &&
        (typeof r.regex === "string" || typeof r.pathPrefix === "string") &&
        (typeof r.regex !== "string" || isValidRegex(r.regex));
      if (!ok) {
        warnings.push(`destructive[${i}]: needs id, tools and a valid regex or pathPrefix`);
        continue;
      }
      destructive.push({
        id: r.id as string,
        tools,
        ...(typeof r.regex === "string" ? { regex: r.regex } : {}),
        ...(typeof r.param === "string" ? { param: r.param } : {}),
        ...(typeof r.pathPrefix === "string" ? { pathPrefix: r.pathPrefix } : {}),
      });
    }
  } else if (src.destructive !== undefined) warnings.push("destructive: expected an array");

  let classifier: ClassifierMode = "auto";
  if (src.classifier === "auto" || src.classifier === "jev" || src.classifier === "keyword") classifier = src.classifier;
  else if (src.classifier !== undefined) warnings.push("classifier: expected auto, jev or keyword");
  let classifierShadow = true;
  if (typeof src.classifierShadow === "boolean") classifierShadow = src.classifierShadow;
  else if (src.classifierShadow !== undefined) warnings.push("classifierShadow: expected a boolean");
  const jevModel = str("jevModel", "typesafe/jev-1.13");

  let senderIds: string[] = [];
  if (Array.isArray(src.senderIds) && src.senderIds.every((v) => typeof v === "string" && v.length > 0)) senderIds = src.senderIds as string[];
  else if (src.senderIds !== undefined) warnings.push("senderIds: expected an array of non-empty strings");

  return {
    config: {
      lanes,
      personaPath: expandHome(str("personaPath", bundledPersonaPath()), home),
      stateDir,
      binary: expandHome(str("binary", "probbit"), home),
      seed: num("seed", 42, 0, Number.MAX_SAFE_INTEGER),
      timeoutMs: num("timeoutMs", 500, 50, 10_000),
      gateMode,
      footer,
      ...(footerPrefix !== undefined ? { footerPrefix } : {}),
      inject,
      classifierTimeoutMs: num("classifierTimeoutMs", classifier === "jev" ? 800 : 300, 50, 5_000),
      decisionThreshold: num("decisionThreshold", 0.6, 0, 1),
      classifier,
      classifierShadow,
      jevModel,
      jevKeyFile: expandHome(str("jevKeyFile", "~/.openclaw/probbit-stance/openrouter.env"), home),
      destructive,
      senderIds,
    },
    warnings,
  };
}

function isValidRegex(source: string): boolean {
  try {
    new RegExp(source);
    return true;
  } catch {
    return false;
  }
}

/** Exact key first, then the longest `prefix*` entry. */
export function resolveLane(lanes: Record<string, string>, sessionKey: string | undefined): string | undefined {
  if (!sessionKey) return undefined;
  const exact = lanes[sessionKey];
  if (exact) return exact;
  let best: { len: number; lane: string } | undefined;
  for (const [key, lane] of Object.entries(lanes)) {
    if (!key.endsWith("*")) continue;
    const prefix = key.slice(0, -1);
    if (sessionKey.startsWith(prefix) && (!best || prefix.length > best.len)) best = { len: prefix.length, lane };
  }
  return best?.lane;
}

/** JSON Schema mirrored in openclaw.plugin.json (the manifest copy is what the host validates before load). */
export const CONFIG_JSON_SCHEMA = {
  type: "object",
  additionalProperties: false,
  properties: {
    lanes: {
      type: "object",
      description: "Session key (exact) or prefix ending in * → lane name. Only listed sessions get the hooks.",
      additionalProperties: { type: "string", pattern: "^[a-z0-9][a-z0-9._-]{0,63}$" },
    },
    personaPath: { type: "string" },
    stateDir: { type: "string" },
    binary: { type: "string" },
    seed: { type: "integer", minimum: 0 },
    timeoutMs: { type: "integer", minimum: 50, maximum: 10000 },
    gateMode: { type: "string", enum: ["shadow", "off"] },
    footer: { type: "boolean" },
    footerPrefix: {
      type: "string",
      minLength: 1,
      maxLength: 24,
      description: "Opens the footer line (default: the persona's name). An emoji or a short handle works.",
    },
    inject: { type: "string", enum: ["appendContext", "appendSystemContext"] },
    classifierTimeoutMs: { type: "integer", minimum: 50, maximum: 5000 },
    decisionThreshold: { type: "number", minimum: 0, maximum: 1 },
    classifier: {
      type: "string",
      enum: ["auto", "jev", "keyword"],
      description:
        "auto: host decision role when an agent decisionModel is configured, else keywords. jev: the Jev adapter (OpenRouter). keyword: keywords only.",
    },
    classifierShadow: {
      type: "boolean",
      description: "classifier jev only (default true): log Jev's answers, feed the engine the keyword flags.",
    },
    jevModel: { type: "string", minLength: 1, description: "Pinned Jev model id (default typesafe/jev-1.13)." },
    jevKeyFile: {
      type: "string",
      minLength: 1,
      description: "Env-style file with OPENROUTER_API_KEY (default ~/.openclaw/probbit-stance/openrouter.env); read once at start.",
    },
    senderIds: { type: "array", items: { type: "string", minLength: 1 } },
    destructive: {
      type: "array",
      items: {
        type: "object",
        additionalProperties: false,
        required: ["id", "tools"],
        properties: {
          id: { type: "string" },
          tools: { type: "array", items: { type: "string" }, minItems: 1 },
          regex: { type: "string" },
          param: { type: "string" },
          pathPrefix: { type: "string" },
        },
      },
    },
  },
} as const;
