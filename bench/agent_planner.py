#!/usr/bin/env python3
"""R19.7 (P2.2 family 6): an agentic tool-call planner (the AI demo). One variable per step over the actions
{search, read, code, test, ask, stop}; unaries = a policy's per-step scores; `tables` pairs = transition preferences between
consecutive steps; hard rules: at most 2 search and at most 1 ask (value caps), stop is absorbing and test needs code or test
just before it (`implies`), and a token budget (one `linear` rule: search 3, read 2, code 5, test 4, ask 1, stop 0 <= budget).
Oracle: brute force over 6^T by the rules' direct meaning. Baseline: greedy step by step (best feasible next action).
Stdlib only. Usage: python3 bench/agent_planner.py [--steps 6,7] [--instances 2] [--seeds 4] [--write-example examples/agent-plan-6.json]"""
import argparse, itertools, json, math, os, random, subprocess, time
PROBBIT = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'target', 'release', 'probbit')
A = ["search", "read", "code", "test", "ask", "stop"]; COST = {"search": 3, "read": 2, "code": 5, "test": 4, "ask": 1, "stop": 0}
PREF = {("search", "read"): 0.8, ("read", "code"): 0.6, ("code", "test"): 0.9, ("test", "stop"): 0.7, ("test", "code"): 0.3, ("ask", "code"): 0.4}
def instance(T, seed):
    r = random.Random(seed)
    h = [{a: round(r.uniform(-0.5, 0.5) + (0.6 if (a in ("search", "read") and t < T / 3) or (a in ("code", "test") and t >= T / 3) else 0.0), 4) for a in A} for t in range(T)]
    tab = [[round(PREF.get((a, b), 0.0) + (-0.4 if a == b and a != "stop" else 0.0) + r.uniform(-0.1, 0.1), 4) for b in A] for a in A]
    return h, tab, 3 * T
def program(T, h, tab, budget):
    s = lambda t: f"s{t}"
    return {"probbit_ir": 1, "comment": f"tool-call plan over {T} steps", "values": A,
        "vars": [{"id": s(t), "h": h[t], **({"forbid": ["test"]} if t == 0 else {})} for t in range(T)],
        "pairs": [{"i": s(t), "j": s(t + 1), "table": tab} for t in range(T - 1)],
        "caps": [{"value": "search", "limit": 2}, {"value": "ask", "limit": 1}],
        "implies": [{"if": {"var": s(t), "value": "stop"}, "then": {"var": s(t + 1), "in": ["stop"]}} for t in range(T - 1)]
                 + [{"if": {"var": s(t), "value": "test"}, "then": {"var": s(t - 1), "in": ["code", "test"]}} for t in range(1, T)],
        "linear": [{"terms": [[s(t), a, COST[a]] for t in range(T) for a in A if COST[a] > 0], "limit": budget}]}
def ok(x, budget):
    if x[0] == "test" or x.count("search") > 2 or x.count("ask") > 1 or sum(COST[a] for a in x) > budget: return False
    for t in range(len(x) - 1):
        if x[t] == "stop" and x[t + 1] != "stop": return False
        if x[t + 1] == "test" and x[t] not in ("code", "test"): return False
    return True
def lw(x, h, tab): return sum(h[t][a] for t, a in enumerate(x)) + sum(tab[A.index(x[t])][A.index(x[t + 1])] for t in range(len(x) - 1))
def brute(T, h, tab, budget):
    z = 0.0; m = [{a: 0.0 for a in A} for _ in range(T)]; best = (-1e9, None); n = 0
    for x in itertools.product(A, repeat=T):
        if not ok(x, budget): continue
        n += 1; v = lw(x, h, tab); e = math.exp(v); z += e
        for t, a in enumerate(x): m[t][a] += e
        if v > best[0]: best = (v, x)
    return n, math.log(z), [{a: m[t][a] / z for a in A} for t in range(T)], best
def greedy(T, h, tab, budget):
    x = []
    for t in range(T):
        cands = [a for a in A if ok(tuple(x + [a]) + ("stop",) * (T - t - 1), budget)]
        if not cands: return None
        x.append(max(cands, key=lambda a: h[t][a] + (tab[A.index(x[-1])][A.index(a)] if x else 0.0)))
    return lw(x, h, tab)
def run(prog, *args):
    t0 = time.time(); p = subprocess.run([PROBBIT, 'run', *args], input=json.dumps(prog), capture_output=True, text=True)
    return p.returncode, json.loads(p.stdout), (time.time() - t0) * 1e3
def main():
    ap = argparse.ArgumentParser(); ap.add_argument('--steps', default='6,7'); ap.add_argument('--instances', type=int, default=2)
    ap.add_argument('--seeds', type=int, default=4); ap.add_argument('--write-example', default=None); a = ap.parse_args()
    commit = subprocess.run(['git', 'rev-parse', '--short', 'HEAD'], capture_output=True, text=True, cwd=os.path.dirname(PROBBIT)).stdout.strip()
    print(json.dumps({"commit": commit, "load_before": os.getloadavg(), "steps": a.steps, "instances": a.instances, "seeds": a.seeds}))
    print("| T | inst | feasible plans | optimum log w | greedy (gap) | default: verdict, tier, n_feasible, max \\|odds - brute\\|, plan = optimum?, ms | sampler verdicts | released | max \\|odds - brute\\| released | median ms |")
    print("|---|---|---|---|---|---|---|---|---|---|")
    tot = {"answers": 0, "released": 0, "wrong": 0, "false": 0}
    for T in map(int, a.steps.split(',')):
        for inst in range(1, a.instances + 1):
            h, tab, budget = instance(T, 13 * T + inst); prog = program(T, h, tab, budget)
            if a.write_example and T == 6 and inst == 1: json.dump(prog, open(a.write_example, 'w'), indent=1)
            n, logz, mg, best = brute(T, h, tab, budget); g = greedy(T, h, tab, budget)
            od = lambda e, v: max(abs(e['marginals'][v].get(x, 0.0) - mg[int(v[1:])][x]) for x in A)
            c, d, ms = run(prog); dm = max(od(d, f"s{t}") for t in range(T)); same = [d['plan'][f"s{t}"] for t in range(T)] == list(best[1])
            vs, rel, worst, times = [], 0, 0.0, []
            for s in range(1, a.seeds + 1):
                c2, e, ms2 = run(prog, '--op', 'sample', '--seed', str(s)); vs.append(e['verdict']); times.append(ms2); tot["answers"] += 1
                released = e.get('released', []); rel += len(released); tot["released"] += len(released)
                err = max([od(e, v) for v in released] or [0.0]); worst = max(worst, err); tot["wrong"] += sum(od(e, v) > 0.05 for v in released)
                if e['verdict'] == 'diagnostics_passed' and err > 0.05: tot["false"] += 1
            vstr = ', '.join(f"{v} {vs.count(v)}" for v in sorted(set(vs))); gs = f"{g:.4f} ({best[0] - g:+.4f})" if g is not None else "stuck"
            print(f"| {T} | {inst} | {n} | {best[0]:.4f} | {gs} | {d.get('verdict')}, {d.get('tier')}, {d.get('n_feasible')}, {dm:.1e}, {same}, {ms:.1f} | {vstr} | {rel}/{T * a.seeds} | {worst:.4f} | {sorted(times)[len(times) // 2]:.0f} |")
    print(json.dumps({"totals": tot, "load_after": os.getloadavg()}))
if __name__ == '__main__': main()
