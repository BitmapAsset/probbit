/**
 * The slice of OpenClaw's plugin SDK this plugin uses, as type declarations, so the plugin compiles without the
 * `openclaw` package installed (its npm package is about 390 MB). Shapes copied from OpenClaw 2026.9.7
 * (`dist/plugin-sdk/plugin-entry.d.ts` and the hook types it re-exports: PluginHookBeforePromptBuildEvent,
 * PluginHookBeforePromptBuildResult, PluginHookAgentContext, PluginHookBeforeToolCallEvent, PluginHookToolContext,
 * PluginHookBeforeToolCallResult). Only the fields this plugin reads or returns are declared; every one of them is
 * optional in the host where it is optional here.
 *
 * `npm run build:host` compiles against the real SDK instead (`node_modules/openclaw` -> an installed OpenClaw), which is
 * how a shape drift between this file and a new host version is caught.
 */
declare module "openclaw/plugin-sdk/plugin-entry" {
  export type PluginHookInputProvenance = { kind: string; sourceSessionKey?: string };
  export type PluginHookInvocationContext = { assertActive(): void };

  export type PluginHookBeforePromptBuildEvent = {
    prompt: string;
    /** Current request before projection. Empty means no textual request; omission is legacy. */
    currentUserMessage?: string;
    /** Stable native admission identity across rebuilds; differs between admitted requests. */
    currentUserMessageId?: string;
    /** Session messages prepared for this run. */
    messages: unknown[];
  };
  export type PluginHookBeforePromptBuildResult = {
    systemPrompt?: string;
    prependContext?: string;
    appendContext?: string;
    toolsAllow?: string[];
    prependSystemContext?: string;
    appendSystemContext?: string;
  };
  export type PluginHookAgentContext = {
    runId?: string;
    jobId?: string;
    agentId?: string;
    sessionKey?: string;
    sessionId?: string;
    workspaceDir?: string;
    modelProviderId?: string;
    modelId?: string;
    messageProvider?: string;
    channel?: string;
    accountId?: string;
    chatId?: string;
    senderId?: string;
    trigger?: string;
    channelId?: string;
    inputProvenance?: PluginHookInputProvenance;
    hookInvocation?: PluginHookInvocationContext;
  };
  export type PluginHookToolContext = {
    agentId?: string;
    sessionKey?: string;
    sessionId?: string;
    runId?: string;
    abortSignal?: AbortSignal;
    toolName: string;
    toolCallId?: string;
    channelId?: string;
  };
  export type PluginHookBeforeToolCallEvent = {
    toolName: string;
    params: Record<string, unknown>;
    runId?: string;
    toolCallId?: string;
    derivedPaths?: readonly string[];
  };
  export type PluginHookBeforeToolCallResult = {
    params?: Record<string, unknown>;
    block?: boolean;
    blockReason?: string;
  };

  export type PluginHookEvents = {
    before_prompt_build: PluginHookBeforePromptBuildEvent;
    before_tool_call: PluginHookBeforeToolCallEvent;
  };
  export type PluginHookContexts = {
    before_prompt_build: PluginHookAgentContext;
    before_tool_call: PluginHookToolContext;
  };
  export type PluginHookResults = {
    before_prompt_build: PluginHookBeforePromptBuildResult | undefined | void;
    before_tool_call: PluginHookBeforeToolCallResult | undefined | void;
  };
  export type PluginHookName = keyof PluginHookEvents;
  export type PluginHookHandler<K extends PluginHookName> = (
    event: PluginHookEvents[K],
    ctx: PluginHookContexts[K],
  ) => PluginHookResults[K] | Promise<PluginHookResults[K]>;

  export type PluginLogger = {
    debug: (message: string) => void;
    info: (message: string) => void;
    warn: (message: string) => void;
    error: (message: string) => void;
  };

  export type OpenClawPluginApi = {
    id: string;
    name: string;
    /** `plugins.entries.<id>.config` from openclaw.json, validated by the manifest's configSchema before load. */
    pluginConfig: unknown;
    /** The resolved host configuration (read-only here). */
    config: unknown;
    runtime?: unknown;
    logger: PluginLogger;
    on<K extends PluginHookName>(name: K, handler: PluginHookHandler<K>): void;
  };

  export type OpenClawPluginConfigSchema = {
    safeParse(value: unknown): { success: true; data: unknown } | { success: false; error: { issues: Array<{ path: Array<string | number>; message: string }> } };
    jsonSchema?: Record<string, unknown>;
  };

  export type OpenClawPluginDefinition = {
    id: string;
    name: string;
    description?: string;
    configSchema?: OpenClawPluginConfigSchema;
    register(api: OpenClawPluginApi): void | Promise<void>;
  };
}
