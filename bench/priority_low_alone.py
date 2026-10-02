# The cost of --priority low (nice 10 + PRIO_DARWIN_BG on macOS) to probbit itself on an otherwise idle machine:
# 400-spin ring, 4 chains x 4000 sweeps on 4 threads, fixed work, 5 alternating runs, wall ms median [IQR]; answer identical.
import os, json, subprocess, statistics
BIN=os.environ.get('PROBBIT', 'target/release/probbit')
n=400; ring=json.dumps({"probbit_ir":1,"values":["-","+"],"vars":[{"id":f"s{i}","h":{"+":0.1*((i%7)-3)}} for i in range(n)],"pairs":[{"i":f"s{i}","j":f"s{(i+1)%n}","table":[[0.4,-0.4],[-0.4,0.4]]} for i in range(n)]})
r={}; ref=None
for _ in range(5):
    for p in ["normal","low"]:
        d=json.loads(subprocess.run([BIN,"run","--op","sample","--sweeps","4000","--polish-ms","0","--priority",p],input=ring,capture_output=True,text=True).stdout)
        r.setdefault(p,[]).append((d['ms'],d['telemetry']['process_cpu_ms'],d['telemetry']['nice'])); k=json.dumps(d['marginals'],sort_keys=True); ref=ref or k; assert k==ref
print("machine: Apple M4 (4P+6E), 16 GB, otherwise idle")
for p,v in r.items():
    w=[x[0] for x in v]; q=statistics.quantiles(w,n=4); print(f"--priority {p}: wall ms {statistics.median(w):.1f} [{q[0]:.1f}-{q[2]:.1f}], CPU ms {statistics.median(x[1] for x in v):.1f}, nice {v[0][2]}; answer identical: yes")
print(f"low / normal wall: x{statistics.median(x[0] for x in r['low'])/statistics.median(x[0] for x in r['normal']):.2f}")
