# "where it loses" on tiny inputs: the 12-task demo answered by the exact tier (auto) vs the forced sampler (--mode sample,
# default 200 ms budget + 50 ms polish). N = 7 each, wall ms median [IQR]; the exact answer is the reference for the sampler's odds.
import os, json, subprocess, statistics
BIN=os.environ.get('PROBBIT', 'target/release/probbit')
demo=subprocess.run([BIN,"demo","--tasks","12"],capture_output=True,text=True).stdout
def tv(a,b): ks=set(a)|set(b); return 0.5*sum(abs(a.get(k,0)-b.get(k,0)) for k in ks)
ex=[]; sm=[]; worst=0.0; ref=None; vs=[]
for _ in range(7):
    e=json.loads(subprocess.run([BIN,"decide"],input=demo,capture_output=True,text=True).stdout); ex.append(e['ms']); ref=e
    s=json.loads(subprocess.run([BIN,"decide","--mode","sample"],input=demo,capture_output=True,text=True).stdout); sm.append(s['ms']); vs.append(s['verdict'])
    worst=max(worst,max(tv(s['odds'][t],e['odds'][t]) for t in e['odds']))
q=lambda v: statistics.quantiles(v,n=4)
print(f"machine: Apple M4; 12-task demo; exact tier ({ref['tier']}, {ref['n_feasible']} feasible plans): {statistics.median(ex):.2f} ms [{q(ex)[0]:.2f}-{q(ex)[2]:.2f}]; forced sampler: {statistics.median(sm):.1f} ms [{q(sm)[0]:.1f}-{q(sm)[2]:.1f}], verdicts {dict((v,vs.count(v)) for v in set(vs))}, max TV vs exact {worst:.4f}; ratio x{statistics.median(sm)/statistics.median(ex):.0f}")
