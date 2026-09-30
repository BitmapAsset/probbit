# --mem-limit-mb. (A) oracle kill test: router sampler forced (--mode sample) on demos whose exact odds come from the exact
# tiers; unthinned vs a tight cap (heavy thinning): false releases = released task with TV(sampler odds, exact odds) > tv_tol.
# (B) memory: 300-task demo, --budget-ms 3000, peak RSS without / with --mem-limit-mb 16 (3 runs each).
import os, json, subprocess, statistics
BIN=os.environ.get('PBIT', 'target/release/pbit')
def run(args, inp): p=subprocess.run([BIN]+args,input=inp,capture_output=True,text=True); return json.loads(p.stdout)
def tv(a,b): ks=set(a)|set(b); return 0.5*sum(abs(a.get(k,0)-b.get(k,0)) for k in ks)
print("machine: Apple M4 (4P+6E), 16 GB")
tot={}
for tasks,hard in [(18,False),(24,False),(24,True)]:
    for seed in range(1,9):
        demo=subprocess.run([BIN,"demo","--tasks",str(tasks),"--seed",str(seed)]+(["--hard"] if hard else []),capture_output=True,text=True).stdout
        ex=run(["decide","--polish-ms","0"],demo); assert ex['verdict']=='exact', ex['verdict']
        for lab,extra in [("unbounded",[]),("mem 1 MB",["--mem-limit-mb","1"])]:
            d=run(["decide","--mode","sample","--budget-ms","150","--polish-ms","0","--seed",str(seed)]+extra,demo)
            tol=d['gate']['tv_tol']; rel=d['released']; bad=[t for t in rel if tv(d['odds'][t],ex['odds'][t])>tol]
            mx=max([tv(d['odds'][t],ex['odds'][t]) for t in rel],default=0)
            r=tot.setdefault((tasks,hard,lab),[0,0,0,0.0,[] ,[]]); r[0]+=1; r[1]+=len(rel); r[2]+=len(bad); r[3]=max(r[3],mx); r[4].append(d['telemetry']['traj_rows']); r[5].append(d['verdict'])
for (tasks,hard,lab),r in tot.items():
    print(f"(A) {tasks} tasks{' --hard' if hard else ''}, {lab}: {r[0]} instances, released {r[1]} tasks, FALSE {r[2]} (max TV of released {r[3]:.3f}), traj rows median {statistics.median(r[4])}, verdicts {dict((v,r[5].count(v)) for v in set(r[5]))}")
demo=subprocess.run([BIN,"demo","--tasks","300"],capture_output=True,text=True).stdout
for lab,extra in [("unbounded",[]),("--mem-limit-mb 16",["--mem-limit-mb","16"])]:
    rss=[]; rows=[]; v=[]
    for _ in range(3):
        d=run(["decide","--budget-ms","3000"]+extra,demo); rss.append(d['telemetry']['peak_rss_mb']); rows.append(d['telemetry']['traj_rows']); v.append(d['verdict'])
    print(f"(B) 300 tasks, --budget-ms 3000, {lab}: peak RSS MB {[round(x,1) for x in rss]} (median {statistics.median(rss):.1f}), traj rows {rows}, verdicts {v}, sweeps {d['telemetry']['sweeps']}")
