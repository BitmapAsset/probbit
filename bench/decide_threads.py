# `pbit decide` (router sampler path) latency + CPU vs --threads at fixed work (300-task demo, 4 chains x 400 sweeps,
# polish 0); asserts identical odds + plan + gate across thread counts (the determinism contract), then default-path budget check.
import os, json, subprocess, statistics
BIN=os.environ.get('PBIT', 'target/release/pbit')
demo=subprocess.run([BIN,"demo","--tasks","300"],capture_output=True,text=True).stdout
print("machine: Apple M4 (4P+6E), 16 GB; 300-task demo, decide --sweeps 400 --chains 4 --polish-ms 0 (fixed work), 5 runs each, medians [IQR]")
ref=None
for t in [1,2,4]:
    ms=[]; cpu=[]; ups=[]
    for _ in range(5):
        d=json.loads(subprocess.run([BIN,"decide","--sweeps","400","--chains","4","--polish-ms","0","--threads",str(t)],input=demo,capture_output=True,text=True).stdout)
        ms.append(d['ms']); cpu.append(d['telemetry']['process_cpu_ms']); ups.append(d['telemetry']['site_updates_per_s'])
        key=json.dumps([d['odds'],d['plan'],d['gate'],d['verdict']],sort_keys=True); ref=ref or key; assert key==ref, "answer changed with --threads"
    q=statistics.quantiles(ms,n=4)
    print(f"--threads {t}: wall ms {statistics.median(ms):.1f} [{q[0]:.1f}-{q[2]:.1f}], CPU ms {statistics.median(cpu):.1f}, site updates/s {statistics.median(ups)/1e6:.2f} M; answer identical: yes; verdict {d['verdict']}")
ms=[]
for _ in range(5):
    d=json.loads(subprocess.run([BIN,"decide","--budget-ms","300"],input=demo,capture_output=True,text=True).stdout); ms.append(d['ms'])
q=statistics.quantiles(ms,n=4); print(f"default path (--budget-ms 300, 4 chains, threads {d['telemetry']['threads']}): wall ms {statistics.median(ms):.1f} [{q[0]:.1f}-{q[2]:.1f}], sweeps {d['telemetry']['sweeps']}, verdict {d['verdict']}")
