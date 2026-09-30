# Does the plan polish honour --threads? A/B of two binaries (OLD = before the polish honoured --threads, NEW = after) on the 300-task demo,
# `pbit decide --sweeps 400 --polish-ms 50 --threads T`, N = 5 alternating; wall ms, process CPU ms (getrusage), plan log w; medians.
import os, json, subprocess, statistics
OLD=os.environ['OLD']; NEW=os.environ.get('NEW', 'target/release/pbit'); N=int(os.environ.get('N', '5'))
demo=subprocess.run([NEW,"demo","--tasks","300"],capture_output=True,text=True).stdout
print(f"machine: Apple M4, 16 GB; N = {N} alternating; columns: threads | binary | wall ms | process CPU ms | CPU/wall | plan log w (min-max)")
for t in ("1","4"):
    r={OLD:[],NEW:[]}
    for _ in range(N):
        for b in (OLD,NEW):
            d=json.loads(subprocess.run([b,"decide","--sweeps","400","--polish-ms","50","--threads",t],input=demo,capture_output=True,text=True).stdout)
            r[b].append((d['ms'], d['telemetry']['process_cpu_ms'], d['plan_logw']))
    for b,name in ((OLD,"old"),(NEW,"new")):
        w=statistics.median(x[0] for x in r[b]); c=statistics.median(x[1] for x in r[b]); lw=[x[2] for x in r[b]]
        print(f"{t} | {name} | {w:.1f} | {c:.1f} | {c/w:.2f} | {statistics.median(lw):.2f} ({min(lw):.2f}-{max(lw):.2f})")
