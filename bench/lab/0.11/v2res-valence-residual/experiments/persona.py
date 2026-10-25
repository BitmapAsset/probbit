"""The I3-sim persona: a probbit format-1 document built only from shipped blocks (docs/persona.md §2.1-2.12).

Three needs (budget, integrity, recovery) whose deficits the engine integrates from logged readings and amounts;
moods `valence` (drives.valence + senses) and `pressure` (a persona-declared mood: no engine pressure field ships).
V0.11 v-run: the RFC 0008 keys at the fixed values (needs.pressure gain 2.0 linear, needs.valence tonic 0.8 phasic 0.6,
viable.at_most = 1 - lo per resource, viable.events 4) and no habit on need.<id>.deficit. `tonic` is a parameter only so
that arm A-t0 can set it to 0.
V2-res (2026-10-21): phasic, drives_valence, fail_valence, damage_valence and valence_inertia are keyword knobs whose
defaults are the shipped values, so arms A and A-t0 build the v-run's documents byte for byte.
"""
import json

NEEDS = ["budget", "integrity", "recovery"]
GOAL_OF = {"budget": "grant", "integrity": "repair", "recovery": "rest"}


VIABLE = {"budget": 0.85, "integrity": 0.75, "recovery": 0.8}   # 1 - lo (0.15 / 0.25 / 0.20), fixed before the run


def build(cfg, tonic=0.8, phasic=0.6, drives_valence=True, fail_valence=-0.5, damage_valence=-0.3, valence_inertia=0.61):
    p = {
        "probbit_persona": 1,
        "identity": {"name": "Vigil", "version": "1.0.0", "seed": 1,
                     "summary": "I3-sim host individual: three needs over viability bands (simulation)"},
        "traits": [{"id": "caution", "levels": ["bold", "measured", "careful"], "logw": [0.2, 0.5, 0.1]}],
        "moods": [
            {"id": "valence", "levels": ["down", "even", "up"], "inertia": valence_inertia, "half_life_hours": 6},
            {"id": "pressure", "levels": ["low", "mid", "high"], "logw": [0.6, 0.0, -0.6],
             "inertia": cfg["pressure_inertia"], "half_life_hours": 1},
        ],
        "inputs": [{"id": "time_pressure", "kind": "number",
                    "effects": {"pressure": cfg["tp_gain"], "caution": -0.3}}],
        "senses": [
            {"id": "tool_fail", "kind": "flag", "unit": "event", "src": ["env"],
             "prior": {"mean": 0.05, "var": 0.05}, "drift_cap": 0.25, "say": "tool failures",
             "effects": {"caution": 0.8, "valence": fail_valence, "pressure": cfg["fail_gain"]}},
            {"id": "spend", "kind": "number", "unit": "budget_share", "range": [0, 0.2], "src": ["env"],
             "prior": {"mean": 0.02, "var": 0.0001}, "drift_cap": 0.05, "say": "budget spent this step",
             "effects": {"pressure": cfg["spend_gain"]}},
            {"id": "damage", "kind": "number", "unit": "integrity_share", "range": [0, 0.3], "src": ["env"],
             "prior": {"mean": 0.005, "var": 0.0004}, "drift_cap": 0.05, "say": "integrity lost this step",
             "effects": {"valence": damage_valence}},
            {"id": "strain", "kind": "number", "unit": "recovery_share", "range": [0, 0.2], "src": ["env"],
             "prior": {"mean": 0.01, "var": 0.0001}, "drift_cap": 0.05, "say": "recovery drained this step",
             "effects": {"pressure": cfg["strain_gain"]}},
        ],
        "drives": {
            "goals": [
                {"id": "work", "interest": 1.0, "say": "do the task"},
                {"id": "grant", "interest": 0.4, "expect_by_hours": 2, "say": "ask for budget"},
                {"id": "repair", "interest": 0.4, "expect_by_hours": 2, "say": "inspect and repair"},
                {"id": "rest", "interest": 0.4, "expect_by_hours": 2, "say": "rest"},
            ],
            "pursue": {"glow": 0},
            "valence": {"mood": "valence", "expect": 0.2, "surprise": 0.6},
        },
        "needs": {
            "drive": {"n": 4, "m": 3},
            "pursue": {"want": cfg["need_want"], "cap": 3},
            "success": {"rate": 0.2, "init": 1},
            "r_cap": 1,
            "pressure": {"mood": "pressure", "gain": 2.0, "shape": "linear"},
            "valence": {"mood": "valence", "tonic": tonic, "phasic": phasic},
            "items": [
                {"id": "budget", "say": "compute budget", "unit": "budget_share", "cap": 1, "weight": 1,
                 "per_hour": 0, "per_reading": {"sense": "spend", "rate": 1}, "satisfy_from": ["env"],
                 "amount": 0.3, "bind": "grant", "viable": {"at_most": VIABLE["budget"], "events": 4}},
                {"id": "integrity", "say": "integrity", "unit": "integrity_share", "cap": 1, "weight": 1,
                 "per_hour": 0, "per_reading": {"sense": "damage", "rate": 1}, "satisfy_from": ["env"],
                 "amount": 0.08, "bind": "repair", "viable": {"at_most": VIABLE["integrity"], "events": 4}},
                {"id": "recovery", "say": "recovery", "unit": "recovery_share", "cap": 1, "weight": 1,
                 "per_hour": 0, "per_reading": {"sense": "strain", "rate": 1}, "satisfy_from": ["env"],
                 "amount": 0.04, "bind": "rest", "viable": {"at_most": VIABLE["recovery"], "events": 4}},
            ],
        },
        "engine": {"op": "decide", "twin": False},
    }
    if not drives_valence:   # V2-res arm knob: omit the drives.valence block entirely (nothing else in the document moves)
        del p["drives"]["valence"]
    return p


def write(cfg, path, tonic=0.8, **knobs):
    with open(path, "w") as f:
        json.dump(build(cfg, tonic, **knobs), f, indent=1, sort_keys=False)
        f.write("\n")
