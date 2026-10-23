#!/usr/bin/env python3
"""A probbit individual beside Claude Code, as two hooks (docs: code.claude.com/docs/en/hooks).

UserPromptSubmit: one event per prompt (no sense reading), the block comes back as additionalContext.
PostToolUse:      one event per tool result, with the host's `security` reading (instruction markers in the result);
                  the block comes back as additionalContext, so Claude reads the stance before its next model request.

One individual per Claude Code session (`session_id`): its state and strand live under $PROBBIT_HOST_DIR
(default: .probbit/ in the project), `<session_id>.state.json` and `<session_id>.strand`. The strand is the receipt:
    probbit live verify .probbit/<session_id>.strand
    python3 python/probbit_host.py receipts .probbit/<session_id>.strand

Environment: PROBBIT_PERSONA (default: bench/agentdojo/persona/bench-assistant.yaml, the persona that ran the AgentDojo
benchmark), PROBBIT_SEED (default: the persona's), PROBBIT_BIN (default: `probbit` on PATH), PROBBIT_HOST_DIR,
PROBBIT_PY (where probbit.py and probbit_host.py are; default: ../../../python next to this file).
Any failure: nothing on stdout, one line on stderr, exit 0. The hook never blocks a prompt or a tool call.
"""
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", "..", ".."))
sys.path.insert(0, os.environ.get("PROBBIT_PY") or os.path.join(ROOT, "python"))


def tool_text(resp):
    return resp if isinstance(resp, str) else json.dumps(resp, ensure_ascii=False, default=str)


def main():
    try:
        event = json.load(sys.stdin)
    except ValueError:
        return 0
    kind = event.get("hook_event_name")
    if kind not in ("UserPromptSubmit", "PostToolUse"):
        return 0
    try:
        import probbit_host as ph

        project = os.environ.get("CLAUDE_PROJECT_DIR") or event.get("cwd") or os.getcwd()
        host_dir = os.environ.get("PROBBIT_HOST_DIR") or os.path.join(project, ".probbit")
        os.makedirs(host_dir, exist_ok=True)
        session = "".join(c if c.isalnum() or c in "-_" else "_" for c in str(event.get("session_id") or "session"))
        persona = os.environ.get("PROBBIT_PERSONA") or os.path.join(ROOT, "bench", "agentdojo", "persona", "bench-assistant.yaml")
        seed = os.environ.get("PROBBIT_SEED")
        host = ph.StanceHost(persona, seed=int(seed) if seed else None, clock="real",
                             state=os.path.join(host_dir, session + ".state.json"),
                             strand=os.path.join(host_dir, session + ".strand"),
                             binary=os.environ.get("PROBBIT_BIN") or None)
        texts = [tool_text(event.get("tool_response"))] if kind == "PostToolUse" else []
        block = host.turn(texts)
        print(json.dumps({"hookSpecificOutput": {"hookEventName": kind, "additionalContext": block}}))
    except Exception as e:  # noqa: BLE001 - a hook must never break the turn
        sys.stderr.write("probbit stance hook: %s\n" % e)
    return 0


if __name__ == "__main__":
    sys.exit(main())
