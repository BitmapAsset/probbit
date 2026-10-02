# BENCHMARKS §2: `probbit decide` end-to-end latency at defaults (budget 200 ms, polish 50 ms, exact tiers on), re-running an earlier
# measurement. Wall ms measured around the whole subprocess (process start + JSON parse + decide + print); `ms` = probbit's own
# figure. N = 20 per queue, run alone. p50 / p95 by nearest rank.
import os, json, subprocess, statistics, time
BIN=os.environ.get('PROBBIT', 'target/release/probbit'); N=int(os.environ.get('N', '20'))
def pct(v,p): s=sorted(v); return s[min(len(s)-1, int(round((len(s)-1)*p)))]
print(f"machine: Apple M4 (4P+6E), 16 GB; N = {N}; columns: queue | verdicts | probbit ms p50 / p95 | wall ms p50 / p95 | min / max wall")
for args in (["--tasks","12"],["--tasks","24"],["--tasks","30"],["--tasks","300"],["--tasks","300","--hard"]):
    demo=subprocess.run([BIN,"demo"]+args,capture_output=True,text=True).stdout
    own=[]; wall=[]; ver={}
    for _ in range(N):
        t0=time.perf_counter(); out=subprocess.run([BIN,"decide"],input=demo,capture_output=True,text=True).stdout; wall.append((time.perf_counter()-t0)*1e3)
        d=json.loads(out); own.append(d['ms']); k=d['verdict']+("/"+d['tier'] if d.get('tier') else ""); ver[k]=ver.get(k,0)+1
    print(f"{' '.join(args)} | {ver} | {pct(own,.5):.1f} / {pct(own,.95):.1f} | {pct(wall,.5):.1f} / {pct(wall,.95):.1f} | {min(wall):.1f} / {max(wall):.1f}")
