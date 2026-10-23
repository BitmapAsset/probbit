import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { isAbsolute, resolve } from "node:path";
import { expandHome, type DestructiveRule } from "./config.js";
import type { StanceDoc } from "./probbit.js";

export type DestructiveHit = { rule: string; detail: string; detailKind: "command" | "path" };

function lowerTool(name: string): string {
  return name.trim().toLowerCase();
}

function candidatePaths(params: Record<string, unknown>, derivedPaths: readonly string[] | undefined): string[] {
  const out: string[] = [];
  for (const key of ["path", "file_path", "notebook_path"]) {
    const v = params[key];
    if (typeof v === "string" && v.trim()) out.push(v.trim());
  }
  for (const p of derivedPaths ?? []) if (typeof p === "string" && p.trim()) out.push(p.trim());
  return out;
}

/** R47 class `destructive`, detected from the operator's list (shell regexes, protected path prefixes). */
export function detectDestructive(
  toolName: string,
  params: Record<string, unknown>,
  derivedPaths: readonly string[] | undefined,
  rules: DestructiveRule[],
  opts: { home: string; cwd?: string },
): DestructiveHit | undefined {
  const tool = lowerTool(toolName);
  for (const rule of rules) {
    if (!rule.tools.some((t) => lowerTool(t) === tool)) continue;
    if (rule.regex) {
      const value = params[rule.param ?? "command"];
      if (typeof value !== "string") continue;
      let re: RegExp;
      try {
        re = new RegExp(rule.regex, "i");
      } catch {
        continue;
      }
      if (re.test(value)) return { rule: rule.id, detail: value, detailKind: "command" };
    }
    if (rule.pathPrefix) {
      const prefix = resolve(expandHome(rule.pathPrefix, opts.home));
      for (const raw of candidatePaths(params, derivedPaths)) {
        const expanded = expandHome(raw, opts.home);
        if (!isAbsolute(expanded) && !opts.cwd) continue;
        const abs = isAbsolute(expanded) ? resolve(expanded) : resolve(opts.cwd ?? "/", expanded);
        if (abs.startsWith(prefix)) return { rule: rule.id, detail: abs, detailKind: "path" };
      }
    }
  }
  return undefined;
}

export type CarefulCheck = { careful: boolean; caution?: string; securityCareful: boolean; source: "memory" | "state" | "none"; n?: number };

/** Gate condition (R47 `destructive.deny_when: {caution: [careful]}`, plus the brief's habit clause). */
export function carefulFromDoc(doc: StanceDoc, n?: number): CarefulCheck {
  const caution = doc.stance?.caution?.level;
  const securityCareful = Array.isArray(doc.habits?.active) && doc.habits.active.includes("security_careful");
  return { careful: caution === "careful" || securityCareful, ...(caution ? { caution } : {}), securityCareful, source: "memory", ...(n !== undefined ? { n } : {}) };
}

/** After a restart: the state file's `prev` holds the levels of the lane's last stance. */
export function carefulFromState(statePath: string): CarefulCheck {
  try {
    const state = JSON.parse(readFileSync(statePath, "utf8")) as { prev?: { caution?: unknown }; turn?: unknown };
    const caution = typeof state.prev?.caution === "string" ? state.prev.caution : undefined;
    return {
      careful: caution === "careful",
      ...(caution ? { caution } : {}),
      securityCareful: false,
      source: "state",
      ...(typeof state.turn === "number" ? { n: state.turn } : {}),
    };
  } catch {
    return { careful: false, securityCareful: false, source: "none" };
  }
}

const SECRETISH = /\b(token|key|secret|password|passwd|bearer|authorization|auth)\b\s*[:=]?\s*\S+/gi;
const LONG_OPAQUE = /[A-Za-z0-9+/_=-]{24,}/g;

/** A reviewable but redacted preview; the full detail is only kept as a digest (R47 §4: details can hold secrets). */
export function redactPreview(detail: string): string {
  return detail.replace(SECRETISH, "$1 [redacted]").replace(LONG_OPAQUE, "[redacted]").replace(/\s+/g, " ").slice(0, 160);
}

export function digest(text: string): string {
  return `sha256:${createHash("sha256").update(text).digest("hex")}`;
}
