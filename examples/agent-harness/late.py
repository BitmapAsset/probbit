#!/usr/bin/env python3
"""Late, reordered outcomes credit the decision they were about. Local and dependency-free: no model or network calls."""
import argparse
import json
from pathlib import Path
import random
import subprocess
import sys
import tempfile

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent.parent / "python"))
import probbit
from run import gate


def late_outcomes(directory, binary=None, decisions=4, seed=7):
    """`addressed.json` awaits the outcome of each decision the host acts on. The outcomes come back late and in another order,
    each naming its decision (`for: {turn, stance}`), so each one credits the stance it was about; a duplicate is refused and
    changes nothing; `live verify --actions` compares the host's action log with the released levels."""
    binary = probbit.find_binary(binary)
    policy = HERE / "addressed.json"
    document = json.loads(policy.read_text(encoding="utf-8"))
    strand = directory / "late.strand"
    result, awaited, actions = None, [], []
    for _ in range(decisions):
        # The host acts on this decision now and will report its outcome later: the stance's turn and digest name it.
        event = {"await": True}
        result = probbit.live_event(document, None if result is None else result["state"], event,
                                    seed=0 if result is None else None, strand=strand, binary=binary)
        stance = result["stance"]
        decision, _ = gate(stance, "local_probe", 0)
        actions.append({"turn": stance["turn"], "action": {"action": "retry" if decision == "dispatch" else "ask"}})
        awaited.append({"turn": stance["turn"], "stance": stance["credit"]["stance"]})
    # Explicitly synthetic confirmations, not production outcomes: late and shuffled, each addressed to its decision.
    order = list(awaited)
    random.Random(seed).shuffle(order)
    paid = []
    for target in order:
        result = probbit.live_event(document, result["state"], {"praise": True, "for": target, "src": "env:synthetic"},
                                    strand=strand, binary=binary)
        record = result["stance"]["credit"]["paid"]
        paid.append({"turn": record["turn"], "e": record["e"], "stance_matches": record["stance"] == target["stance"]})
    before = strand.read_bytes()
    try:
        probbit.live_event(document, result["state"], {"praise": True, "for": order[0], "src": "env:synthetic"},
                           strand=strand, binary=binary)
        duplicate = "accepted"
    except probbit.ProbbitInputError as error:
        duplicate = error.path
    log = directory / "host-actions.jsonl"
    log.write_text("".join(json.dumps(a) + "\n" for a in actions), encoding="utf-8")
    verify = subprocess.run([binary, "live", "verify", str(strand), "--actions", str(log)], capture_output=True,
                            encoding="utf-8", timeout=30)
    summary = json.loads(verify.stdout)
    return {"decisions": len(awaited), "paid": paid, "open_after": result["stance"]["credit"]["open"],
            "duplicate_refused_at": duplicate, "strand_unchanged_by_refusal": strand.read_bytes() == before,
            "verify_exit": verify.returncode, "actions": summary.get("actions"), "verify_ok": summary.get("ok")}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", help="probbit executable (otherwise PROBBIT_BIN/PATH/build fallback)")
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="probbit-late-outcomes-") as directory:
        print(json.dumps(late_outcomes(Path(directory), args.binary), indent=2))


if __name__ == "__main__":
    main()
