# BENCHMARKS §4.3: pbit's plan on a scheduling program vs an ILP solver's proven optimum. Needs scipy >= 1.9 (HiGHS
# MILP), e.g. `/opt/homebrew/bin/python3 bench/schedule_ilp.py prog.json [...]` (a benchmark dependency, NOT a pbit dependency).
# Works for any pbit-ir JSON v1 program WITHOUT pairs (unary "h" + caps), e.g. schedule_oracle's
# `PROBE_START=200 PSEED=s JSON_OUT=p.json cargo run --release -p pbit-ir --example schedule_oracle` (200 jobs x 40 slots,
# cap 10 per slot, precedence as pair caps). ILP: max sum h[i,v] x[i,v]; each var exactly one allowed value; every cap
# sum(members) <= limit. pbit = `pbit run --op decide --budget-ms B` (defaults otherwise), BUDGETS=1000,5000 (ms), N runs each.
import os, sys, json, subprocess, statistics, time
import numpy as np
from scipy.optimize import milp, LinearConstraint, Bounds
from scipy.sparse import coo_matrix
BIN = os.environ.get('PBIT', 'target/release/pbit'); N = int(os.environ.get('N', '3'))
BUDGETS = [int(b) for b in os.environ.get('BUDGETS', '1000,5000').split(',')]; TL = float(os.environ.get('TIME_LIMIT', '120'))
def ilp(p):
    vals = p['values']; idx = {}; c = []
    for v in p['vars']:
        al = v.get('allowed') or vals; h = v.get('h') or {}
        for s in al: idx[(v['id'], s)] = len(c); c.append(-float(h.get(s, 0.0)))
    rows, cols, lo, hi = [], [], [], []; r = 0
    for v in p['vars']:
        for s in (v.get('allowed') or vals): rows.append(r); cols.append(idx[(v['id'], s)])
        lo.append(1); hi.append(1); r += 1
    for cp in p['caps']:
        ms = [idx[(a, b)] for a, b in cp['members'] if (a, b) in idx]
        if len(ms) <= cp['limit']: continue  # cannot bind
        rows += [r] * len(ms); cols += ms; lo.append(-np.inf); hi.append(cp['limit']); r += 1
    M = coo_matrix((np.ones(len(rows)), (rows, cols)), shape=(r, len(c))).tocsr()
    t0 = time.perf_counter()
    res = milp(np.array(c), constraints=LinearConstraint(M, np.array(lo), np.array(hi)), integrality=np.ones(len(c)), bounds=Bounds(0, 1), options={'time_limit': TL})
    return (-res.fun if res.x is not None else float('nan')), (time.perf_counter() - t0) * 1e3, res.status, len(c), r, getattr(res, 'mip_gap', None)
if __name__ == '__main__':
    print(f"machine: {os.uname().machine} {os.uname().sysname}; pbit={BIN}; N={N}; budgets={BUDGETS} ms; HiGHS time limit {TL} s", flush=True)
    for path in sys.argv[1:]:
        p = json.load(open(path)); src = json.dumps(p)
        opt, ms, st, nv, nr, gap = ilp(p)
        print(f"{os.path.basename(path)}: {len(p['vars'])} vars, {len(p['caps'])} caps | ILP optimum {opt:.3f} in {ms:.0f} ms, status {st} (0 = proven optimal), mip_gap {gap}, {nv} binaries x {nr} rows", flush=True)
        for b in BUDGETS:
            out = []
            for _ in range(N):
                t0 = time.perf_counter(); cp = subprocess.run([BIN, 'run', '--op', 'decide', '--budget-ms', str(b)], input=src, capture_output=True, text=True); w = (time.perf_counter() - t0) * 1e3
                d = json.loads(cp.stdout); out.append((d['verdict'], d.get('plan_logw'), len(d.get('released', [])), w, d.get('violations')))
            lws = [o[1] for o in out if o[1] is not None]
            print(f"  pbit --budget-ms {b}: verdicts {[o[0] for o in out]}, released {[o[2] for o in out]}, violations {[o[4] for o in out]}, plan log w median {statistics.median(lws) if lws else float('nan'):.3f} "
                  f"[{min(lws) if lws else float('nan'):.3f}-{max(lws) if lws else float('nan'):.3f}], gap to ILP median {opt - statistics.median(lws) if lws else float('nan'):.3f} nats, wall median {statistics.median([o[3] for o in out]):.0f} ms", flush=True)
