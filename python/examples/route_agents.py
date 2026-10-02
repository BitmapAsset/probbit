"""Route 24 agent tasks to 6 workers under hard quotas (router document; `probbit decide`)."""
import os, sys
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
import probbit

a = probbit.decide(probbit.demo(tasks=24, seed=1), budget_ms=200)
print(a["verdict"], "released", len(a["released"]), "of", a["tasks"], "tasks")
for task, worker in list(a["plan"].items())[:5]:
    print(f"  {task} -> {worker}  odds {a['odds'][task][worker]:.3f}")
