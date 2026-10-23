import { createHash } from "node:crypto";
import { appendFileSync, mkdirSync } from "node:fs";
import { homedir } from "node:os";
import { dirname } from "node:path";
import type { OpenClawPluginApi, OpenClawPluginDefinition } from "openclaw/plugin-sdk/plugin-entry";
import { buildBlock, footerInputs, movedNames } from "./block.js";
import { baselineClassification, extractCurrentMessage, classify, type Classification, type DecisionEvaluate } from "./classify.js";
import { createJevEvaluate, loadJevKey, type FetchLike } from "./jev.js";
import { CONFIG_JSON_SCHEMA, resolveConfig, resolveLane, type StanceConfig } from "./config.js";
import { carefulFromDoc, carefulFromState, detectDestructive, digest, redactPreview } from "./gate.js";
import {
  appendEventLog,
  buildEventLine,
  elapsedHours,
  ensureLaneDirs,
  findLoggedEvent,
  lanePaths,
  readDeclaredInputs,
  readLastEpoch,
  runLiveEvent,
  writeLastEpoch,
  type DeclaredInputsCache,
  type LogMeta,
  type StanceDoc,
} from "./probbit.js";

export const PLUGIN_ID = "probbit-stance";

type Entry = { n: number; doc: StanceDoc; inputs?: string[]; moved?: string[] };
type Flight = { promise: Promise<Entry | undefined>; state: { late: boolean; warned: boolean } };

export type StanceDeps = {
  /** Wall clock in ms (tests pin it). */
  now?: () => number;
  home?: string;
  /** Replaces the global fetch for the Jev adapter (tests). */
  fetch?: FetchLike;
};

const SEEN_LIMIT = 256;
const TIMED_OUT = Symbol("timed-out");

function sha(text: string): string {
  return createHash("sha256").update(text).digest("hex");
}

function isoSeconds(ms: number): string {
  return new Date(ms).toISOString().replace(/\.\d{3}Z$/, "Z");
}

function latestUserTimestamp(messages: unknown): number | undefined {
  if (!Array.isArray(messages)) return undefined;
  for (let i = messages.length - 1; i >= 0; i--) {
    const m = messages[i] as { role?: unknown; timestamp?: unknown } | null;
    if (m && m.role === "user" && typeof m.timestamp === "number") return m.timestamp;
  }
  return undefined;
}

/** One event per admitted user message: the native admission id when the harness gives one, else text + an anchor. */
export function idempotencyKey(input: {
  lane: string;
  sessionKey: string;
  text: string;
  messageId?: string | undefined;
  runId?: string | undefined;
  messages?: unknown;
  nowMs: number;
}): { key: string; basis: string } {
  if (input.messageId) return { key: `mid:${sha(`${input.lane}\n${input.messageId}`).slice(0, 32)}`, basis: "message-id" };
  let anchor: string;
  let basis: string;
  if (input.runId) {
    anchor = `run:${input.runId}`;
    basis = "text+run-id";
  } else {
    const ts = latestUserTimestamp(input.messages);
    anchor = ts !== undefined ? `ts:${ts}` : `min:${Math.floor(input.nowMs / 60_000)}`;
    basis = ts !== undefined ? "text+message-timestamp" : "text+minute";
  }
  return { key: `h:${sha(`${input.lane}\n${input.sessionKey}\n${anchor}\n${input.text}`).slice(0, 32)}`, basis };
}

function decisionEvaluateFor(api: OpenClawPluginApi, agentId: string | undefined): DecisionEvaluate | undefined {
  const cfg = api.config as unknown as {
    agents?: { defaults?: { decisionModel?: unknown }; entries?: Record<string, { decisionModel?: unknown } | undefined> };
  };
  const perAgent = agentId ? cfg?.agents?.entries?.[agentId]?.decisionModel : undefined;
  const model = perAgent !== undefined ? perAgent : cfg?.agents?.defaults?.decisionModel;
  if (typeof model !== "string" || !model.trim()) return undefined;
  const decisions = (api as unknown as { runtime?: { decisions?: { evaluate?: unknown } } }).runtime?.decisions;
  const evaluate = decisions?.evaluate;
  if (typeof evaluate !== "function") return undefined;
  return (request, options) => (evaluate as DecisionEvaluate).call(decisions, request, options);
}

function raceTimeout<T>(promise: Promise<T>, ms: number): Promise<T | typeof TIMED_OUT> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const timeout = new Promise<typeof TIMED_OUT>((resolve) => {
    timer = setTimeout(() => resolve(TIMED_OUT), ms);
  });
  return Promise.race([promise, timeout]).finally(() => {
    if (timer) clearTimeout(timer);
  });
}

export function registerStance(api: OpenClawPluginApi, deps: StanceDeps = {}): StanceConfig {
  const home = deps.home ?? homedir();
  const now = deps.now ?? (() => Date.now());
  const { config: cfg, warnings } = resolveConfig(api.pluginConfig, home);
  const warn = (message: string): void => {
    try {
      api.logger.warn(`[${PLUGIN_ID}] ${message}`);
    } catch {
      // logging must never break a hook
    }
  };
  for (const w of warnings) warn(`config: ${w}`);
  if (Object.keys(cfg.lanes).length === 0) {
    warn("no lanes configured; no hooks registered");
    return cfg;
  }

  // The Jev key is read once here and only ever placed in the request header; it is never logged.
  let jevEvaluate: DecisionEvaluate | undefined;
  if (cfg.classifier === "jev") {
    const key = loadJevKey(cfg.jevKeyFile);
    if (key) jevEvaluate = createJevEvaluate({ apiKey: key, model: cfg.jevModel, ...(deps.fetch ? { fetch: deps.fetch } : {}) });
    else warn(`classifier jev: no OPENROUTER_API_KEY in ${cfg.jevKeyFile}; the adapter is off and keywords classify`);
  }
  /** The classifier for this turn and whether it is the named (Jev) one. */
  const classifierFor = (agentId: string | undefined): { evaluate?: DecisionEvaluate; named?: "jev" } => {
    if (cfg.classifier === "keyword") return {};
    if (cfg.classifier === "jev") return jevEvaluate ? { evaluate: jevEvaluate, named: "jev" } : {};
    const evaluate = decisionEvaluateFor(api, agentId);
    return evaluate ? { evaluate } : {};
  };

  const seen = new Map<string, Entry>();
  const inflight = new Map<string, Flight>();
  const laneChains = new Map<string, Promise<unknown>>();
  const lastByLane = new Map<string, Entry>();
  const declaredCache: DeclaredInputsCache = new Map();
  const warnedFlags = new Set<string>();
  let warnedUnreadable = false;

  /** Version skew, both ways: only flags the persona declares reach probbit; one warn per unknown flag per process. */
  const declaredOnly = (flags: string[]): { kept: string[]; dropped: string[] } => {
    const ids = readDeclaredInputs(cfg.personaPath, declaredCache);
    if (!ids) {
      if (!warnedUnreadable) {
        warnedUnreadable = true;
        warn(`persona inputs unreadable at ${cfg.personaPath}; flags passed through unfiltered`);
      }
      return { kept: flags, dropped: [] };
    }
    const kept: string[] = [];
    const dropped: string[] = [];
    for (const f of flags) (ids.has(f) ? kept : dropped).push(f);
    for (const f of dropped) {
      if (warnedFlags.has(f)) continue;
      warnedFlags.add(f);
      warn(`persona does not declare input "${f}"; flag dropped (warned once per process)`);
    }
    return { kept, dropped };
  };

  const remember = (lane: string, key: string, entry: Entry): void => {
    seen.set(key, entry);
    if (seen.size > SEEN_LIMIT) {
      const oldest = seen.keys().next().value;
      if (oldest !== undefined) seen.delete(oldest);
    }
    const prev = lastByLane.get(lane);
    if (!prev || entry.n >= prev.n) lastByLane.set(lane, entry);
  };

  const withLaneLock = <T>(lane: string, work: () => Promise<T>): Promise<T> => {
    const prev = laneChains.get(lane) ?? Promise.resolve();
    const next = prev.then(work, work);
    laneChains.set(
      lane,
      next.catch(() => undefined),
    );
    return next;
  };

  const inject = (entry: Entry) => {
    const block = buildBlock(
      entry.n,
      entry.doc,
      cfg.footer,
      {
        ...(entry.inputs ? { inputs: entry.inputs } : {}),
        ...(entry.moved ? { moved: entry.moved } : {}),
      },
      cfg.footerPrefix ? { prefix: cfg.footerPrefix } : {},
    );
    return cfg.inject === "appendSystemContext" ? { appendSystemContext: block } : { appendContext: block };
  };

  const takeEvent = async (
    lane: string,
    key: string,
    keyBasis: string,
    text: string,
    textSource: string,
    agentId: string | undefined,
    state: Flight["state"],
  ): Promise<Entry | undefined> => {
    const paths = lanePaths(cfg.stateDir, lane);
    ensureLaneDirs(cfg.stateDir);
    const logged = findLoggedEvent(paths.log, key);
    if (logged) return logged;
    const { evaluate, named } = classifierFor(agentId);
    const shadow = named !== undefined && cfg.classifierShadow;
    const clsPromise = classify(text, {
      ...(evaluate ? { evaluate } : {}),
      ...(named ? { named, shadow } : {}),
      ...(agentId ? { agentId } : {}),
      timeoutMs: cfg.classifierTimeoutMs,
      threshold: cfg.decisionThreshold,
    });
    // A handler is attached at once: a rejection while probbit runs must never surface as an unhandled rejection.
    const clsSettled = clsPromise.then(
      (c) => c,
      () => undefined,
    );
    // Shadow: the engine gets markers/keywords exactly as without a classifier, so probbit runs while Jev answers.
    const fed: Classification = shadow ? baselineClassification(text) : await clsPromise;
    const nowMs = now();
    const nowSec = Math.floor(nowMs / 1000);
    const hours = elapsedHours(nowSec, readLastEpoch(paths.last));
    const { kept: flags, dropped } = declaredOnly(fed.flags);
    const declared = readDeclaredInputs(cfg.personaPath, declaredCache);
    const timePressure =
      fed.timePressure !== undefined && fed.timePressure >= 0.05 && declared?.has("time_pressure") ? fed.timePressure : undefined;
    const eventLine = buildEventLine(flags, hours, timePressure !== undefined ? { time_pressure: timePressure } : {});
    const run = await runLiveEvent({
      binary: cfg.binary,
      personaPath: cfg.personaPath,
      seed: cfg.seed,
      paths,
      eventLine,
      killAfterMs: Math.max(5_000, cfg.timeoutMs * 10),
    });
    const cls: Classification = shadow ? ((await clsSettled) ?? fed) : fed;
    if (!run.ok) {
      if (!state.warned) {
        state.warned = true;
        warn(`lane ${lane}: no stance this turn (${run.reason})`);
      }
      return undefined;
    }
    writeLastEpoch(paths.last, nowSec);
    const inputs = footerInputs(flags, timePressure);
    const moved = movedNames(lastByLane.get(lane)?.doc, run.doc);
    const meta: LogMeta = {
      src: PLUGIN_ID,
      n: run.n,
      key,
      keyBasis,
      method: cls.method,
      flags,
      textSource,
      ms: run.ms,
      ...(state.late ? { late: true } : {}),
      ...(cls.probabilities ? { probabilities: cls.probabilities as Record<string, number> } : {}),
      ...(cls.decisionNote ? { decisionNote: cls.decisionNote } : {}),
      ...(dropped.length > 0 ? { dropped } : {}),
      ...(timePressure !== undefined ? { timePressure } : {}),
      ...(cls.classifier ? { classifier: cls.classifier as unknown as Record<string, unknown> } : {}),
      inputs,
      ...(moved ? { moved } : {}),
    };
    appendEventLog(paths.log, isoSeconds(nowMs), run.eventLine, run.outLine, meta);
    return { n: run.n, doc: run.doc, inputs, ...(moved ? { moved } : {}) };
  };

  api.on("before_prompt_build", async (event, ctx) => {
    try {
      const sessionKey = ctx?.sessionKey;
      const lane = resolveLane(cfg.lanes, sessionKey);
      if (!lane || !sessionKey) return;
      if (ctx.trigger === "heartbeat" || ctx.trigger === "cron") return;
      if (ctx.inputProvenance && ctx.inputProvenance.kind !== "external_user") return;
      if (cfg.senderIds.length > 0 && !(typeof ctx.senderId === "string" && cfg.senderIds.includes(ctx.senderId))) return;
      const hasCurrent = typeof event?.currentUserMessage === "string";
      let text: string;
      let textSource: string;
      if (hasCurrent) {
        text = event.currentUserMessage as string;
        textSource = "currentUserMessage";
      } else {
        const extracted = extractCurrentMessage(typeof event?.prompt === "string" ? event.prompt : "");
        text = extracted.text;
        textSource = extracted.source;
      }
      const { key, basis } = idempotencyKey({
        lane,
        sessionKey,
        text,
        messageId: event?.currentUserMessageId,
        runId: ctx.runId,
        messages: event?.messages,
        nowMs: now(),
      });
      const cached = seen.get(key);
      if (cached) return inject(cached);

      let flight = inflight.get(key);
      const joined = flight !== undefined;
      if (!flight) {
        try {
          ctx.hookInvocation?.assertActive();
        } catch {
          return;
        }
        const state = { late: false, warned: false };
        const promise = withLaneLock(lane, () =>
          takeEvent(lane, key, basis, text, textSource, ctx.agentId, state),
        )
          .then(
            (entry) => {
              if (entry) remember(lane, key, entry);
              return entry;
            },
            (err: unknown) => {
              if (!state.warned) {
                state.warned = true;
                warn(`lane ${lane}: no stance this turn (${String(err).slice(0, 200)})`);
              }
              return undefined;
            },
          )
          .finally(() => inflight.delete(key));
        flight = { promise, state };
        inflight.set(key, flight);
      }
      const budget = cfg.timeoutMs + (classifierFor(ctx.agentId).evaluate ? cfg.classifierTimeoutMs + 50 : 0);
      const entry = await raceTimeout(flight.promise, budget);
      if (entry === TIMED_OUT) {
        flight.state.late = true;
        if (!joined && !flight.state.warned) {
          flight.state.warned = true;
          warn(`lane ${lane}: probbit did not answer within ${budget} ms; no stance this turn`);
        }
        return;
      }
      return entry ? inject(entry) : undefined;
    } catch (err) {
      warn(`before_prompt_build skipped: ${String(err).slice(0, 200)}`);
      return;
    }
  });

  if (cfg.gateMode === "shadow") {
    let gateDirReady = false;
    api.on("before_tool_call", (event, ctx) => {
      try {
        const lane = resolveLane(cfg.lanes, ctx?.sessionKey);
        if (!lane) return;
        const params = event?.params && typeof event.params === "object" ? (event.params as Record<string, unknown>) : {};
        const cwdRaw = (ctx as unknown as { cwd?: unknown }).cwd;
        const toolName = typeof event?.toolName === "string" ? event.toolName : typeof ctx?.toolName === "string" ? ctx.toolName : "";
        const hit = detectDestructive(toolName, params, event?.derivedPaths, cfg.destructive, {
          home,
          ...(typeof cwdRaw === "string" ? { cwd: cwdRaw } : {}),
        });
        if (!hit) return;
        const paths = lanePaths(cfg.stateDir, lane);
        const fromState = carefulFromState(paths.state);
        const mem = lastByLane.get(lane);
        const check = mem && (fromState.source !== "state" || fromState.n === undefined || fromState.n === mem.n) ? carefulFromDoc(mem.doc, mem.n) : fromState;
        if (!check.careful) return;
        if (!gateDirReady) {
          mkdirSync(dirname(paths.gateLog), { recursive: true });
          gateDirReady = true;
        }
        const line = {
          ts: isoSeconds(now()),
          mode: "shadow",
          would: "deny",
          decided: "allow",
          lane,
          class: "destructive",
          policy: "actions.classes.destructive.deny_when[0]",
          rule: hit.rule,
          tool: toolName,
          ...(event?.toolCallId ? { toolCallId: event.toolCallId } : {}),
          ...(ctx?.runId ? { runId: ctx.runId } : {}),
          stance: {
            source: check.source,
            ...(check.n !== undefined ? { n: check.n } : {}),
            ...(check.caution ? { caution: check.caution } : {}),
            securityCareful: check.securityCareful,
          },
          detail: { kind: hit.detailKind, digest: digest(hit.detail), preview: redactPreview(hit.detail) },
        };
        appendFileSync(paths.gateLog, `${JSON.stringify(line)}\n`);
      } catch (err) {
        warn(`before_tool_call shadow gate skipped: ${String(err).slice(0, 200)}`);
      }
      return;
    });
  }
  return cfg;
}

const plugin = {
  id: PLUGIN_ID,
  name: "probbit stance",
  description: "Injects the lane's probbit stance into each turn and shadow-logs destructive tool calls a careful stance would deny.",
  configSchema: {
    safeParse(value: unknown) {
      const { warnings } = resolveConfig(value);
      if (warnings.length > 0) {
        return { success: false, error: { issues: warnings.map((message) => ({ path: [] as Array<string | number>, message })) } };
      }
      return { success: true, data: value };
    },
    jsonSchema: CONFIG_JSON_SCHEMA as unknown as NonNullable<NonNullable<OpenClawPluginDefinition["configSchema"]>["jsonSchema"]>,
  },
  register(api: OpenClawPluginApi) {
    registerStance(api);
  },
} satisfies OpenClawPluginDefinition;

export default plugin;
