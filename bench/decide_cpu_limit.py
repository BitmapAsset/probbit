# --cpu-limit on the router path (probbit decide): 300-task demo, --sweeps 1000 --chains 4 --threads 4 --polish-ms 0, N = 3 per level.
# utilisation = process CPU ms / (sample_ms x threads) (sampler phase only; the exact tiers' decline is outside the duty cycle).
import os, json, subprocess, statistics
BIN=os.environ.get('PROBBIT', 'target/release/probbit')
demo=subprocess.run([BIN,"demo","--tasks","300"],capture_output=True,text=True).stdout; ref=None
for pct in [100,50,25]:
    u=[]; w=[]; c=[]
    for _ in range(3):
        d=json.loads(subprocess.run([BIN,"decide","--sweeps","1000","--polish-ms","0","--cpu-limit",str(pct)],input=demo,capture_output=True,text=True).stdout)
        t=d['telemetry']; w.append(d['ms']); c.append(t['process_cpu_ms']); k=json.dumps([d['odds'],d['plan']],sort_keys=True); ref=ref or k; assert k==ref
        u.append(t['sample_ms'])
    print(f"--cpu-limit {pct}: wall ms {statistics.median(w):.1f}, sampler ms {statistics.median(u):.1f}, process CPU ms {statistics.median(c):.1f}; answer identical")
