# Does --priority low yield under contention? Foreground = probbit run, 400-spin ring, 10 chains x 20000 sweeps on 10 threads,
# fixed work, normal priority. Background hog = probbit run, same ring, 10 chains / 10 threads, --budget-ms 4000, at normal vs low priority.
# (Deliberately concurrent: this measures sharing, not probbit alone.) 5 reps per condition, foreground wall ms median [IQR].
import os, json, subprocess, statistics, time
BIN=os.environ.get('PROBBIT', 'target/release/probbit')
n=400; ring=json.dumps({"probbit_ir":1,"values":["-","+"],"vars":[{"id":f"s{i}","h":{"+":0.1*((i%7)-3)}} for i in range(n)],"pairs":[{"i":f"s{i}","j":f"s{(i+1)%n}","table":[[0.4,-0.4],[-0.4,0.4]]} for i in range(n)]})
fg=[BIN,"run","--op","sample","--sweeps","20000","--chains","10","--threads","10","--polish-ms","0"]
def fore():
    d=json.loads(subprocess.run(fg,input=ring,capture_output=True,text=True).stdout); return d['ms'], d['telemetry']['process_cpu_ms']
res={}
print("machine: Apple M4 (4P+6E = 10 cores), 16 GB")
for rep in range(5):
    for cond in ["alone","bg normal","bg low"]:
        bg=None
        if cond!="alone":
            args=[BIN,"run","--op","sample","--budget-ms","4000","--chains","10","--threads","10","--polish-ms","0"]+(["--priority","low"] if cond=="bg low" else [])
            bg=subprocess.Popen(args,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True); bg.stdin.write(ring); bg.stdin.close(); time.sleep(0.4)
        ms,cpu=fore(); res.setdefault(cond,[]).append((ms,cpu))
        if bg: 
            bg.kill(); bg.wait()
        time.sleep(0.3)
for cond,v in res.items():
    w=[x[0] for x in v]; c=[x[1] for x in v]; q=statistics.quantiles(w,n=4)
    print(f"foreground {cond}: wall ms {statistics.median(w):.1f} [{q[0]:.1f}-{q[2]:.1f}], fg CPU ms {statistics.median(c):.1f}")
a=statistics.median(x[0] for x in res["alone"]); print(f"slowdown vs alone: bg normal x{statistics.median(x[0] for x in res['bg normal'])/a:.2f}, bg low x{statistics.median(x[0] for x in res['bg low'])/a:.2f}")
