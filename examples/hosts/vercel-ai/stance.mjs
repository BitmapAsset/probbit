// probbit beside the Vercel AI SDK (ai >= 5): one individual per conversation, one event per step.
//   prepareStep runs before every model call: the tool results collected since the previous call go to the host
//   (host.turn), which runs one `probbit live` event and returns the block; the step's messages get a system message
//   with base instructions + block in front (prepareStep may return `messages`; docs: ai-sdk.dev/docs/agents/loop-control).
//   onToolExecutionEnd (experimental lifecycle callback) collects each tool result (docs: ai-sdk.dev/docs/ai-sdk-core/generating-text).
//
// Checked here: nothing in this file runs in the repository's tests (it needs the `ai` package, a provider and a key).
// The host it uses, examples/node/stance-host.mjs, is tested against the real binary (`node --test examples/node/stance-host.test.mjs`).
import { generateText, isStepCount } from 'ai';
import { StanceHost } from '../../node/stance-host.mjs';

const BASE = 'You are a careful assistant with tools. Finish the user\'s task.';

export async function runWithStance({ model, tools, prompt, persona = 'bench/agentdojo/persona/bench-assistant.yaml', seed = 1, strand = 'run.strand' }) {
  const host = new StanceHost(persona, { seed, strand });
  await host.init();
  let pending = [];
  const result = await generateText({
    model,
    tools,
    prompt,
    stopWhen: isStepCount(10),
    onToolExecutionEnd({ toolOutput }) {
      pending.push(toolOutput.type === 'tool-result' ? JSON.stringify(toolOutput.output) : String(toolOutput.error));
    },
    prepareStep: async ({ messages }) => {
      const block = await host.turn(pending);
      pending = [];
      return { messages: [{ role: 'system', content: `${BASE}\n\n${block}` }, ...messages.filter((m) => m.role !== 'system')] };
    },
  });
  const receipt = await host.verify(); // {ok: true, events, final_state, last_line}: every stance of this run replays from the strand's header
  return { result, receipt, strand };
}
