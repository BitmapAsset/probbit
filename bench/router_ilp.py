# BENCHMARKS §2/§6: the router's single best plan vs an ILP solver. Needs a Python with scipy >= 1.9 (HiGHS MILP), e.g.
# `python3 bench/router_ilp.py` (numpy + scipy); the benchmark dependency is NOT a probbit dependency.
# ILP: max sum h[i,a] x[i,a] + lam * sum_{same-group pairs i<j} sum_a y[i,j,a]; each task on exactly one allowed worker; worker
# loads <= cap; y <= x[i,a], y <= x[j,a] (lam > 0, so y = x_i*x_j at the optimum). HiGHS proves optimality (mip_gap 0 at the end).
# probbit = `probbit decide` at defaults (exact tiers, then 200 ms sampler + gate + 50 ms polish). N = 5 timing repeats per solver.
import os, sys, json, subprocess, statistics, time
import numpy as np
from scipy.optimize import milp, LinearConstraint, Bounds
from scipy.sparse import lil_matrix
BIN=os.environ.get('PROBBIT', 'target/release/probbit'); N=int(os.environ.get('N', '5'))
def ilp(doc):
    W=[w['id'] for w in doc['workers']]; cap=[w['cap'] for w in doc['workers']]; T=doc['tasks']; lam=doc['affinity']; A=len(W)
    xs={}; c=[]
    for i,t in enumerate(T):
        for a,w in enumerate(W):
            if w in t['allowed']: xs[(i,a)]=len(c); c.append(-t['scores'][w])
    grp={}
    for i,t in enumerate(T): grp.setdefault(t['group'],[]).append(i)
    ys=[]
    for g in grp.values():
        for p in range(len(g)):
            for q in range(p+1,len(g)):
                for a in range(A):
                    if (g[p],a) in xs and (g[q],a) in xs: ys.append((xs[(g[p],a)],xs[(g[q],a)],len(c))); c.append(-lam)
    nv=len(c); rows=len(T)+A+2*len(ys); M=lil_matrix((rows,nv)); lo=np.full(rows,-np.inf); hi=np.zeros(rows); r=0
    for i in range(len(T)):
        for a in range(A):
            if (i,a) in xs: M[r,xs[(i,a)]]=1
        lo[r]=hi[r]=1; r+=1
    for a in range(A):
        for i in range(len(T)):
            if (i,a) in xs: M[r,xs[(i,a)]]=1
        hi[r]=cap[a]; r+=1
    for (u,v,y) in ys:
        M[r,y]=1; M[r,u]=-1; r+=1; M[r,y]=1; M[r,v]=-1; r+=1
    integ=np.zeros(nv); integ[:len(c)-len(ys)]=1
    t0=time.perf_counter(); res=milp(np.array(c),constraints=LinearConstraint(M.tocsr(),lo,hi),integrality=integ,bounds=Bounds(0,1),options={'time_limit':120}); ms=(time.perf_counter()-t0)*1e3
    return -res.fun, ms, res.status, nv, rows
def logw(doc, plan):
    T=doc['tasks']; lam=doc['affinity']; s=sum(t['scores'][plan[t['id']]] for t in T); grp={}
    for t in T: grp.setdefault(t['group'],[]).append(plan[t['id']])
    return s+lam*sum(1 for g in grp.values() for p in range(len(g)) for q in range(p+1,len(g)) if g[p]==g[q])
print(f"machine: Apple M4, 16 GB; scipy HiGHS milp; N = {N} timing repeats; columns: queue seed | ILP optimum, ms median [min-max], status | probbit verdict, plan log w (recomputed), gap nats, ms median")
for args in (["--tasks","30"],["--tasks","300","--seed","7"],["--tasks","300","--seed","8"],["--tasks","300","--seed","9"],["--tasks","300","--seed","10"],["--tasks","300","--hard"]):
    js=subprocess.run([BIN,"demo"]+args,capture_output=True,text=True).stdout; doc=json.loads(js)
    runs=[ilp(doc) for _ in range(N)]; opt=runs[0][0]; ims=[r[1] for r in runs]
    ps=[]; pl=None
    for _ in range(N):
        d=json.loads(subprocess.run([BIN,"decide"],input=js,capture_output=True,text=True).stdout); ps.append(d['ms']); pl=d
    lw=logw(doc,pl['plan'])
    print(f"{' '.join(args)} | ILP {opt:.3f}, {statistics.median(ims):.1f} [{min(ims):.1f}-{max(ims):.1f}] ms, status {runs[0][2]}, {runs[0][3]} vars x {runs[0][4]} rows | probbit {pl['verdict']}, {lw:.3f} (reported {pl['plan_logw']}), gap {opt-lw:.3f}, {statistics.median(ps):.1f} ms")
