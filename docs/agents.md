# Calling pbit from anything

`pbit` is one binary with a JSON contract: a document on stdin, one JSON document on stdout, and an exit code that says
what kind of answer it is. Anything that can start a process can use it: a shell, Python, Node, PowerShell, a CI job, or
an agent harness with a shell tool. No server, no bindings, no network. The document formats are in the README
("Use it from anything": the router document of `pbit decide`) and in [pbit-ir-json.md](pbit-ir-json.md) (programs for
`pbit run`).

| exit | meaning | what to do |
|---|---|---|
| 0 | an answer: verdict `exact`, `diagnostics_passed` or `partial` | act on `released`; escalate `escalated` (empty unless `partial`) |
| 1 | `infeasible`: no plan satisfies the rules (a proof) | relax a rule or a cap; it is an answer, not a crash |
| 2 | bad input: one `{"error": {"code", "path", "message"}}` object on stdout; a bad flag prints one line on stderr instead | fix the document or the flag |
| 3 | `refused` / `declined` (the gate or a cap said no), or `{"error": {"code": "numeric"}}` | escalate the whole decision; the best-effort plan is still in the output |

Every recipe below was run in the cross-platform matrix (`.github/workflows/bench.yml`; transcripts in
`scripts/bench/results/`), on the OSes named under each.

## Shell (bash, zsh, sh)

Run on Linux and macOS (and in Git Bash on Windows).

```sh
pbit demo --tasks 12 | pbit decide --pretty                 # a synthetic 12-task routing document, decided
pbit decide --budget-ms 200 < router.json > decision.json   # your document
case $? in 0) echo answer ;; 1) echo infeasible ;; 2) echo "bad input" ;; 3) echo "refused: escalate" ;; esac
```

Read fields with any JSON tool, e.g. `python3 -c 'import json,sys; print(json.load(sys.stdin)["verdict"])' < decision.json`.

## Python

Run on Linux, macOS and Windows: `python/pbit.py` is stdlib only (Python >= 3.9); `python3 python/test_pbit.py` is its test.

```python
import pbit                                        # python/pbit.py (copy it next to your code, or add python/ to sys.path)
decision = pbit.decide(pbit.demo(tasks=12), budget_ms=200)   # exit 1 and 3 come back as answers
answer = pbit.run(program, deadline_ms=1000)                  # a pbit-ir program; bad input raises pbit.PbitInputError
```

The binary is found via `binary=`, `$PBIT_BIN`, `pbit` on `PATH`, or `../target/release/pbit` next to `pbit.py`
(on Windows set `PBIT_BIN` to the full path of `pbit.exe`).

## Node

Run on Linux, macOS and Windows: [examples/node/decide.mjs](../examples/node/decide.mjs), no dependencies (Node >= 18).
It exports `pbit(args, input)`, which spawns the binary, writes the JSON, and resolves to
`{kind: 'answer' | 'infeasible' | 'bad_input' | 'refused', exitCode, output, stderr}`.

```sh
node examples/node/decide.mjs               # a 12-task demo, then one call per exit code (0, 1, 2, 3)
node examples/node/decide.mjs router.json   # decide your document
```

```js
import { pbit } from './decide.mjs';
const r = await pbit(['decide', '--budget-ms', '200'], problem);   // problem: an object or JSON text
if (r.kind === 'answer') act(r.output.plan, r.output.released); else escalate(r);
```

The npm package (`npm/`) installs the binary and a `pbit` command with exit codes passed through. On Windows, spawn
`pbit.exe` itself (set `PBIT_BIN`): Node cannot spawn npm's `pbit.cmd` shim without a shell.

## PowerShell

Run on Windows (`windows-latest`) with PowerShell 7.6 as written, and with Windows PowerShell 5.1 through `cmd` (below).

```powershell
pbit demo --tasks 12 | pbit decide | ConvertFrom-Json | Select-Object verdict, violations, ms
Get-Content router.json -Raw | pbit decide --budget-ms 200 | ConvertFrom-Json; $LASTEXITCODE   # 0 / 1 / 2 / 3
```

Windows PowerShell 5.1 (the `powershell.exe` that ships with Windows) adds a UTF-8 byte-order mark to text it pipes into
a program, even with `$OutputEncoding` set to `us-ascii` (the image default), and pbit 0.2.0 rejects that input: exit 2,
`bad JSON: unexpected character 'ï' at byte 0`. Use PowerShell 7 (`pwsh`), or let `cmd` do the piping and the
redirection; PowerShell then only reads pbit's output:

```powershell
cmd /c "pbit demo --tasks 12 | pbit decide" | ConvertFrom-Json | Select-Object verdict, violations, ms
cmd /c "pbit decide --budget-ms 200 < router.json" | ConvertFrom-Json; $LASTEXITCODE
```

PowerShell 7 writes a byte-order mark too if a profile sets `$OutputEncoding = [Text.Encoding]::UTF8` (a string piped
into pbit then exits 2; seen with PowerShell 7.6); `$OutputEncoding = [System.Text.UTF8Encoding]::new($false)` fixes that.

## For agents

Coding agents with a shell tool (Claude Code, Codex, and other harnesses that can run commands) call pbit the same way a
script does: they run the command, read the JSON, and branch on the exit code. Nothing needs to be registered. A
paragraph like this one in the project's agent instructions (`CLAUDE.md`, `AGENTS.md`) is enough:

> To route tasks to workers under hard rules (allowed workers, quotas) with odds, write a router document (README, "Use it
> from anything") and run `pbit decide --budget-ms 200 < doc.json`. Exit 0: act on `plan` for the ids in `released` and
> hand the ids in `escalated` to a human. Exit 1: no plan satisfies the rules. Exit 2: fix the document (the `error`
> object says where). Exit 3: escalate everything. Never read stderr as data. `pbit <command> --help` lists every flag.

Practical notes for agent use:

- Bound the call: `--budget-ms` caps the sampler (with `--budget-ms 300` the 300-task demo answered in 420-495 ms on
  every machine in BENCHMARK-MATRIX.md: the budget, then the gate and the polish), and `pbit run --deadline-ms N` bounds
  a whole `run` call (a target, not a hard limit on a slow CPU: PORTABILITY.md). Small documents (about 12 tasks by 6
  workers) are answered exactly.
- Fixed work is reproducible: `--sweeps N --polish-ms 0` makes the answer a pure function of the document and `--seed`,
  which is what a test or a replayed agent step wants.
- `pbit stats` prints the machine, the effective controls and a measured self-test; `--threads`, `--cpu-limit` and
  (on Linux and macOS) `--priority low` keep it from crowding the agent's own process.
- An MCP server is planned for a later release and is not built yet; until then the shell command above is the
  integration.
