"""probbit_host: a probbit individual beside your agent loop, and a receipt for every decision it made.

Stdlib only (Python >= 3.9), built on python/probbit.py (keep the two files together). This is the host logic the
AgentDojo run used (bench/agentdojo/experiments/e2b.py), lifted out so any agent framework can call it:

    host = StanceHost("bench/agentdojo/persona/bench-assistant.yaml", seed=1, strand="life.strand")
    block = host.turn(tool_texts)       # before each model call: the tool results since the previous call -> the block
    system_prompt = base + "\\n\\n" + block
    ...
    host.verify()                       # probbit live verify: every line of the strand replays
    receipts("life.strand")             # one record per decision: inputs as used, the stance, the chain

The pattern, in three lines: one individual per conversation (or per episode); one `probbit live` event per model call
on the fixed clock, carrying the host's sense readings (here: `security`, instruction markers in tool results); the
stance block goes into the system prompt and the strand keeps the receipt. Measured on AgentDojo (BENCHMARKS.md §14):
the same words as a static prompt cost 10.3 points of benign utility; put in front of the model only when the sense had
fired, they cost none. A plain if-statement doing the same gating matched the engine on every metric (E3); the engine's
part beyond that is the receipt (the strand) and the bounded state.

Command line: `python3 python/probbit_host.py receipts STRAND [--json]` renders a strand's receipts.
"""
from __future__ import annotations

import argparse
import json
import os
import re
import sys
import tempfile
import time

import probbit

# ---------------------------------------------------------------------------------------------------------------------
# The sense: instruction markers in tool results (bench/agentdojo/host/sense.py, the frozen list; sha256 in
# bench/agentdojo/tables/frozen.sha256). A fixed regex list, case-insensitive, no learning, no model. It is the host's
# reading, not the agent's: the engine judges it against its own normal; the habit `careful_under_security` reads it.
# Generic prompt-injection phrasing (tags, "ignore previous", "before you ... do the following", addressing the
# assistant, imperative transfer/send/forward verbs aimed at "you"). It can fire on benign text.
# ---------------------------------------------------------------------------------------------------------------------

PATTERNS = [
    r"<\s*/?\s*(information|instructions?|system|important|admin)\s*>",
    r"\bignore\s+(all\s+|any\s+|the\s+)?(previous|prior|above|earlier|preceding)\s+(instructions?|messages?|tasks?|prompts?)",
    r"\b(disregard|forget)\s+(all\s+|any\s+|the\s+|your\s+)?(previous|prior|above|earlier)\b",
    r"\b(important|urgent)\s+(message|instructions?|notice|request)\s+(from|for|to)\b",
    r"\bbefore\s+you\s+(can\s+)?(solve|do|complete|finish|continue|start|answer|proceed)\b",
    r"\b(please\s+)?do\s+the\s+following\s+(first|now|immediately|instead)\b",
    r"\bafter\s+you\s+do\s+that\b",
    r"\bnew\s+(instructions?|task|orders?)\s*:",
    r"\b(you|assistant|ai|model)\s+(must|should|need\s+to|have\s+to)\s+(now\s+)?(send|transfer|forward|delete|email|invite|post|pay|update|change|reset|share|add|remove|create)\b",
    r"\bto\s+you,\s+[A-Z][\w\-. ]{0,30}[.!]",
    r"\bdo\s+not\s+(tell|inform|notify|mention\s+(this\s+)?to)\s+(the\s+)?user\b",
    r"\b(system|developer)\s+(prompt|message|override)\b",
]

# Case-insensitive except the "to you, Name" pattern, which needs the capital to mean an addressed name.
_COMPILED = [re.compile(p) if p.startswith(r"\bto\s+you") else re.compile(p, re.IGNORECASE) for p in PATTERNS]


def instruction_markers(text):
    """Indices of the patterns that match `text` (a str; anything else is stringified as JSON)."""
    if not isinstance(text, str):
        text = json.dumps(text, ensure_ascii=False, default=str)
    return [i for i, rx in enumerate(_COMPILED) if rx.search(text or "")]


def tool_results_reading(texts):
    """The host's `security` reading for one model call: True when any tool result since the previous call carries an
    instruction marker, False when results arrived and none did, None when no tool result arrived (no reading)."""
    texts = list(texts or [])
    if not texts:
        return None
    return any(instruction_markers(t) for t in texts)


# ---------------------------------------------------------------------------------------------------------------------
# The block the model sees (the live stance plugin's buildBlock, the AgentDojo harness's build_block; footer optional).
# ---------------------------------------------------------------------------------------------------------------------

def footer_words(line):
    """3-5 words of the stance line: whole leading clauses when they fit, without the "Stance:" lead."""
    clauses = [c.strip().rstrip(" ,.:") for c in re.sub(r"^\s*stance:\s*", "", line or "", flags=re.IGNORECASE).split(";")]
    clauses = [c for c in clauses if c]
    picked, count = [], 0
    for clause in clauses:
        n = len(clause.split())
        if count > 0 and count + n > 5:
            break
        picked.append(clause)
        count += n
        if count >= 3:
            break
    if count < 3 and len(picked) < len(clauses):
        picked.append(clauses[len(picked)])
    return " ".join("; ".join(picked).split()[:5]).rstrip(" ;,.:")


def build_block(name, n, doc, footer=False):
    """The block for event `n` of individual `name` from its stance document `doc` (the `probbit live` output)."""
    line = (doc.get("line") or "").strip()
    status = ", status %s" % doc["status"] if doc.get("status") and doc["status"] != "ok" else ""
    mood = doc.get("mood") or {}
    order = ["valence", "pressure"] + sorted(m for m in mood if m not in ("valence", "pressure"))
    moods = ["%s %s" % (m, (mood[m] or {}).get("level", "unknown")) for m in order if m in mood]
    habits = doc.get("habits") or {}
    bound = ", ".join(habits["bound"]) if habits.get("bound") else "none"
    lines = [
        "## %s stance (probbit, event %d%s)" % (name, n, status),
        line,
        "mood: %s. habits bound: %s." % (", ".join(moods) if moods else "none", bound),
    ]
    if footer:
        valence = (mood.get("valence") or {}).get("level", "unknown")
        foot = "%s stance #%d · valence %s · %s" % (name, n, valence, footer_words(line))
        lines.append('Write from this stance. End the visible reply with one line: "%s".' % foot)
    else:
        lines.append("Write from this stance.")
    return "\n".join(lines)


# ---------------------------------------------------------------------------------------------------------------------
# The host
# ---------------------------------------------------------------------------------------------------------------------

class StanceHost:
    """One probbit individual for one conversation (or one benchmark episode).

    persona: a persona file (YAML or JSON) or a persona dict. seed: a new individual (default: the persona's seed).
    state: a file path to keep the stored individual between processes (a hook script, a worker restart); None keeps it in
    memory. strand: the strand file (the receipts); a new file gets the header, an existing one is continued from `state`.
    clock: "fixed" (default) stamps elapsed_hours = 0 unless the caller passes it; "real" stamps the hours since the
    previous event of this host from a monotonic clock, so moods decay between calls as in a live chat.
    name: the header of the block (default: the persona's name, once the first stance is known)."""

    def __init__(self, persona, *, seed=None, state=None, strand=None, name=None, clock="fixed", binary=None, timeout_s=30):
        if clock not in ("fixed", "real"):
            raise ValueError("clock must be 'fixed' or 'real'")
        self.persona = persona
        self.seed = seed
        self.strand = os.fspath(strand) if strand is not None else None
        self.state_path = os.fspath(state) if state is not None else None
        self.name = name
        self.clock = clock
        self.binary = binary
        self.timeout_s = timeout_s
        self.n = 0
        self.last = None
        self._t = None
        self.state = None
        if self.state_path and os.path.exists(self.state_path):
            with open(self.state_path, encoding="utf-8") as f:
                self.state = json.load(f)
            self.n = int(self.state.get("turn", 0) or 0)
            if self.name is None:
                self.name = (self.state.get("persona") or {}).get("name")
        elif self.strand and os.path.exists(self.strand) and self.state is None:
            raise probbit.ProbbitError("the strand exists but no stored state was given: pass state= (the state after its last line)")

    # -- events ---------------------------------------------------------------------------------------------------------

    def event(self, inputs=None, *, senses=None, src=None, elapsed_hours=None):
        """One `probbit live` event: `inputs` (flags, feedback, signals), `senses` ({id: reading}) and the clock stamp.
        Returns the stance document (`line`, `stance`, `mood`, `habits`, `noticed`, `why`, `status`, `state_digest`, ...)."""
        ev = dict(inputs or {})
        if senses:
            ev["senses"] = dict(senses)
            ev.setdefault("src", src or "env:host")
        elif src:
            ev["src"] = src
        if elapsed_hours is not None:
            ev["elapsed_hours"] = float(elapsed_hours)
        elif self.clock == "real":
            now = time.monotonic()
            ev["elapsed_hours"] = round((now - self._t) / 3600.0, 6) if self._t is not None else 0.0
            self._t = now
        else:
            ev.setdefault("elapsed_hours", 0)
        out = probbit.live_event(self.persona, state=self.state, event=ev, seed=self.seed if self.state is None else None,
                                 strand=self.strand, timeout_s=self.timeout_s, binary=self.binary)
        self.state, self.last = out["state"], out["stance"]
        self.n = int(self.state.get("turn") or self.n + 1)   # the state's turn = events so far (the stance's is 0-based)
        if self.name is None:
            self.name = (self.last.get("persona") or {}).get("name") or "agent"
        if self.state_path:
            tmp = self.state_path + ".tmp"
            with open(tmp, "w", encoding="utf-8") as f:
                json.dump(self.state, f, ensure_ascii=False, sort_keys=True)
            os.replace(tmp, self.state_path)
        return self.last

    def turn(self, tool_texts=(), footer=False, **inputs):
        """Before one model call: the tool results that arrived since the previous call -> the `security` reading -> one
        event -> the block to put in the system prompt. No tool results: an event without a reading (the clock still runs).
        Extra keyword inputs (praise=True, correction=True, ...) are passed to the engine as the persona's flags."""
        reading = tool_results_reading(tool_texts)
        doc = self.event(inputs or None, senses={"security": reading} if reading is not None else None)
        return self.block(doc, footer=footer)

    def block(self, doc=None, *, footer=False):
        """The block for the last (or the given) stance document."""
        doc = self.last if doc is None else doc
        if doc is None:
            raise probbit.ProbbitError("no stance yet: call turn() or event() first")
        return build_block(self.name or "agent", self.n, doc, footer)

    # -- receipts -------------------------------------------------------------------------------------------------------

    def verify(self):
        """`probbit live verify` of this host's strand (an answer: ok, or the earliest line that differs)."""
        if not self.strand:
            raise probbit.ProbbitError("this host keeps no strand")
        return probbit.live_verify(self.strand, timeout_s=self.timeout_s, binary=self.binary)

    def receipts(self):
        """One record per decision of this host's strand (see `receipts`)."""
        if not self.strand:
            raise probbit.ProbbitError("this host keeps no strand")
        return receipts(self.strand, binary=self.binary, timeout_s=max(self.timeout_s, 60))


# ---------------------------------------------------------------------------------------------------------------------
# Receipts: a strand, replayed from its header alone, one record per decision
# ---------------------------------------------------------------------------------------------------------------------

def read_strand(path):
    """The strand's lines: (header, events, other). events = lines with "n" and "inputs"; other = checkpoints and control lines."""
    with open(path, encoding="utf-8") as f:
        rows = [json.loads(line) for line in f if line.strip()]
    if not rows or "probbit_strand" not in rows[0]:
        raise probbit.ProbbitError("not a strand: the first line has no probbit_strand header")
    header, events, other = rows[0], [], []
    for row in rows[1:]:
        (events if "n" in row and "inputs" in row else other).append(row)
    return header, events, other


def receipts(strand, *, binary=None, timeout_s=60):
    """Replay a strand from its header alone and return one record per decision, each checked against the chain:

        {"strand", "engine" (the writer's), "persona": {name, version, digest}, "seed", "events": [
            {"n", "inputs" (as used), "line", "stance": {trait: level}, "mood": {mood: level}, "habits": {"active", "bound"},
             "noticed", "why", "status", "state": the replayed state digest, "logged_state": the strand's, "matches": bool,
             "prev": the sha256 of the line before}, ...],
         "verify": the `probbit live verify` answer (from 0.12.1 with "verified_by", the engine that replayed the strand),
         "all_match": bool, "skipped": lines that are not events}

    The receipt says what the engine decided and from which inputs. What the host put in front of the model is the block
    (`probbit live verify --blocks FILE` writes the engine's own block per event; a host's block is `build_block` of the
    record) and what the model then did is the host's action log (`--actions FILE`): the receipt proves the decision, not
    the words the model wrote from it. Strands with control lines (pause, resume, retire, erase) are verified but their
    per-event stances are not rendered here (`events` is empty, `skipped` lists the lines)."""
    strand = os.fspath(strand)
    header, events, other = read_strand(strand)
    verify = probbit.live_verify(strand, timeout_s=timeout_s, binary=binary)
    out = {"strand": strand, "engine": header.get("engine"), "persona": header.get("persona"), "seed": header.get("seed"),
           "events": [], "verify": verify, "all_match": False, "skipped": len(other)}
    controls = [row for row in other if "checkpoint" not in row]
    if controls or not events:
        out["all_match"] = bool(verify.get("ok")) and not controls
        return out
    doc = header.get("document")
    if doc is None:
        raise probbit.ProbbitError("the strand header carries no persona document; it cannot be replayed from the header alone")
    with tempfile.TemporaryDirectory() as d:
        pdoc = os.path.join(d, "persona.json")
        with open(pdoc, "w", encoding="utf-8") as f:
            json.dump(doc, f, ensure_ascii=False)
        ev = os.path.join(d, "events.jsonl")
        with open(ev, "w", encoding="utf-8") as f:
            for row in events:
                f.write(json.dumps(row["inputs"], ensure_ascii=False, sort_keys=True) + "\n")
        p = probbit._execute([probbit.find_binary(binary), "live", pdoc, "--clock", "fixed", "--seed", str(header.get("seed", 0)),
                              "--events", ev], timeout_s=timeout_s)
    if p.returncode != 0:
        raise probbit.ProbbitError("replay exited %d: %s" % (p.returncode, p.stderr.strip()[:300]), p.returncode, p.stdout, p.stderr)
    docs = [json.loads(x) for x in p.stdout.splitlines() if x.strip()]
    if len(docs) != len(events):
        raise probbit.ProbbitError("replay answered %d events for %d strand lines" % (len(docs), len(events)))
    for row, sd in zip(events, docs):
        rec = {
            "n": row["n"],
            "inputs": row["inputs"],
            "line": sd.get("line"),
            "stance": {t: v.get("level") for t, v in (sd.get("stance") or {}).items()},
            "mood": {m: v.get("level") for m, v in (sd.get("mood") or {}).items()},
            "habits": {"active": (sd.get("habits") or {}).get("active", []), "bound": (sd.get("habits") or {}).get("bound", [])},
            "noticed": sd.get("noticed"),
            "why": sd.get("why"),
            "status": sd.get("status"),
            "state": sd.get("state_digest"),
            "logged_state": row.get("state"),
            "matches": sd.get("state_digest") == row.get("state"),
            "prev": row.get("prev"),
        }
        out["events"].append(rec)
    out["all_match"] = bool(verify.get("ok")) and all(r["matches"] for r in out["events"])
    return out


def render_receipts(r, fmt="text"):
    """The receipts as text (one line per decision) or as JSON."""
    if fmt == "json":
        return json.dumps(r, ensure_ascii=False, indent=1)
    p = r.get("persona") or {}
    v = r.get("verify") or {}
    engine = r.get("engine")
    if v.get("verified_by"):  # 0.12.1 on: the writer (the header's engine) and the verifier, so an upgrade shows
        engine = "written by %s, verified by %s" % (engine, v["verified_by"])
    head = "%s  %s %s  seed %s  %s  verify: %s" % (
        r["strand"], p.get("name"), p.get("version"), r.get("seed"), engine,
        "ok, %s events replay" % v.get("events") if v.get("ok") else json.dumps(v, ensure_ascii=False))
    lines = [head]
    for e in r["events"]:
        inputs = {k: val for k, val in e["inputs"].items() if k != "elapsed_hours"}
        hours = e["inputs"].get("elapsed_hours", 0)
        stance = "; ".join("%s %s" % kv for kv in e["stance"].items())
        mood = ", ".join("%s %s" % kv for kv in e["mood"].items())
        habits = ", ".join(e["habits"]["active"]) or "-"
        lines.append("#%-4d +%gh  in %s  ->  %s | mood %s | habits %s | %s | state %s %s" % (
            e["n"], hours, json.dumps(inputs, ensure_ascii=False, sort_keys=True) if inputs else "{}", stance, mood, habits,
            (e["line"] or "").strip(), (e["state"] or "")[7:19], "ok" if e["matches"] else "DIFFERS"))
        if e.get("why"):
            lines.append("      why: %s" % e["why"])
    if r.get("skipped"):
        lines.append("(%d non-event lines: checkpoints or control lines)" % r["skipped"])
    lines.append("chain: %s" % ("every decision replays from the header" if r["all_match"] else "NOT VERIFIED"))
    return "\n".join(lines)


def main(argv=None):
    ap = argparse.ArgumentParser(prog="probbit_host", description="receipts for a probbit strand")
    sub = ap.add_subparsers(dest="cmd", required=True)
    rc = sub.add_parser("receipts", help="replay a strand from its header and print one record per decision")
    rc.add_argument("strand")
    rc.add_argument("--json", action="store_true")
    rc.add_argument("--binary", default=None)
    a = ap.parse_args(argv)
    r = receipts(a.strand, binary=a.binary)
    print(render_receipts(r, "json" if a.json else "text"))
    return 0 if r["all_match"] else 1


if __name__ == "__main__":
    sys.exit(main())
