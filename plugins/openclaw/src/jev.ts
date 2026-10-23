import { readFileSync } from "node:fs";
import type { DecisionAnswer, DecisionEvaluate, DecisionOutcome } from "./classify.js";

/** TypeSafe System One on OpenRouter. The reference client is the workspace's scripts/jev.py. */
export const JEV_URL = "https://openrouter.ai/api/v1/systemone";
/** Pinned; "typesafe/jev-1.13.0" answers 400 on OpenRouter. */
export const JEV_DEFAULT_MODEL = "typesafe/jev-1.13";

export type FetchLike = (
  url: string,
  init: { method: string; headers: Record<string, string>; body: string; signal: AbortSignal },
) => Promise<{ ok: boolean; status: number; text(): Promise<string> }>;

export type JevOptions = {
  apiKey: string;
  model?: string;
  url?: string;
  /** Injected in tests; the global fetch otherwise. */
  fetch?: FetchLike;
};

/** Cost and latency of one Jev call, kept beside the answers for the lane log. */
export type JevMeta = { ms: number; cost?: number; model: string };

/**
 * Reads OPENROUTER_API_KEY from an env-style file (`KEY=value`, optional `export `, optional quotes). Undefined when the
 * file or the line is missing. The value is never logged.
 */
export function loadJevKey(path: string): string | undefined {
  let raw: string;
  try {
    raw = readFileSync(path, "utf8");
  } catch {
    return undefined;
  }
  for (const rawLine of raw.split(/\r?\n/u)) {
    let line = rawLine.trim();
    if (line.startsWith("export ")) line = line.slice(7).trim();
    if (!line.startsWith("OPENROUTER_API_KEY=")) continue;
    const value = line.slice("OPENROUTER_API_KEY=".length).trim().replace(/^["']|["']$/gu, "");
    return value || undefined;
  }
  return undefined;
}

/** The plugin's question format → Jev's: boolean → noul; score passes through with its level list. Others are skipped. */
export function toJevQuestions(questions: Record<string, unknown>): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const [id, q] of Object.entries(questions)) {
    const src = q && typeof q === "object" ? (q as Record<string, unknown>) : {};
    if (src.type === "boolean") out[id] = { type: "noul", instructions: src.instructions, criteria: src.criteria };
    else if (src.type === "score" && Array.isArray(src.criteria)) out[id] = { type: "score", instructions: src.instructions, criteria: src.criteria };
  }
  return out;
}

function finite(v: unknown): v is number {
  return typeof v === "number" && Number.isFinite(v);
}

/** One Jev answer → the decision-role answer shape (`probabilityTrue` for Boolean questions). Undefined = unusable. */
function toAnswer(raw: unknown): DecisionAnswer | undefined {
  if (!raw || typeof raw !== "object") return undefined;
  const a = raw as Record<string, unknown>;
  if (a.type === "noul" || "noul" in a) {
    return finite(a.noul) && a.noul >= 0 && a.noul <= 1 ? { type: "boolean", probabilityTrue: a.noul } : undefined;
  }
  if (a.type === "score") {
    const probabilities: Record<string, number> = {};
    if (a.probabilities && typeof a.probabilities === "object") {
      for (const [k, v] of Object.entries(a.probabilities as Record<string, unknown>)) if (finite(v)) probabilities[k] = v;
    }
    return {
      type: "score",
      ...(finite(a.score) ? { score: a.score } : {}),
      ...(Object.keys(probabilities).length > 0 ? { probabilities } : {}),
    };
  }
  return undefined;
}

/**
 * Same contract as the host decision role's `evaluate`: `{status: "ok", result: {answers}}`, or `{status: "error", reason}`
 * on any failure (missing key, timeout, HTTP error, malformed body), so the caller falls back to keywords.
 */
export async function jevEvaluate(
  request: { state: unknown; questions: Record<string, unknown> },
  options: { timeoutMs: number } & JevOptions,
): Promise<DecisionOutcome & { meta?: JevMeta }> {
  const model = options.model ?? JEV_DEFAULT_MODEL;
  const t0 = Date.now();
  const doFetch = options.fetch ?? (globalThis.fetch as unknown as FetchLike | undefined);
  if (!options.apiKey) return { status: "error", reason: "no-key" };
  if (typeof doFetch !== "function") return { status: "error", reason: "no-fetch" };
  const questions = toJevQuestions(request.questions);
  if (Object.keys(questions).length === 0) return { status: "error", reason: "no-questions" };
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), Math.max(1, options.timeoutMs));
  try {
    const res = await doFetch(options.url ?? JEV_URL, {
      method: "POST",
      headers: {
        Authorization: `Bearer ${options.apiKey}`,
        "Content-Type": "application/json",
        "User-Agent": "probbit-stance/0.3",
      },
      body: JSON.stringify({ model, state: request.state, questions }),
      signal: controller.signal,
    });
    const body = await res.text();
    const ms = Date.now() - t0;
    if (!res.ok) return { status: "error", reason: `http-${res.status}`, meta: { ms, model } };
    let parsed: unknown;
    try {
      parsed = JSON.parse(body);
    } catch {
      return { status: "error", reason: "malformed-json", meta: { ms, model } };
    }
    const obj = parsed && typeof parsed === "object" ? (parsed as Record<string, unknown>) : undefined;
    const rawAnswers = obj?.answers;
    if (!rawAnswers || typeof rawAnswers !== "object" || Array.isArray(rawAnswers)) {
      return { status: "error", reason: "malformed-answers", meta: { ms, model } };
    }
    const answers: Record<string, DecisionAnswer> = {};
    for (const [id, raw] of Object.entries(rawAnswers as Record<string, unknown>)) {
      const a = toAnswer(raw);
      if (a) answers[id] = a;
    }
    if (Object.keys(answers).length === 0) return { status: "error", reason: "malformed-answers", meta: { ms, model } };
    const usage = obj?.usage && typeof obj.usage === "object" ? (obj.usage as Record<string, unknown>) : {};
    const cost = finite(usage.cost) ? usage.cost : undefined;
    return { status: "ok", result: { answers }, meta: { ms, model, ...(cost !== undefined ? { cost } : {}) } };
  } catch (err) {
    const ms = Date.now() - t0;
    const reason = controller.signal.aborted ? "timeout" : `fetch-${String((err as { name?: unknown })?.name ?? "error").slice(0, 40)}`;
    return { status: "error", reason, meta: { ms, model } };
  } finally {
    clearTimeout(timer);
  }
}

/** Binds a key and model into a `DecisionEvaluate` the classifier can call like the host's decision role. */
export function createJevEvaluate(opts: JevOptions): DecisionEvaluate {
  return (request, options) => jevEvaluate(request, { ...opts, timeoutMs: options.timeoutMs });
}
