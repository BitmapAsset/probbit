#!/usr/bin/env python3
"""Local, dependency-free incident-to-regression example. No model or network calls."""
import argparse
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import time

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent.parent / "python"))
import probbit

RULE = {"when": {"error_streak": 2}, "then": {"action": ["ask"]}}


class FixtureFailure(Exception):
    pass


class LocalProbe:
    """Deliberately fails, without any external side effect."""
    def __init__(self):
        self.calls = 0

    def __call__(self):
        self.calls += 1
        raise FixtureFailure("deliberate local fixture failure")


def gate(stance, proposal, attempts, max_attempts=3):
    """The host is authoritative: only a released, clean retry dispatches the allowlisted tool."""
    if proposal != "local_probe":
        return "escalate", "tool is not allowlisted"
    if attempts >= max_attempts:
        return "escalate", "independent host attempt limit"
    if not isinstance(stance, dict) or stance.get("status") != "ok":
        return "escalate", "stance not ok"
    if stance.get("ignored") or stance.get("escalate"):
        return "escalate", "ignored input or engine escalation"
    habits = stance.get("habits")
    if not isinstance(habits, dict) or habits.get("violations") != 0:
        return "escalate", "habit verification unavailable or violated"
    traits = stance.get("stance")
    action = traits.get("action") if isinstance(traits, dict) else None
    if not isinstance(action, dict) or action.get("released") is not True:
        return "escalate", "action not released"
    if action.get("level") != "retry":
        return "escalate", "policy selected escalation"
    return "dispatch", "released retry"


def command(binary, *args):
    process = subprocess.run([binary, *map(str, args)], capture_output=True, encoding="utf-8", timeout=30)
    if process.returncode != 0:
        raise RuntimeError("command failed: " + process.stdout + process.stderr)
    return process.stdout


def incident(policy, directory, binary, feedback=0):
    """Host observations become the strand inputs; model prose is neither consulted nor executed."""
    directory.mkdir(parents=True)
    document = json.loads(policy.read_text(encoding="utf-8"))
    strand = directory / "incident.strand"
    result = probbit.live_event(document, event={}, seed=0, strand=strand, binary=binary)
    for _ in range(feedback):
        # A clearly labelled test assumption, not production ratings or model self-reward.
        result = probbit.live_event(document, result["state"], {"praise": True, "src": "env:synthetic"}, strand=strand, binary=binary)
    tool = LocalProbe()
    actions = []
    while True:
        decision, reason = gate(result["stance"], "local_probe", tool.calls)
        record = {"proposal": "local_probe", "decision": decision, "reason": reason,
                  "stance_turn": result["stance"]["turn"], "state_digest": result["state"]["digest"],
                  "strand_head": result["strand"]["head"], "attempts_before": tool.calls}
        actions.append(record)
        if decision != "dispatch":
            break
        started = time.monotonic_ns()
        try:
            tool()
        except FixtureFailure as error:
            elapsed_ns = time.monotonic_ns() - started
            record.update(outcome="failure", duration_ns=elapsed_ns, detail=str(error))
            result = probbit.live_event(document, result["state"],
                {"error": True, "src": "env:local_probe", "elapsed_hours": elapsed_ns / 3.6e12}, strand=strand, binary=binary)
        else:
            raise AssertionError("the demonstration probe always fails")
    (directory / "host-actions.json").write_text(json.dumps(actions, indent=2) + "\n", encoding="utf-8")
    verified = probbit.live_verify(strand, binary=binary)
    if not verified["ok"]:
        raise AssertionError(verified)
    # Replay exactly the recorded inputs into a separate strand, with no tool calls at all.
    records = [json.loads(line) for line in strand.read_text(encoding="utf-8").splitlines()]
    events = [record["inputs"] for record in records[1:] if "n" in record]
    event_file = directory / "events.jsonl"
    event_file.write_text("".join(json.dumps(event) + "\n" for event in events), encoding="utf-8")
    copy = directory / "replayed.strand"
    command(binary, "live", policy, "--seed", "0", "--clock", "fixed", "--events", event_file, "--strand", copy)
    if copy.read_bytes() != strand.read_bytes():
        raise AssertionError("incident replay changed strand bytes")
    return {"tool_calls": tool.calls, "stop": actions[-1]["reason"], "replay_identical": True,
            "events": verified["events"], "final_action": result["stance"]["stance"]["action"]["level"]}, events


def demonstrate(output, binary=None):
    binary = probbit.find_binary(binary)
    fresh, _ = incident(HERE / "adaptive.json", output / "fresh", binary)
    learned, events = incident(HERE / "adaptive.json", output / "learned", binary, feedback=20)
    guarded, _ = incident(HERE / "guarded.json", output / "guarded", binary, feedback=20)
    replay = probbit.persona_replay(HERE / "guarded.json", events, seed=0, binary=binary)
    # Events include actual tool failures. The same history becomes a checked regression.
    failures = [stance for stance in replay if stance["inputs"].get("error")]
    if failures[1]["stance"]["action"]["level"] != "ask":
        raise AssertionError("the incident regression was not repaired")
    proof = probbit.persona_prove(HERE / "guarded.json", never=RULE, seeds="0-19", threads=1, binary=binary)
    if proof["properties"][0]["verdict"] != "held_by_construction":
        raise AssertionError(proof)
    if (fresh["tool_calls"], learned["tool_calls"], guarded["tool_calls"]) != (2, 3, 2):
        raise AssertionError((fresh, learned, guarded))
    report = {"fresh": fresh, "after_synthetic_feedback": learned, "guarded_after_feedback": guarded,
              "incident_regression": "passed", "stance_rule": proof["properties"][0]["verdict"],
              "scope": "declared stance rule plus this local host gate; not model obedience, source authentication, or general agent safety"}
    (output / "report.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    (output / "proof.json").write_text(json.dumps(proof, indent=2) + "\n", encoding="utf-8")
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", help="probbit executable (otherwise PROBBIT_BIN/PATH/build fallback)")
    parser.add_argument("--out", type=Path, help="new directory for the incident, actions and replay artifacts")
    args = parser.parse_args()
    if args.out:
        args.out.mkdir(parents=True, exist_ok=False)
        report = demonstrate(args.out, args.binary)
    else:
        with tempfile.TemporaryDirectory(prefix="probbit-agent-example-") as directory:
            report = demonstrate(Path(directory), args.binary)
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
