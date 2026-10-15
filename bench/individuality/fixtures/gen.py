"""Seeded generators for bench/individuality (Python 3.9, standard library only).

    python3 gen.py p0        # the P0 corpus scripts: fixtures/p0/scripts/*.jsonl
    python3 gen.py streams   # the sensed fixture's streams and probes: fixtures/streams/*.jsonl

Every output is a pure function of this file: the seeds are fixed below. The outputs are committed, and fixtures/MANIFEST
records their sha256, so a changed generator shows as a changed fixture.
"""
import hashlib
import json
import os
import random
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.dirname(HERE))
import common  # noqa: E402

# P0: the shipped personas (and two engine test fixtures) whose documents must not change. key -> path, repo-relative.
P0_PERSONAS = [
    ("tutor", "examples/persona/tutor.yaml"),
    ("ops-engineer", "examples/persona/ops-engineer.yaml"),
    ("trader-assistant", "examples/persona/trader-assistant.yaml"),
    ("adaptive", "examples/agent-harness/adaptive.json"),
    ("guarded", "examples/agent-harness/guarded.json"),
    ("scout", "bench/individuality/fixtures/p0/personas/scout.json"),
    ("drifter", "probbit-cli/tests/fixtures/persona/drives-adversary.json"),
    ("tutor-0.5.0", "probbit-cli/tests/fixtures/persona/tutor-0.5.0.yaml"),
]
# the same personas in their other shipped form (describe and check only: one digest, one individual)
P0_ALIASES = [
    ("tutor-json", "examples/persona/tutor.json"),
    ("ops-engineer-json", "examples/persona/ops-engineer.json"),
    ("trader-assistant-json", "examples/persona/trader-assistant.json"),
    ("demo-tutor", "probbit-cli/src/demo-tutor.yaml"),
]
P0_SCRIPTS = 4
P0_EVENTS = 100
# keys a persona without the new blocks must list in `ignored` exactly as 0.8 does
UNDECLARED = ["senses", "await", "for", "cancel", "control"]


def rng_for(*parts):
    h = hashlib.sha256("|".join(str(p) for p in parts).encode()).hexdigest()
    return random.Random(int(h[:16], 16))


def describe(path):
    code, out, err = common.run(["persona", "describe", path])
    if code:
        raise common.SetupError("describe %s: %s" % (path, err))
    return json.loads(out)


def number_max(path, iid):
    if path.endswith(".json"):
        doc = json.load(open(os.path.join(common.REPO, path), encoding="utf-8"))
        for i in doc.get("inputs", []):
            if i["id"] == iid:
                return i.get("max", 1)
    return 1


def drives_doc(path):
    if not path.endswith(".json"):
        return None
    return json.load(open(os.path.join(common.REPO, path), encoding="utf-8")).get("drives")


def reward_sources(path):
    if not path.endswith(".json"):
        return None
    return json.load(open(os.path.join(common.REPO, path), encoding="utf-8")).get("reward_from")


def senses_value(r):
    """A plausible readings object for a persona that has no senses block: it must stay an ignored key."""
    pick = r.random()
    if pick < 0.4:
        return {"tool_fail": r.random() < 0.3}
    if pick < 0.7:
        return {"tool_latency": {"v": round(r.uniform(0, 8), 3), "src": "env:runner"}, "tempo": r.randrange(0, 40)}
    return {"silence": round(r.uniform(0, 30), 2)}


def p0_event(r, info, path, k, turn):
    ev = {}
    for iid, spec in sorted(info["inputs"].items()):
        kind = spec["kind"]
        if kind == "flag":
            if r.random() < (0.45 if k == 2 else 0.25):
                ev[iid] = True
            elif r.random() < 0.1:
                ev[iid] = False
        elif kind == "level":
            if r.random() < 0.5:
                ev[iid] = r.choice(spec["levels"])
        elif kind == "number":
            if r.random() < 0.5:
                mx = number_max(path, iid)
                if k == 3:
                    ev[iid] = r.choice([0, mx])
                else:
                    ev[iid] = r.choice([0, mx, round(r.uniform(0, mx), 3), round(mx / 2.0, 3)])
    gaps = [0, 0, 0.1, 0.5, 1, 3, 10] if k != 3 else [0, 24, 720]
    if r.random() < 0.6:
        ev["elapsed_hours"] = r.choice(gaps)
    drives = drives_doc(path)
    if drives and r.random() < 0.6:
        goals = {}
        win_max = drives.get("expectation", {}).get("win_max", 2.0)
        for g in drives["goals"]:
            if r.random() < 0.4:
                s = {}
                c = r.random()
                if c < 0.25:
                    s["win"] = r.choice([0, 0.5, 1, win_max, round(r.uniform(0, win_max), 3)])
                elif c < 0.5:
                    s["progress"] = round(r.random(), 3)
                elif c < 0.65:
                    s["cue"] = r.random() < 0.7
                elif c < 0.8:
                    s["setback"] = round(r.random(), 3)
                elif c < 0.9:
                    s["novelty"] = r.choice([True, False, round(r.random(), 3)])
                else:
                    s["deadline_hours"] = r.choice([None, 0, 1, 12, 30, 100])
                goals[g["id"]] = s
        if goals:
            ev["goals"] = goals
    rf = reward_sources(path)
    if rf is not None:
        # a reward-bearing event needs an accepted source; script 1 also carries refused ones (live only)
        if k == 1 and r.random() < 0.2:
            ev["src"] = r.choice(["self", "clock", "human", "env:", "nonsense"])
        elif r.random() < 0.8:
            ev["src"] = r.choice(["env", "env:runner", "env:ci"])
    elif k == 1 and r.random() < 0.3:
        ev["src"] = r.choice(["env", "self", "human:a", "clock"])
    if k == 1:
        for key in UNDECLARED:
            if r.random() < 0.3:
                if key == "senses":
                    ev[key] = senses_value(r)
                elif key == "await":
                    ev[key] = r.random() < 0.8
                elif key == "for":
                    ev[key] = max(0, turn - r.randrange(1, 4))
                elif key == "cancel":
                    ev[key] = [max(0, turn - r.randrange(1, 6))]
                else:
                    ev[key] = r.choice(["pause", "resume"])
        if drives is None and r.random() < 0.15:
            ev["goals"] = {"fun": {"win": 1}}
    return ev


def gen_p0():
    out_dir = os.path.join(HERE, "p0", "scripts")
    os.makedirs(out_dir, exist_ok=True)
    total = 0
    for key, path in P0_PERSONAS:
        info = describe(path)
        for k in range(P0_SCRIPTS):
            r = rng_for("p0", key, k)
            events = [p0_event(r, info, path, k, t) for t in range(P0_EVENTS)]
            common.write_jsonl(os.path.join(out_dir, "%s-%d.jsonl" % (key, k)), events)
            total += len(events)
    print("p0 scripts: %d personas x %d scripts x %d events = %d events per seed" %
          (len(P0_PERSONAS), P0_SCRIPTS, P0_EVENTS, total))


if __name__ == "__main__":
    what = sys.argv[1] if len(sys.argv) > 1 else ""
    if what == "p0":
        gen_p0()
    elif what == "streams":
        import streams  # noqa: E402  (fixtures/streams.py: the sensed fixture's generators)
        streams.main()
    else:
        print(__doc__)
        sys.exit(2)
