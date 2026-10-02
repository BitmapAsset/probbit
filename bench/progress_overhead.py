# Cost of --progress (monitor thread + 1 atomic add / 64 sweeps / chain) at fixed work; answer must be identical.
import os, json, subprocess, statistics
BIN=os.environ.get('PROBBIT', 'target/release/probbit')
demo=subprocess.run([BIN,"demo","--tasks","300"],capture_output=True,text=True).stdout
n=400; ring=json.dumps({"probbit_ir":1,"values":["-","+"],"vars":[{"id":f"s{i}","h":{"+":0.1*((i%7)-3)}} for i in range(n)],"pairs":[{"i":f"s{i}","j":f"s{(i+1)%n}","table":[[0.4,-0.4],[-0.4,0.4]]} for i in range(n)]})
tiny=json.dumps({"probbit_ir":1,"values":["-","+"],"vars":[{"id":"a"},{"id":"b"},{"id":"c"}],"pairs":[{"i":"a","j":"b","table":[[0.3,0],[0,0.3]]}]})
print("machine: Apple M4 (4P+6E), 16 GB; fixed work, polish 0, 4 chains / 4 threads; 7 runs alternating off/on; medians [IQR] of telemetry.site_updates_per_s (sampler only)")
cases=[("router 300-task demo, decide --sweeps 2000",[BIN,"decide","--sweeps","2000","--polish-ms","0"],demo,'odds'),
       ("IR 400-spin ring, run --op sample --sweeps 4000",[BIN,"run","--op","sample","--sweeps","4000","--polish-ms","0"],ring,'marginals'),
       ("IR 3-var toy, run --op sample --sweeps 400000",[BIN,"run","--op","sample","--sweeps","400000","--polish-ms","0"],tiny,'marginals')]
for name,cmd,inp,key in cases:
    r={False:[],True:[]}; ref=None; nl=[]
    for _ in range(7):
        for on in (False,True):
            p=subprocess.run(cmd+(["--progress","50"] if on else []),input=inp,capture_output=True,text=True); d=json.loads(p.stdout)
            r[on].append(d['telemetry']['site_updates_per_s']); k=json.dumps(d[key],sort_keys=True); ref=ref or k; assert k==ref
            if on: nl.append(len(p.stderr.splitlines()))
    f=lambda v:(statistics.median(v)/1e6,statistics.quantiles(v,n=4)[0]/1e6,statistics.quantiles(v,n=4)[2]/1e6)
    a,b=f(r[False]),f(r[True])
    print(f"{name}: off {a[0]:.2f} M/s [{a[1]:.2f}-{a[2]:.2f}], on {b[0]:.2f} M/s [{b[1]:.2f}-{b[2]:.2f}] ({(b[0]/a[0]-1)*100:+.1f}%), progress lines/run median {statistics.median(nl)}; answer identical: yes")
