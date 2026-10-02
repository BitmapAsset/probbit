"""Agentic tool-call planner (tool x step with precedence, budget, caps), with a whole-call deadline."""
import json, os, sys
HERE = os.path.dirname(os.path.abspath(__file__)); sys.path.insert(0, os.path.join(HERE, ".."))
import probbit

with open(os.path.join(HERE, "..", "..", "examples", "agent-plan-6.json")) as f:
    prog = json.load(f)
try:
    a = probbit.run(prog, deadline_ms=1000)
except probbit.ProbbitInputError as e:
    sys.exit(f"bad program: {e}")
print(a["verdict"], a.get("tier"), "deadline met:", a["deadline"]["met"])
print(" -> ".join(f"{s}:{t}" for s, t in a["plan"].items()))
