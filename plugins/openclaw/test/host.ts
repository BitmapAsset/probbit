/**
 * A mocked OpenClaw host. Event, context and result types are taken from the installed SDK by instantiating
 * `OpenClawPluginApi["on"]` per hook, so a shape drift in 2026.9.7's types fails `tsc`, not just a test.
 * Source of the shapes: /opt/homebrew/lib/node_modules/openclaw/dist/channel-core-BrH0n2qr.d.ts
 * (PluginHookBeforePromptBuildEvent :17549, PluginHookBeforePromptBuildResult :17558, PluginHookAgentContext :18236,
 *  PluginHookToolContext :18584, PluginHookBeforeToolCallEvent :18607, PluginHookBeforeToolCallResult :17588,
 *  PluginHookAfterToolCallEvent :18629).
 * Runner semantics mirrored from docs/plugins/hooks/reference.md: before_prompt_build logs and skips a throwing
 * handler; before_tool_call fails closed (a throw blocks the call); after_tool_call is observe-only (a throw is logged).
 */
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import type { OpenClawPluginApi } from "openclaw/plugin-sdk/plugin-entry";

declare const sdkApi: OpenClawPluginApi;
export type PromptBuildHandler = Parameters<typeof sdkApi.on<"before_prompt_build">>[1];
export type PromptBuildEvent = Parameters<PromptBuildHandler>[0];
export type AgentCtx = Parameters<PromptBuildHandler>[1];
export type PromptBuildResult = Awaited<ReturnType<PromptBuildHandler>>;
export type ToolCallHandler = Parameters<typeof sdkApi.on<"before_tool_call">>[1];
export type ToolCallEvent = Parameters<ToolCallHandler>[0];
export type ToolCtx = Parameters<ToolCallHandler>[1];
export type ToolCallResult = Awaited<ReturnType<ToolCallHandler>>;
export type AfterToolHandler = Parameters<typeof sdkApi.on<"after_tool_call">>[1];
export type AfterToolEvent = Parameters<AfterToolHandler>[0];

/** The probbit binary: $PROBBIT_BIN, else `probbit` on PATH. */
export const PROBBIT = process.env.PROBBIT_BIN ?? "probbit";
/** The bundled persona (the plugin's default personaPath). */
export const PERSONA_SRC = fileURLToPath(new URL("../../personas/assistant.yaml", import.meta.url));
/** An older-style persona (no `win`/`wants_energy` inputs): the bundled one without those two. */
export const PERSONA_V01 = fileURLToPath(new URL("../../test/fixtures/assistant-v0.1.yaml", import.meta.url));
/** The bundled persona plus a senses block declaring `tempo` (number, src env) and `tool_fail` (flag, src env). */
export const PERSONA_SENSES = fileURLToPath(new URL("../../test/fixtures/assistant-senses.yaml", import.meta.url));
export const GROUP_KEY = "agent:main:telegram:group:-1001234567890";

export type LogLine = { level: string; message: string };

export function tempStateDir(): string {
  return mkdtempSync(join(tmpdir(), "probbit-stance-"));
}

export function createHost(pluginConfig: Record<string, unknown>, extra: { config?: unknown; runtime?: unknown } = {}) {
  const promptHandlers: PromptBuildHandler[] = [];
  const toolHandlers: ToolCallHandler[] = [];
  const afterToolHandlers: AfterToolHandler[] = [];
  const logs: LogLine[] = [];
  const registered: string[] = [];
  const logger = {
    debug: (message: string) => logs.push({ level: "debug", message }),
    info: (message: string) => logs.push({ level: "info", message }),
    warn: (message: string) => logs.push({ level: "warn", message }),
    error: (message: string) => logs.push({ level: "error", message }),
  };
  const api = {
    id: "probbit-stance",
    name: "probbit stance",
    pluginConfig,
    config: extra.config ?? {},
    runtime: extra.runtime ?? {},
    logger,
    on(name: string, handler: unknown) {
      registered.push(name);
      if (name === "before_prompt_build") promptHandlers.push(handler as PromptBuildHandler);
      if (name === "before_tool_call") toolHandlers.push(handler as ToolCallHandler);
      if (name === "after_tool_call") afterToolHandlers.push(handler as AfterToolHandler);
    },
  } as unknown as OpenClawPluginApi;

  return {
    api,
    logs,
    registered,
    async promptBuild(event: PromptBuildEvent, ctx: AgentCtx): Promise<{ result: PromptBuildResult; threw: boolean }> {
      let result: PromptBuildResult = undefined;
      let threw = false;
      for (const h of promptHandlers) {
        try {
          result = (await h(event, ctx)) ?? result;
        } catch {
          threw = true;
        }
      }
      return { result, threw };
    },
    async toolCall(event: ToolCallEvent, ctx: ToolCtx): Promise<{ result: ToolCallResult; threw: boolean; blocked: boolean }> {
      let result: ToolCallResult = undefined;
      let threw = false;
      for (const h of toolHandlers) {
        try {
          result = (await h(event, ctx)) ?? result;
        } catch {
          threw = true;
        }
      }
      const blocked = threw || (result !== undefined && result !== null && (result as { block?: boolean }).block === true);
      return { result, threw, blocked };
    },
    async afterToolCall(event: AfterToolEvent, ctx: ToolCtx): Promise<{ threw: boolean }> {
      let threw = false;
      for (const h of afterToolHandlers) {
        try {
          await h(event, ctx);
        } catch {
          threw = true;
        }
      }
      return { threw };
    },
  };
}

/** A CLI-runner-shaped prompt event (the CLI path passes prompt + messages only; see prepare.runtime-BDdlx7B4.mjs:941). */
export function cliEvent(prompt: string): PromptBuildEvent {
  return { prompt, messages: [] };
}

/** An embedded-harness-shaped event with the native admission identity. */
export function embeddedEvent(text: string, id: string): PromptBuildEvent {
  return { prompt: `history...\n${text}`, currentUserMessage: text, currentUserMessageId: id, messages: [] };
}

export function groupCtx(runId: string, extra: Partial<AgentCtx> = {}): AgentCtx {
  return {
    runId,
    agentId: "main",
    sessionKey: GROUP_KEY,
    sessionId: "s-1",
    trigger: "user",
    channel: "telegram",
    messageProvider: "telegram",
    ...extra,
  };
}

export function toolCtx(extra: Partial<ToolCtx> = {}): ToolCtx {
  return { agentId: "main", sessionKey: GROUP_KEY, sessionId: "s-1", runId: "r-tool", toolName: "exec", ...extra };
}
