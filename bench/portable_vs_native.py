# Portable default build vs -C target-cpu=native (Apple M4, aarch64-apple-darwin: native adds only bf16/bti/i8mm over
# the apple-m1 baseline). 5 alternating runs each: probbit-core kernels example, probbit stats self-test, 300-task decide fixed work.
import os, json, subprocess, statistics, re
P=os.environ.get('PROBBIT_DIR', 'target/release'); N=os.environ.get('PROBBIT_NATIVE_DIR', '/tmp/probbit-native-target/release')
demo=subprocess.run([P+"/probbit","demo","--tasks","300"],capture_output=True,text=True).stdout
res={}; ans={}
def add(var,key,v): res.setdefault((key,var),[]).append(v)
for _ in range(5):
    for var,d in [("portable",P),("native",N)]:
        out=subprocess.run([d+"/examples/kernels"],capture_output=True,text=True).stdout
        for line in out.splitlines():
            m=re.match(r"(.+?)\s*:\s*([0-9.e+]+) updates/s",line)
            if m: add(var,m.group(1).strip(),float(m.group(2)))
            if line.startswith("chk"): ans.setdefault(("chk",var),set()).add(line)
        st=json.loads(subprocess.run([d+"/probbit","stats"],capture_output=True,text=True).stdout)['self_test']
        add(var,"probbit stats IR ring, 1 thread",st['site_updates_per_s_1_thread']); add(var,"probbit stats IR ring, 4 threads",st['site_updates_per_s'])
        dd=json.loads(subprocess.run([d+"/probbit","decide","--sweeps","2000","--polish-ms","0"],input=demo,capture_output=True,text=True).stdout)
        add(var,"probbit decide 300 tasks, 4 thr (sampler)",dd['telemetry']['site_updates_per_s']); ans.setdefault(("decide",var),set()).add(json.dumps([dd['odds'],dd['plan'],dd['gate']],sort_keys=True))
print("machine: Apple M4 (4P+6E), 16 GB, rustc 1.98.1; 5 alternating runs; medians [IQR]; ratio = native / portable")
for key in dict.fromkeys(k for k,_ in res):
    a=res[(key,"portable")]; b=res[(key,"native")]; qa=statistics.quantiles(a,n=4); qb=statistics.quantiles(b,n=4)
    print(f"{key}: portable {statistics.median(a):.4g} [{qa[0]:.4g}-{qa[2]:.4g}] | native {statistics.median(b):.4g} [{qb[0]:.4g}-{qb[2]:.4g}] | ratio {statistics.median(b)/statistics.median(a):.3f}")
print("kernels checksum identical across builds:", ans[("chk","portable")]==ans[("chk","native")], "| decide answer (odds+plan+gate) identical across builds:", ans[("decide","portable")]==ans[("decide","native")] and len(ans[("decide","portable")])==1)
