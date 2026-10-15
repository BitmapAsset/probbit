"""Seeded generators of the lives metrics a, b, d and e run (Python 3.9, standard library only).

    python3 lives.py     # writes lives/*.jsonl (the outputs are committed and checked against MANIFEST)

Rooms A and B are one stream each, shared by every individual, so two lives of one individual differ only by the room and
two individuals in one room differ only by their genes. Rewards are unaddressed: each pays the stance just before it.
"""
import itertools
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.dirname(HERE))
sys.path.insert(0, HERE)
import common  # noqa: E402
import gen  # noqa: E402

LIVES = os.path.join(HERE, "lives")
ROOM_EVENTS = 60
STEP_HOURS = 0.5
CALM = {"tool_fail": False, "tool_latency": 0.2, "tempo": 1, "silence": 0.5}   # room A's medians
CALM_EVENTS = 8
GAP_HOURS = 720
PROBES = 20
HABITUATION_READINGS = 20
SURPRISE_READINGS = 40


def readings_event(senses, src, extra=None):
    ev = {"senses": senses, "src": src, "elapsed_hours": STEP_HOURS}
    if extra:
        # feedback comes from a person: the readings keep the instrument's source
        ev = {"senses": {k: {"v": v, "src": "env:host"} for k, v in senses.items()}, "src": extra[1],
              "elapsed_hours": STEP_HOURS, extra[0]: True}
    return ev


def room_a():
    r = gen.rng_for("lives", "room-a")
    out = []
    for k in range(ROOM_EVENTS):
        s = {"tool_fail": r.random() < 0.02, "tool_latency": round(r.uniform(0.1, 0.3), 3), "tempo": r.randint(0, 2),
             "silence": 0.5}
        out.append(readings_event(s, "env:host", ("praise", "human:a") if k % 6 == 5 else None))
    return out


def room_b():
    r = gen.rng_for("lives", "room-b")
    out = []
    for k in range(ROOM_EVENTS):
        s = {"tool_fail": r.random() < 0.3, "tool_latency": round(r.uniform(1, 8), 3), "tempo": r.randint(5, 30),
             "silence": 9 if k % 10 == 9 else 0.5}
        out.append(readings_event(s, "env:host", ("criticism", "human:b") if k % 7 == 6 else None))
    return out


def tail():
    """A month away with no readings, then 8 calm events at room A's medians: moods relax, history rolls out."""
    return [{"elapsed_hours": GAP_HOURS}] + [readings_event(dict(CALM), "env:host") for _ in range(CALM_EVENTS)]


def probes():
    grid = list(itertools.product([True, False], [0.2, 2, 8], [1, 10], [0.5, 30]))
    r = gen.rng_for("lives", "probes")
    pick = sorted(r.sample(range(len(grid)), PROBES))
    return [readings_event({"tool_fail": f, "tool_latency": lat, "tempo": t, "silence": s}, "env:host")
            for f, lat, t, s in (grid[i] for i in pick)]


def habituation():
    return [readings_event({"tool_fail": True}, "env:host") for _ in range(HABITUATION_READINGS)]


def surprise_quiet():
    return [readings_event({"tool_fail": False}, "env:host") for _ in range(SURPRISE_READINGS)] + \
        [readings_event({"tool_fail": True}, "env:host")]


def surprise_rate():
    """40 readings at rate 0.3 (failures at 0, 3 and 6 of every 10), then a failure."""
    return [readings_event({"tool_fail": k % 10 in (0, 3, 6)}, "env:host") for k in range(SURPRISE_READINGS)] + \
        [readings_event({"tool_fail": True}, "env:host")]


STREAMS = {
    "room-a": room_a, "room-b": room_b, "tail": tail, "probes": probes,
    "habituation": habituation, "surprise-quiet": surprise_quiet, "surprise-rate": surprise_rate,
}


def load(name):
    return common.read_jsonl(os.path.join(LIVES, name + ".jsonl"))


if __name__ == "__main__":
    os.makedirs(LIVES, exist_ok=True)
    for name, f in STREAMS.items():
        events = f()
        common.write_jsonl(os.path.join(LIVES, name + ".jsonl"), events)
        print("%-16s %3d events" % (name, len(events)))
