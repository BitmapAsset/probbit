# probbit as an MCP server

`probbit mcp` is a Model Context Protocol server on stdio: JSON-RPC 2.0, one message per line, no network, no API key.
It gives an agent the same engine the CLI has: a persona's stance for this turn (a level per trait with exact odds, the
habits in force, a stance line for the prompt), a resident individual that logs its life to a strand and replays it, a
search for counterexamples to a character rule, and plans that keep hard rules with odds for every part.

It serves the MCP revision 2026-07-28 and answers an `initialize` with the revisions 2025-11-25, 2025-06-18, 2025-03-26
and 2024-11-05. Paths in tool arguments (`persona_path`, `strand_path`) are read and written on the server's disk,
relative to the directory the client starts it in.

Install the binary first: `npm i -g probbit` (Node 18+), or the install script for your system in the
[README](../README.md) (Linux, macOS, Windows).

`probbit version` prints the version; the server reports the same one as `serverInfo.version`.

## The tools

`tools/list` gives 10 tools. Every one but `probbit_live_event` is read-only; `probbit_live_event` writes the strand
when it is given a `strand_path`. Every tool says `openWorldHint: false` (no network).

| tool | read-only (`readOnlyHint`) | description (its first sentence, as `tools/list` prints it) |
|---|---|---|
| `probbit_decide` | true | probbit decide: assign every task to one worker under hard rules (allowed sets, quotas, clamps) with workflow affinity |
| `probbit_run` | true | probbit run: a general constrained categorical program (variables, values, scores, pairs, caps, all_different, implies, tables, precedes, linear) |
| `probbit_stats` | true | probbit stats: machine, build features, effective controls and a measured self-test (site updates per second). |
| `probbit_demo` | true | probbit demo: a seeded synthetic agent-routing document (PII and prod-DB rules, quotas, workflow affinity) to pass to probbit_decide. |
| `probbit_evaluate` | true | probbit evaluate: a decision model's System One request (questions noul \| choice \| score keyed by id) with the judge's answers or per-question weights and rules over question ids (caps, implies, tables, precedes, linear, all_different, pairs) |
| `probbit_persona_init` | true | probbit persona init: a persona (inline document or a file path) and a seed -> the individual's initial state (genes from the seed, resting stance, empty mood and history) |
| `probbit_persona_turn` | true | probbit persona turn: a persona (inline document or a file path), the individual's state and this turn's inputs (the persona's evidence: sentiment, error, loss, praise, criticism, stakes, time_pressure, claim_done, elapsed_hours, and the persona's own; `goals` for a persona with a drives block) -> {stance, state} |
| `probbit_persona_fuzz` | true | probbit persona fuzz: a persona (inline document or a file path) and a character property in habit syntax (never: {when, then}; or props: a list of them) -> for each individual (seeds, default 0-99) the shortest event script whose stance breaks it, shrunk, with the replay command; `found` says whether any did |
| `probbit_live_event` | false | probbit live, one event: a persona (inline document or a file path), the individual's state (or a seed for a new one) and one event (inputs keyed by input id, with elapsed_hours since the previous event from the host's clock; quantised to 1e-6 h) -> {stance, state}: moods decay by their half-lives over the elapsed hours, and with a learning block the reward / correction flags move the learned deltas within their caps (habits never move) |
| `probbit_live_verify` | true | probbit live verify: a strand (its text, or a file path on the server's disk) -> {ok: true, events, persona, seed, engine, verified_by, final_state, last_line} when every line replays byte for byte from the header alone, else {ok: false, line, diverges} with the earliest line that differs and what (a changed input, a removed or reordered line, a changed header), or {ok: false, line, unsupported, engine, strand_engine} at the earliest line with a field or line kind this engine does not know (unsupported: the field's path; a newer engine's strand, not verified and not a divergence) |

## Setup

Each block is the whole setup for one client. "Run here" means the maintainers ran it on 2026-11-18 with a 0.15.1-dev
build and saw the result named; "not run here" means it was not run (the label says why), so the block follows the
client's own documentation and is untested by us. Replace `probbit` with the binary's full path if the client does not inherit your
`PATH`.

**Claude Code** (run here with `--scope project`: `claude mcp list` showed the server, and `claude -p` called `probbit_demo` and answered
"12 tasks and 6 workers")

```sh
claude mcp add probbit -- probbit mcp                    # for you, in this project
claude mcp add --scope project probbit -- probbit mcp    # shared through .mcp.json; Claude Code asks once to approve it
```

**Codex CLI** (run here: `codex mcp list` showed it enabled, and `codex exec` called `probbit_demo` and answered "12 tasks
and 6 workers")

```sh
codex mcp add probbit -- probbit mcp
```

That writes to `~/.codex/config.toml`:

```toml
[mcp_servers.probbit]
command = "probbit"
args = ["mcp"]
```

**Gemini CLI** (run here: `gemini mcp list` showed `probbit ... (stdio) - Connected`; no model call was made)

```sh
gemini mcp add probbit probbit mcp                       # .gemini/settings.json in this folder (the default scope)
gemini mcp add --scope user probbit probbit mcp          # every folder
```

**Claude Desktop** (not run here: not installed on the test machine). In `claude_desktop_config.json` (Settings, Developer, Edit Config), then restart the app:

```json
{"mcpServers": {"probbit": {"command": "probbit", "args": ["mcp"]}}}
```

**Cursor** (not run here: not installed on the test machine). In `~/.cursor/mcp.json`, or `.cursor/mcp.json` in a project:

```json
{"mcpServers": {"probbit": {"command": "probbit", "args": ["mcp"]}}}
```

**VS Code** (not run here: not installed on the test machine). In `.vscode/mcp.json` of the workspace:

```json
{"servers": {"probbit": {"type": "stdio", "command": "probbit", "args": ["mcp"]}}}
```

**OpenClaw** (not run here: adding a server changes a running gateway's configuration, which these checks do not do; the
flags are from `openclaw mcp add --help` of OpenClaw 2026.10.8)

```sh
openclaw mcp add probbit --command probbit --arg mcp     # connects once before saving; --no-probe skips that
```

**Without a global install** (run here: `npx -y probbit@0.15.0 mcp` from an empty npm cache answered `initialize` and
listed 10 tools): any client above can start `npx -y probbit mcp` instead of `probbit mcp`
(command `npx`, args `["-y", "probbit", "mcp"]`); the npm package fetches the release binary of its own version and checks its SHA-256.


## Two worked examples

Both ran over stdio on 2026-11-18 against a 0.15.1-dev build (the `engine` field below names it, and the hashes are
that build's), from the repository root, after `initialize`. The responses are the JSON in `result.content[0].text`, shortened where marked.

**1. A resident individual: one event, then its receipt.** The tutor persona, seed 2, one event from an upset learner,
logged to `pip.strand`:

```json
{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"probbit_live_event","arguments":{
  "persona_path":"examples/persona/tutor.yaml","seed":2,"strand_path":"pip.strand",
  "event":{"sentiment":"negative","elapsed_hours":0}}}}
```

The answer has `stance`, `state` and `strand`. From the stance (shortened): `"status":"ok"`, humour `none` with odds
`{"light":0.179758,"none":0.820242,"playful":0}`, `"why":"learner upset -> valence down, humour down"`, the habits in
force `["no_play_when_upset","flourish_budget","no_formal_emoji","check_last"]`, and the line to put in the prompt:

```text
Stance: go easy on the jokes; no jokes; gentle; explain step by step; warm and encouraging; suggest what to try next; ask what they think first; emoji welcome.
```

and `"strand":{"events":1,"head":"sha256:8b9a27df…","path":"pip.strand"}`. Then the receipt:

```json
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"probbit_live_verify","arguments":{"strand_path":"pip.strand"}}}
```

```json
{"engine":"probbit 0.15.1-dev","events":1,"final_state":"sha256:c56c8ee1…","last_line":"sha256:8b9a27df…","ok":true,"persona":"Pip","seed":2,"verified_by":"probbit 0.15.1-dev"}
```

`ok: true` means every line of the strand replayed byte for byte from its header alone; `last_line` is the head the event
returned.

**2. A plan under hard rules.** `probbit_demo` with `{"seed": 1}` returns a synthetic routing document (12 tasks, 6
workers, rules on personal data and the production database, quotas); passing that document unchanged as the arguments
of `probbit_decide` returns (shortened) `"verdict":"exact"`, `"tier":"enumerate"`, `"n_feasible":180540`,
`"violations":0`, `"plan_status":"released"`, a `plan` with one worker per task and `odds` per task over the workers.
`exact` means the odds were computed by enumerating all 180,540 plans that keep every rule, not sampled.

## Troubleshooting

- **The client shows no probbit tools, or says the command was not found.** GUI clients often start servers without your
  shell's `PATH`. Put the full path from `command -v probbit` (Windows: `where probbit`) in `command`.
- **Claude Code lists the server as "Pending approval".** A server from `.mcp.json` (`--scope project`) waits until you
  approve it: run `claude` in that folder once.
- **A file is not found.** `persona_path` and `strand_path` are on the server's disk, relative to the folder the client
  started it in. Use absolute paths when unsure.
- **An event is refused with an error at `arguments.inputs.src`.** The persona has `reward_from` ([persona.md](persona.md) §2.10): an event whose
  reward is on (for example `"praise": true`) needs `"src"`. The error names the field and shows one accepted line.
- **Check the server without a client.** `printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}' '{"jsonrpc":"2.0","method":"notifications/initialized"}' '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' | probbit mcp`
  prints two lines: the `initialize` answer and the 10 tools.
