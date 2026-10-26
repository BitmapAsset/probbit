import { createHash, randomBytes } from "node:crypto";
import { existsSync, readFileSync, renameSync, unlinkSync, writeFileSync } from "node:fs";
import { basename, dirname, extname, join } from "node:path";
import { runBinary } from "./probbit.js";

/**
 * A strand's presentation labels (who wrote it, from which host and lane) live in a sidecar next to it, never in the
 * strand: `strands/<lane>.strand` -> `strands/<lane>.labels.json`, one flat JSON object of string -> string, keys
 * sorted, compact, one trailing newline. `probbit monitor` reads it; `probbit live --label k=v` merges keys into it.
 */
export const MAX_LABELS = 8;
/** Keys with a meaning: who (agent), harness + version (host), the lane, a short non-secret session label. */
export const RESERVED_LABELS = ["agent", "host", "lane", "session"];
/** Free keys `labels` may add beside the reserved ones, so a lane's labels always fit MAX_LABELS. */
export const MAX_FREE_LABELS = MAX_LABELS - RESERVED_LABELS.length;
const LABEL_KEY = /^[A-Za-z0-9_.-]{1,64}$/u;
const CONTROL = /[\u0000-\u001f\u007f-\u009f]/u;
const LONE_SURROGATE = /[\ud800-\udfff]/u;
export const LABEL_RULES =
  "keys are 1-64 characters of [A-Za-z0-9_.-], values 1-64 characters without control characters";

export type Labels = Record<string, string>;

/** Why a key/value pair breaks the label rules, or undefined when it is fine. */
export function labelProblem(key: string, value: unknown): string | undefined {
  if (!LABEL_KEY.test(key)) return "key";
  if (typeof value !== "string") return "value is not a string";
  const chars = [...value].length;
  if (chars < 1 || chars > 64) return "value length";
  if (CONTROL.test(value) || LONE_SURROGATE.test(value)) return "control character in value";
  return undefined;
}

/** The strand path with its last extension replaced by `.labels.json` (`a/b.strand` -> `a/b.labels.json`, `foo` -> `foo.labels.json`). */
export function sidecarPath(strandPath: string): string {
  const ext = extname(strandPath);
  return `${ext ? strandPath.slice(0, -ext.length) : strandPath}.labels.json`;
}

/** The sidecar's bytes: sorted keys, compact JSON, one trailing newline. */
export function renderLabels(labels: Labels): string {
  const sorted: Labels = {};
  for (const k of Object.keys(labels).sort()) sorted[k] = labels[k] as string;
  return `${JSON.stringify(sorted)}\n`;
}

/**
 * The `labels` config entry: valid pairs kept, the rest dropped. At most MAX_FREE_LABELS keys besides the reserved ones
 * (in key order); a reserved key here overrides the value the plugin computes.
 */
export function parseLabelsConfig(raw: unknown): { labels: Labels; dropped: string[] } {
  const labels: Labels = {};
  const dropped: string[] = [];
  if (raw === undefined) return { labels, dropped };
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) return { labels, dropped: ["(not an object)"] };
  let free = 0;
  for (const key of Object.keys(raw as Record<string, unknown>).sort()) {
    const value = (raw as Record<string, unknown>)[key];
    if (labelProblem(key, value) !== undefined) {
      dropped.push(key);
      continue;
    }
    if (!RESERVED_LABELS.includes(key)) {
      if (free >= MAX_FREE_LABELS) {
        dropped.push(key);
        continue;
      }
      free += 1;
    }
    labels[key] = value as string;
  }
  return { labels, dropped };
}

const SESSION_KINDS = new Set(["channel", "chat", "direct", "dm", "group", "room", "thread", "topic"]);
const CHANNEL = /^[a-z][a-z0-9-]{0,23}$/u;

/**
 * A short, non-secret label for a session: the channel and kind segments of `agent:<id>:<channel>:<kind>:...` plus the
 * first 8 hex of sha256(sessionKey), e.g. `telegram group #3fa9c2d1`; `#<8 hex>` for any other shape. Never the key itself,
 * never a chat or user id.
 */
export function sessionLabel(sessionKey: string): string {
  const hash = createHash("sha256").update(sessionKey).digest("hex").slice(0, 8);
  const parts = sessionKey.split(":");
  const channel = parts[2] ?? "";
  const kind = parts[3] ?? "";
  if (parts[0] === "agent" && parts.length >= 5 && CHANNEL.test(channel) && SESSION_KINDS.has(kind)) return `${channel} ${kind} #${hash}`;
  return `#${hash}`;
}

/** The agent id of an `agent:<agentId>:...` session key. */
export function agentIdOf(sessionKey: string): string | undefined {
  const parts = sessionKey.split(":");
  return parts[0] === "agent" && parts[1] ? parts[1] : undefined;
}

/** A lane's labels: host, lane, agent and session computed here, then the config's `labels` on top (the user's word wins). */
export function laneLabels(input: {
  lane: string;
  sessionKey: string;
  hostVersion?: string | undefined;
  agentName?: string | undefined;
  extra: Labels;
}): Labels {
  const version = input.hostVersion?.trim();
  const host = version ? `OpenClaw ${version}` : "OpenClaw";
  const computed: Labels = {
    host: labelProblem("host", host) === undefined ? host : "OpenClaw",
    lane: input.lane,
    session: sessionLabel(input.sessionKey),
  };
  const agent = input.agentName ?? agentIdOf(input.sessionKey);
  if (agent !== undefined && labelProblem("agent", agent) === undefined) computed.agent = agent;
  return { ...computed, ...input.extra };
}

/** `probbit live --label k=v` arguments, in key order. */
export function labelArgs(labels: Labels): string[] {
  return Object.keys(labels)
    .sort()
    .flatMap((k) => ["--label", `${k}=${labels[k] as string}`]);
}

function readSidecar(path: string): { raw: string; labels: Labels } | undefined {
  if (!existsSync(path)) return undefined;
  const raw = readFileSync(path, "utf8");
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    parsed = undefined;
  }
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed) || !Object.values(parsed).every((v) => typeof v === "string")) {
    throw new Error(`${basename(path)} is not a flat JSON object of strings; left as is`);
  }
  return { raw, labels: parsed as Labels };
}

/**
 * Makes the strand's sidecar hold `labels` merged over its current keys (keys not named stay). Writes only when the file is
 * missing or its bytes differ from the merge, atomically (temp file in the same directory + rename). Never touches the
 * strand. Throws when the current file is not a label object or the merge would exceed MAX_LABELS keys.
 */
export function syncSidecar(strandPath: string, labels: Labels): "written" | "unchanged" {
  const path = sidecarPath(strandPath);
  const current = readSidecar(path);
  const merged = { ...(current?.labels ?? {}), ...labels };
  if (Object.keys(merged).length > MAX_LABELS) {
    throw new Error(`${basename(path)} would hold ${Object.keys(merged).length} keys (at most ${MAX_LABELS}); left as is`);
  }
  const want = renderLabels(merged);
  if (current?.raw === want) return "unchanged";
  const tmp = join(dirname(path), `.${basename(path)}.${process.pid}.${randomBytes(4).toString("hex")}.tmp`);
  try {
    writeFileSync(tmp, want);
    renameSync(tmp, path);
  } catch (err) {
    try {
      unlinkSync(tmp);
    } catch {
      // nothing to clean up
    }
    throw err;
  }
  return "written";
}

const labelProbes = new Map<string, Promise<boolean>>();

/** Whether `probbit live` takes `--label` (its `live --help` mentions it); asked once per binary per process. */
export function liveTakesLabels(binary: string, killAfterMs: number): Promise<boolean> {
  let probe = labelProbes.get(binary);
  if (!probe) {
    probe = runBinary(binary, ["live", "--help"], "", killAfterMs).then((r) => !r.spawnError && `${r.stdout}\n${r.stderr}`.includes("--label"));
    labelProbes.set(binary, probe);
  }
  return probe;
}
