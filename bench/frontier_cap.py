# The frontier tier's state cap (--frontier-states, default 4096) on `probbit demo --tasks N` (seed 7): verdict/tier and probbit's
# own ms (median of N = 5) at caps 4096 and 16384. A larger cap answers more queues exactly but costs more time when it declines.
import os, json, subprocess, statistics
BIN=os.environ.get('PROBBIT', 'target/release/probbit'); N=int(os.environ.get('N', '5'))
print("machine: Apple M4, 16 GB; columns: tasks | hard | cap 4096: verdict, ms | cap 16384: verdict, ms | delta ms")
for n in (24, 30, 36, 42, 48, 60, 90, 150, 300):
    for hard in ([], ["--hard"]):
        js=subprocess.run([BIN,"demo","--tasks",str(n)]+hard,capture_output=True,text=True).stdout; row=[]
        for cap in ("4096","16384"):
            ms=[]; v=None
            for _ in range(N):
                d=json.loads(subprocess.run([BIN,"decide","--frontier-states",cap],input=js,capture_output=True,text=True).stdout); ms.append(d['ms']); v=d['verdict']+("/"+d['tier'] if d.get('tier') else "")
            row.append((v, statistics.median(ms)))
        print(f"{n} | {'hard' if hard else '-'} | {row[0][0]}, {row[0][1]:.1f} | {row[1][0]}, {row[1][1]:.1f} | {row[1][1]-row[0][1]:+.1f}")
