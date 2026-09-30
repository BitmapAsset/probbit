# What the memory cap costs where the gate is marginal: 300-task --hard demo, decide --budget-ms 1000, 5 seeds,
# unbounded vs --mem-limit-mb 8 / 2 (thinning x ~7 / ~27): released tasks, verdicts, peak RSS. Run alone.
import os, json, subprocess, statistics
BIN=os.environ.get('PBIT', 'target/release/pbit')
demo=subprocess.run([BIN,"demo","--tasks","300","--hard"],capture_output=True,text=True).stdout
print("machine: Apple M4 (4P+6E), 16 GB; 300-task --hard demo, decide --budget-ms 1000 (polish default), seeds 1-5")
for lab,extra in [("unbounded",[]),("--mem-limit-mb 8",["--mem-limit-mb","8"]),("--mem-limit-mb 2",["--mem-limit-mb","2"])]:
    rel=[]; v=[]; rss=[]; rows=[]; sw=[]
    for seed in range(1,6):
        d=json.loads(subprocess.run([BIN,"decide","--budget-ms","1000","--seed",str(seed)]+extra,input=demo,capture_output=True,text=True).stdout)
        rel.append(len(d['released'])); v.append(d['verdict']); rss.append(d['telemetry']['peak_rss_mb']); rows.append(d['telemetry']['traj_rows']); sw.append(d['telemetry']['sweeps'])
    print(f"{lab}: released {rel} (median {statistics.median(rel)}), verdicts {v}, peak RSS median {statistics.median(rss):.1f} MB, traj rows median {statistics.median(rows)}, sweeps median {statistics.median(sw)}")
