# BENCHMARKS §6: does the exact tier's gap budget make `probbit run --op decide` give up on CSPs that
# `--op exact` answers? Random 3-colourings G(n, m) near the threshold, one edge clamped. `python3 bench/csp_gap_probe.py`
# (NS=100,150,200 DS=4.3,4.6 SEEDS=3 ALARM=6 BUDGET=200 (ms, --budget-ms of both ops); PROBBIT=path overrides the binary; stdlib only).
import json, random, subprocess, sys, time, os
BIN = os.environ.get('PROBBIT', 'target/release/probbit')
def prog(n, d, seed):
    r = random.Random(seed); m = int(round(n * d / 2)); E = set()
    while len(E) < m:
        a, b = r.sample(range(n), 2); E.add((min(a, b), max(a, b)))
    E = sorted(E); cols = ['r', 'g', 'b']
    vars_ = [{"id": f"v{i}", "h": {c: round(r.uniform(0, 0.3), 3) for c in cols}} for i in range(n)]
    a, b = E[0]; vars_[a]["clamp"] = "r"; vars_[b]["clamp"] = "g"
    caps = [{"limit": 1, "members": [[f"v{a}", c], [f"v{b}", c]]} for (a, b) in E for c in cols]
    return {"probbit_ir": 1, "values": cols, "vars": vars_, "caps": caps}
def run(p, op, alarm):
    t = time.time()
    cp = subprocess.run(['perl', '-e', f'alarm {alarm}; exec @ARGV', BIN, 'run', '--op', op, '--budget-ms', os.environ.get('BUDGET', '200'), '--polish-ms', '0'],
                        input=json.dumps(p), capture_output=True, text=True)
    w = time.time() - t
    if cp.returncode == 142 or not cp.stdout.strip(): return {"verdict": f"alarm/exit{cp.returncode}", "wall": w}
    d = json.loads(cp.stdout); return {"verdict": d.get("verdict"), "tier": d.get("tier"), "n": d.get("n_feasible"), "rel": len(d.get("released", [])), "wall": w}
if __name__ == '__main__':
    ns = [int(x) for x in os.environ.get('NS', '50,70').split(',')]; ds = [float(x) for x in os.environ.get('DS', '4.2,4.6').split(',')]
    seeds = int(os.environ.get('SEEDS', '4')); alarm = int(os.environ.get('ALARM', '10'))
    print("n,d,seed,decide_verdict,decide_tier,decide_n,decide_released,decide_wall_s,exact_verdict,exact_n,exact_wall_s", flush=True)
    for n in ns:
        for d in ds:
            for s in range(seeds):
                p = prog(n, d, 1000 * n + int(10 * d) * 10 + s)
                a = run(p, 'decide', alarm); b = run(p, 'exact', alarm)
                print(f"{n},{d},{s},{a['verdict']},{a.get('tier')},{a.get('n')},{a.get('rel')},{a['wall']:.3f},{b['verdict']},{b.get('n')},{b['wall']:.3f}", flush=True)
