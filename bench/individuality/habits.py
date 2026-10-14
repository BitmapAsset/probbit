"""Metric e: zero habit breaks (Python 3.9, standard library only; post-processing only).

Every stance of metrics a, b and d is checked twice: the engine's own counter (`habits.violations`) and an independent check
here: each habit's `when` evaluated on the logged inputs, with `sense.<id>.raw` and `.z` read from the stance's `senses`,
against the released levels. A zero is not vacuous: the stances where a habit bound (`habits.bound`) are counted.
"""
import json
import os

import common

# the habits of fixtures/sensed.yaml (and of its no-genes copy), as data
HABITS = [
    {"id": "no_bold_after_failure", "when": {"sense.tool_fail.raw": True}, "then": ("caution", "not", ["bold"])},
    {"id": "short_when_busy", "when": {"sense.tempo.raw": {"at_least": 20}}, "then": ("verbosity", "at_most", "short")},
]
LEVELS = {"caution": ["bold", "measured", "careful"], "verbosity": ["terse", "short", "full"]}


def value(doc, key):
    if key.startswith("sense."):
        _, sid, field = key.split(".")
        r = doc.get("senses", {}).get(sid)
        return None if r is None else r[field]
    return doc.get("inputs", {}).get(key)


def holds(cond, v):
    if v is None:
        return False  # a condition on a sense not read this event is false
    if isinstance(cond, dict):
        return all((k == "at_least" and v >= x) or (k == "at_most" and v <= x) for k, x in cond.items())
    return v == cond


def broken(doc, habit):
    if not all(holds(c, value(doc, k)) for k, c in habit["when"].items()):
        return False
    trait, op, arg = habit["then"]
    st = doc["stance"].get(trait)
    if not st or not st.get("released"):
        return False
    lv = LEVELS[trait]
    level = st["level"]
    if op == "not":
        return level in arg
    return lv.index(level) > lv.index(arg)


def metric(stances, bars):
    stances = [d for d in stances if d and "stance" in d]
    viol = sum(1 for d in stances if d["habits"]["violations"] > 0)
    breaks = sum(1 for d in stances for h in HABITS if broken(d, h))
    bound = sum(1 for d in stances if d["habits"]["bound"])
    active = sum(1 for d in stances if d["habits"]["active"])
    r = {"scope": "every stance of metrics a, b and d", "stances": len(stances), "violations": viol, "breaks": breaks,
         "bound": bound, "in_force": active}
    r["pass"] = {"e": viol <= bars["e"]["violations_max"] and breaks <= bars["e"]["breaks_max"] and bound >= bars["e"]["bound_min"]}
    return r


if __name__ == "__main__":
    print(json.dumps(HABITS))
    print(os.path.basename(common.FIXTURES))
