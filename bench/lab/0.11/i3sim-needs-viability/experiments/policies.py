"""Policies: (stance, world observation) -> action.

A  affect-coupled: the stance's pressure gates need-serving; pursue and needs.*.deficit choose which need; caution picks
   the work variant. The engine has no viability band (RFC 0008 gap), so the host config supplies band widths (1 - lo)
   to rank deficits; the policy never reads a resource level.
B  the same function fed a constant resting stance (no state).
C  reactive heuristic reading the world's resource levels directly (comparator; no engine).
D  uniform random.
"""
from world import ACTIONS, RES, SERVES

LEVEL_IDX = {"low": 0, "mid": 1, "high": 2, "down": 0, "even": 1, "up": 2, "bold": 0, "measured": 1, "careful": 2}
GOAL_ACTION = {"grant": "request_grant", "repair": "inspect", "rest": "rest"}


def view(doc):
    """the stance fields policy A reads (all named in LOOP.md §1)"""
    return {
        "pressure": doc["mood"]["pressure"]["level"],
        "valence": doc["mood"]["valence"]["level"],
        "caution": doc["stance"]["caution"]["level"],
        "pursue": doc["pursue"]["goal"],
        "deficit": {r: doc["needs"][r]["deficit"] for r in RES},
    }


def policy_affect(sv, obs, pcfg, width):
    if LEVEL_IDX[sv["pressure"]] >= pcfg["gate_level"]:
        if pcfg["use_pursue"] and sv["pursue"] in GOAL_ACTION:
            return GOAL_ACTION[sv["pursue"]]
        worst = max(RES, key=lambda r: (sv["deficit"][r] / width[r], -RES.index(r)))
        if sv["deficit"][worst] > 0:
            return SERVES[worst]
    if sv["caution"] == "bold" and pcfg["risky_when_bold"]:
        return "work_risky"
    return "work_safe"


def policy_reactive(world, obs, ccfg):
    d = world.dist()
    worst = min(RES, key=lambda r: (d[r], RES.index(r)))
    if d[worst] < ccfg["threshold"]:
        return SERVES[worst]
    return "work_risky" if d["integrity"] > ccfg["risky_above"] else "work_safe"


def policy_random(rng):
    return ACTIONS[rng.randrange(len(ACTIONS))]


def policy_deficit_only(sv, obs, pcfg, width, gate=0.5):
    """ablation A0: the engine's deficits only, gated by a host threshold (the habits' mid rule); no mood is read"""
    worst = max(RES, key=lambda r: (sv["deficit"][r] / width[r], -RES.index(r)))
    if sv["deficit"][worst] / width[worst] >= gate:
        return SERVES[worst]
    return "work_safe"
