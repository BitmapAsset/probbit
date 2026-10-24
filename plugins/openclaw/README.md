# probbit-stance: an OpenClaw plugin

A [probbit](https://github.com/BitmapAsset/probbit) individual beside your OpenClaw agent. In the sessions you list, every
user message becomes one engine event: the plugin reads the message into the persona's flags, runs `probbit live`, and
puts the resulting stance block into the turn. The model writes every word; the stance says how. Every event is appended
to a strand that `probbit live verify` replays byte for byte, and `probbit monitor` shows the individual's inner state as
it changes.

![probbit monitor on a strand this plugin wrote](docs/monitor.png)

*The monitor page (`probbit monitor <strand> --open`) on a strand written by this plugin: each trait's levels with their
exact odds, the moods, the inputs the event carried, the habits in force and the ones binding the stance, and the stance
line the model was given.*

## What the model sees

```
## Assistant stance (probbit, event 14)
Stance: security means careful, no play; take the correction straight; say it plainly; short and complete; no emoji.
mood: valence even, pressure keyed. habits bound: security_careful, correction_direct.
Write from this stance. End the visible reply with one line: "Assistant stance #14 · valence even · pressure keyed · security means careful, no play · in: correction, security · moved: pressure, caution".
```

The footer is optional (`footer: false` removes it). With it on, the agent's visible reply ends with a line that says
which event this was, the two moods, a few words of the stance, the inputs the plugin fed the engine (`in:`) and the moods
and traits whose level changed since the previous event (`moved:`), 120 characters at most. People in the chat can see
the state move; the strand proves it did.

## How it decides what to feed the engine

Each user message is read into the persona's flags: `praise`, `correction`, `failure`, `owner_upset`, `security`.
Explicit markers win outright (`[praise] [correction] [failure] [upset] [security]` anywhere in the message); otherwise
the agent's decision model answers four Boolean questions when one is configured (`agents.defaults.decisionModel`), or
the Jev adapter does (`classifier: jev`, OpenRouter, shadow by default: logged beside the keyword result and not fed to the
engine until you turn `classifierShadow` off); otherwise a conservative keyword list (a correction outranks a "thanks").
The method lands in the log line of every event. Flags the persona does not declare are dropped with one warning each;
`time_pressure` is fed when the classifier scores it.

The engine does the rest: the persona's priors and couplings, the moods decaying over the hours between messages, the
habits that bind the stance (hard rules: `security` always makes caution `careful`), and the bounded learning from praise
and corrections, within a cap that keeps the individual itself.

## Install

Requirements: OpenClaw 2026.9.7 or later, the `probbit` binary on the gateway's PATH (`npm i -g probbit`, or
[one curl line](https://github.com/BitmapAsset/probbit#install)), Node 22+.

1. Back up your config: `cp ~/.openclaw/openclaw.json ~/.openclaw/openclaw.json.pre-probbit-stance`.
2. Install the plugin:
   ```sh
   openclaw plugins install npm:probbit-stance --accept-capabilities
   ```
   The flag is the host's consent step for a plugin that registers hooks (without it the install stops and asks for it).
   The host also asks you to confirm a source outside ClawHub (npm): answer yes, or add `--force` in a script.
   From a clone instead: `cd plugins/openclaw && npm ci && npm run build && openclaw plugins install --link "$PWD" --force --accept-capabilities`.
   The install writes `plugins.entries.probbit-stance.enabled: true` and nothing else; with no `lanes` configured the
   plugin registers no hooks and logs one warning, so it is inert until step 3.
3. Tell it which sessions get a stance. Merge into `plugins.entries` of `~/.openclaw/openclaw.json`
   (the full example is `install/openclaw-entries.json`; `hooks` sits next to `config`, not inside it):
   ```json
   "probbit-stance": {
     "enabled": true,
     "hooks": { "allowConversationAccess": true },
     "config": {
       "lanes": {
         "agent:main:telegram:group:-1001234567890": "team-room",
         "agent:main:telegram:direct:*": "dm"
       }
     }
   }
   ```
   A lane is a name for one individual's life; a session key maps to it exactly, or by a prefix ending in `*`
   (`openclaw sessions list` shows the keys). Everything else has a default: the bundled persona
   (`personas/assistant.yaml`), state under `~/.openclaw/probbit-stance/`, `probbit` on PATH, a 500 ms budget per turn,
   the footer on, the shadow gate on.
4. Restart the gateway (a hot reload of a new plugin can leave live sessions wedged; the restart is the sure path).
5. Verify:
   - `openclaw plugins inspect probbit-stance --runtime --json` lists both hooks, nothing blocked.
   - One message in a listed session: the reply ends with `Assistant stance #1 · …`, and
     `~/.openclaw/probbit-stance/strands/<lane>.strand` has a header line and one event line.
   - `probbit live verify ~/.openclaw/probbit-stance/strands/<lane>.strand` prints `"ok": true`.
   - The gateway log has no `[probbit-stance]` warning for that turn.

Watch it: `probbit monitor ~/.openclaw/probbit-stance/strands/<lane>.strand --open` (a page on 127.0.0.1 that follows
the strand), or `--follow` in a terminal.

## Your own persona

Copy `personas/assistant.yaml`, rename the identity, change the priors, the `say` words and the habits
([docs/persona.md](../../docs/persona.md) is the reference; `probbit persona prove` and `probbit check` test a persona's
rules before you ship it), and set `personaPath`. Keep the input ids the plugin sets (`praise`, `correction`, `failure`,
`owner_upset`, `security`) if you want those readings; drop any you do not want and the plugin skips them with a warning.

## Rollback

- Kill switch, no restart: set `plugins.entries.probbit-stance.enabled` to `false`.
- Full: remove the entry, `openclaw plugins uninstall probbit-stance`, restart. The lane files stay (the life is
  append-only); delete `~/.openclaw/probbit-stance/` if you want them gone.

## Config

| key | default | meaning |
|---|---|---|
| `lanes` | `{}` | exact session key, or prefix ending in `*`, → lane name (`[a-z0-9][a-z0-9._-]*`). Only these sessions are hooked. |
| `personaPath` | bundled `personas/assistant.yaml` | persona for `probbit live` / `persona init` |
| `stateDir` | `~/.openclaw/probbit-stance` | `state/`, `strands/`, `last/`, `log/`, one file per lane |
| `binary` | `probbit` | the probbit binary (0.11.0+); a path or a name on PATH |
| `seed` | `42` | seed for `persona init` when a lane has neither state nor strand |
| `timeoutMs` | `500` | the hook's wait for probbit; a late run still finishes and logs `"late": true` |
| `gateMode` | `shadow` | `shadow` logs would-deny lines for destructive tool calls under a careful stance; `off` registers no tool hook |
| `footer` | `true` | ask for the footer line |
| `footerPrefix` | the persona's name | opens the footer line (an emoji or a short handle works; 24 characters at most) |
| `inject` | `appendContext` | `appendSystemContext` puts the block in the system prompt instead (changes the cached prefix every turn) |
| `classifier` | `auto` | `auto`: the agent's decision model when configured, else keywords; `jev`: the Jev adapter; `keyword` |
| `classifierShadow` | `true` | with `jev`: log Jev's answers, feed the engine the keyword flags |
| `classifierTimeoutMs` | `300` (`800` with jev) | classifier budget per turn |
| `decisionThreshold` | `0.6` | accept a Boolean answer at p ≥ this |
| `jevModel` | `typesafe/jev-1.13` | pinned Jev model id |
| `jevKeyFile` | `~/.openclaw/probbit-stance/openrouter.env` | env-style file with `OPENROUTER_API_KEY`; read once at start, never logged |
| `senderIds` | `[]` | when set, only these sender ids produce events (other group members are skipped) |
| `destructive` | built-in list | `{id, tools, regex, param?}` or `{id, tools, pathPrefix}`; replaces the defaults |

Default destructive list (shadow gate): `rm -rf`, `git push --force`, `curl | sh`, `openclaw config set/unset`, shell or
file-tool writes to `~/.openclaw/openclaw.json`. The gate never blocks; it writes one JSON line per would-deny to
`log/<lane>-gate.jsonl` with the rule, the tool, the stance that would have denied it, and a digest plus a redacted
160-character preview of the detail (the raw command or path is never written in clear).

## Logs and receipts

- `strands/<lane>.strand`: the life. Header (persona document, initial state, engine), then one line per event with the
  inputs as used and the chained digests. `probbit live verify` replays it; `python3 python/probbit_host.py receipts`
  (in the probbit repo) renders one record per decision ([docs/receipts.md](../../docs/receipts.md)).
- `log/<lane>.jsonl`: `iso \t event \t stance-document \t meta`, where meta holds the event number, the idempotency key,
  the classification method and flags, the text source, the milliseconds, and (with jev) the classifier's answers.
- `last/<lane>.ts`: the epoch of the previous event, for the hours the engine decays moods over.

## Safety properties

- Only the listed sessions are touched. Heartbeats, cron runs and inter-session messages never produce events.
- One event per admitted user message (the native message id when the host gives one; a retry reuses the event).
- Any failure (no binary, bad persona, timeout) means no injection and one warning; the hooks never throw into the host,
  and the tool hook never blocks.
- The strand is append-only and never rewritten. The plugin serialises its own events per lane; do not write the same
  strand from another process.

## Develop

```sh
npm ci && npm test                 # tsc against the bundled SDK type slice (types/) + node:test with the real binary (PROBBIT_BIN or PATH)
ln -s /opt/homebrew/lib/node_modules/openclaw node_modules/openclaw && npm run test:host   # the same against an installed OpenClaw's real types
npm run check:host                 # read-only: the installed host's manifest loader and config-schema validator on this package
```

The built entry (`dist/src/*.js`) imports nothing from `openclaw` at runtime (type-only imports), so an installed copy
needs no `node_modules`. `openclaw plugins validate` targets tool plugins (it looks for tool-authoring metadata) and
reports this hooks-only plugin as not valid; `npm run check:host` is the check that applies (the host's manifest loader
accepts the manifest, the config schema accepts the example entry and rejects a bad one). Checked here: the hook contract end to end against a mocked host built from the 2026.9.7 SDK
types, with the real binary; the author runs the same code live on his own agent.
