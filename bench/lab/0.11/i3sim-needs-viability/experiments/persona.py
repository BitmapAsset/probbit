"""The I3-sim persona: a probbit format-1 document built only from shipped blocks (docs/persona.md §2.1-2.12).

Three needs (budget, integrity, recovery) whose deficits the engine integrates from logged readings and amounts;
moods `valence` (drives.valence + senses) and `pressure` (a persona-declared mood: no engine pressure field ships).
The deficit -> pressure path is the only one the shipped engine offers: habits on need.<id>.deficit (§2.12 l.639).
"""
import json

NEEDS = ["budget", "integrity", "recovery"]
GOAL_OF = {"budget": "grant", "integrity": "repair", "recovery": "rest"}


def build(cfg):
    th_mid = cfg["habit_mid"]    # per need: deficit at which pressure is at least mid
    th_high = cfg["habit_high"]  # per need: deficit at which pressure is high
    habits = []
    for n in NEEDS:
        habits.append({"id": f"{n}_mid", "when": {f"need.{n}.deficit": {"at_least": round(th_mid[n], 6)}},
                       "then": {"pressure": {"at_least": "mid"}}, "say": f"{n} deficit"})
        habits.append({"id": f"{n}_high", "when": {f"need.{n}.deficit": {"at_least": round(th_high[n], 6)}},
                       "then": {"pressure": ["high"]}, "say": f"{n} deficit near its bound"})
    p = {
        "probbit_persona": 1,
        "identity": {"name": "Vigil", "version": "1.0.0", "seed": 1,
                     "summary": "I3-sim host individual: three needs over viability bands (simulation)"},
        "traits": [{"id": "caution", "levels": ["bold", "measured", "careful"], "logw": [0.2, 0.5, 0.1]}],
        "moods": [
            {"id": "valence", "levels": ["down", "even", "up"], "inertia": 0.61, "half_life_hours": 6},
            {"id": "pressure", "levels": ["low", "mid", "high"], "logw": [0.6, 0.0, -0.6],
             "inertia": cfg["pressure_inertia"], "half_life_hours": 1},
        ],
        "inputs": [{"id": "time_pressure", "kind": "number",
                    "effects": {"pressure": cfg["tp_gain"], "caution": -0.3}}],
        "senses": [
            {"id": "tool_fail", "kind": "flag", "unit": "event", "src": ["env"],
             "prior": {"mean": 0.05, "var": 0.05}, "drift_cap": 0.25, "say": "tool failures",
             "effects": {"caution": 0.8, "valence": -0.5, "pressure": cfg["fail_gain"]}},
            {"id": "spend", "kind": "number", "unit": "budget_share", "range": [0, 0.2], "src": ["env"],
             "prior": {"mean": 0.02, "var": 0.0001}, "drift_cap": 0.05, "say": "budget spent this step",
             "effects": {"pressure": cfg["spend_gain"]}},
            {"id": "damage", "kind": "number", "unit": "integrity_share", "range": [0, 0.3], "src": ["env"],
             "prior": {"mean": 0.005, "var": 0.0004}, "drift_cap": 0.05, "say": "integrity lost this step",
             "effects": {"valence": -0.3}},
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
            "items": [
                {"id": "budget", "say": "compute budget", "unit": "budget_share", "cap": 1, "weight": 1,
                 "per_hour": 0, "per_reading": {"sense": "spend", "rate": 1}, "satisfy_from": ["env"],
                 "amount": 0.3, "bind": "grant"},
                {"id": "integrity", "say": "integrity", "unit": "integrity_share", "cap": 1, "weight": 1,
                 "per_hour": 0, "per_reading": {"sense": "damage", "rate": 1}, "satisfy_from": ["env"],
                 "amount": 0.08, "bind": "repair"},
                {"id": "recovery", "say": "recovery", "unit": "recovery_share", "cap": 1, "weight": 1,
                 "per_hour": 0, "per_reading": {"sense": "strain", "rate": 1}, "satisfy_from": ["env"],
                 "amount": 0.04, "bind": "rest"},
            ],
        },
        "habits": habits,
        "engine": {"op": "decide", "twin": False},
    }
    return p


def write(cfg, path):
    with open(path, "w") as f:
        json.dump(build(cfg), f, indent=1, sort_keys=False)
        f.write("\n")
