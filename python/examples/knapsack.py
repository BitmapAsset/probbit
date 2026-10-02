"""Knapsack: 20 items, one weighted budget (probbit-ir `linear`); exact odds that each item is in the knapsack."""
import json, os, sys
HERE = os.path.dirname(os.path.abspath(__file__)); sys.path.insert(0, os.path.join(HERE, ".."))
import probbit

with open(os.path.join(HERE, "..", "..", "examples", "knapsack-20.json")) as f:
    prog = json.load(f)
a = probbit.exact(prog, exact_ms=2000)
print(a["verdict"], a.get("tier"), "feasible plans", a.get("n_feasible"))
print("in:", [i for i, v in a["plan"].items() if v == "in"])
