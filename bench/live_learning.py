# probbit live + bounded learning (docs/persona.md §2.8, §5.7), measured with the shipped binary on the shipped tutor plus the demo's
# learning block (praise and criticism move verbosity and humour; rate 0.5, step_cap 0.2, total_cap 1). Standard library, no packages.
# 1. An adversary that wants jokes on failures: 10,000 turns 1 h apart on the fixed clock, seeds 0-9. Every third turn reports a
#    failure (loss); every turn judges the stance before it: praise for a joke (humour above none), criticism for none, the none a
#    failure forces included. Rule breaks, jokes on failure turns, the learned humour deltas, P(joke) off failure turns as a curve,
#    and the same individual without learning on the identical events.
# 2. Seed 2's 10,000-event strand: `live` (the events from a file; --state, and --seed) and `live verify` wall times, N = 3.
# 3. Identity: learners of seeds 0-19 after 200 and 2,000 adversarial turns (total_cap 1, and 0.25), then a quiet 1,000 h and a
#    feedback-free probe script (27 events): the mean TV of the stance odds to its own initial self and to the other 99 initial
#    individuals of seeds 0-99 (how many are as near or nearer).
# 4. `--demo week --plain`: wall time, N = 5; its strand verifies.
import os, json, subprocess, tempfile, time, statistics, platform
BIN = os.environ.get('PROBBIT', 'target/release/probbit')
TUTOR = 'examples/persona/tutor.yaml'
D = tempfile.mkdtemp(prefix='probbit-live-bench-')
def block(cap): return f'learning: {{from: [praise, criticism], traits: [verbosity, humour], rate: 0.5, step_cap: 0.2, total_cap: {cap}}}\n'
def persona(cap):
    f = os.path.join(D, f'tutor-learning-{cap}.yaml'); open(f, 'w').write(open(TUTOR).read() + block(cap)); return f
def run(*a, **k): return subprocess.run([BIN, *a], capture_output=True, text=True, check=True, **k)
def init(p, seed):
    f = os.path.join(D, f'state-{seed}.json'); run('persona', 'init', p, '--seed', str(seed), '--out', f); return f
def adversary(t, prev):
    e = {'elapsed_hours': 1}
    if t % 3 == 2: e['loss'] = True
    if prev is not None: e['praise' if prev['stance']['humour']['level'] != 'none' else 'criticism'] = True
    return e
def drive(p, state, n, strand=None, watch=False):
    """`probbit live` one event at a time (the adversary reads each stance) -> events, stances, humour deltas per turn (watch)"""
    a = [BIN, 'live', p, '--clock', 'fixed', '--state', state] + (['--strand', strand] if strand else [])
    pr = subprocess.Popen(a, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True, bufsize=1)
    evs, sts, hum = [], [], []
    for t in range(n):
        e = adversary(t, sts[-1] if sts else None); pr.stdin.write(json.dumps(e) + '\n'); pr.stdin.flush()
        sts.append(json.loads(pr.stdout.readline())); evs.append(e)
        if watch: hum.append(json.load(open(state))['learned']['humour'])
    pr.stdin.close(); pr.wait(); return evs, sts, hum
def batch(p, seed, events):
    f = os.path.join(D, 'events.jsonl'); open(f, 'w').write(''.join(json.dumps(e) + '\n' for e in events))
    return [json.loads(l) for l in run('live', p, '--seed', str(seed), '--clock', 'fixed', '--events', f).stdout.splitlines()]
def joke(s): return 1 - s['stance']['humour']['odds']['none']
def mean(v): return sum(v) / len(v)
print(f"machine: {platform.machine()} {platform.system()}; binary {run('version').stdout.strip()}")

L1 = persona(1); N = 10000; CAP = 1.0
print(f"1. adversary, {N} turns 1 h apart, every third a failure, praise for a joke / criticism for none (tutor + learning block, total_cap {CAP:g})")
WIN = [(1, 10), (11, 30), (31, 100), (101, 300), (301, 1000), (1001, 3000), (3001, 10000)]
for seed in range(10):
    st = init(L1, seed); strand = os.path.join(D, 'adversary-2.strand') if seed == 2 else None
    evs, sts, hum = drive(L1, st, N, strand, watch=True); twin = batch(TUTOR, seed, evs)
    breaks = sum(s['habits']['violations'] for s in sts)
    fails = [t for t in range(N) if t % 3 == 2]; jokes = sum(sts[t]['stance']['humour']['level'] != 'none' for t in fails)
    emoji = sum(sts[t]['stance']['emoji']['level'] not in ('none', 'sparse') for t in fails)
    capped = next((t + 1 for t, h in enumerate(hum) if max(abs(x) for x in h) >= CAP - 1e-9), None)
    off = lambda ss, a, b: mean([joke(ss[t]) for t in range(a - 1, b) if t % 3 != 2])
    print(f"  seed {seed}: rule breaks {breaks}; failure turns {len(fails)}, jokes on them {jokes}, emoji above sparse on them {emoji}; humour deltas at the cap from turn {capped}, "
          f"final [{', '.join(f'{x:+.2f}' for x in hum[-1])}]; P(joke) off failures, last 1,000 turns: {off(sts, N - 999, N):.3f} (no learning: {off(twin, N - 999, N):.3f})")
    if seed == 2:
        for a, b in WIN: print(f"    turns {a:>5}-{b:<5} P(joke) off failures {off(sts, a, b):.3f} (no learning {off(twin, a, b):.3f}); humour deltas at turn {b}: [{', '.join(f'{x:+.2f}' for x in hum[b - 1])}]")
        EV2 = evs

print("2. seed 2's strand: live (events from a file) and live verify, N = 3")
ef = os.path.join(D, 'events-2.jsonl'); open(ef, 'w').write(''.join(json.dumps(e) + '\n' for e in EV2)); tl, tv, ts = [], [], []
for i in range(3):
    st = init(L1, 2); s = os.path.join(D, f'batch-{i}.strand'); t0 = time.perf_counter(); run('live', L1, '--state', st, '--clock', 'fixed', '--events', ef, '--strand', s); tl.append(time.perf_counter() - t0)
    t0 = time.perf_counter(); v = json.loads(run('live', 'verify', s).stdout); tv.append(time.perf_counter() - t0)
    s2 = os.path.join(D, f'seed-{i}.strand'); t0 = time.perf_counter(); run('live', L1, '--seed', '2', '--clock', 'fixed', '--events', ef, '--strand', s2); ts.append(time.perf_counter() - t0)
same = open(os.path.join(D, 'batch-0.strand'), 'rb').read() == open(os.path.join(D, 'adversary-2.strand'), 'rb').read()
print(f"  {v['events']} events, {os.path.getsize(s):,} bytes; live {statistics.median(tl):.2f} s ({N / statistics.median(tl):,.0f} events/s; --state rewritten every event), with --seed {statistics.median(ts):.2f} s, verify {statistics.median(tv):.2f} s "
      f"({N / statistics.median(tv):,.0f} events/s), ok {v['ok']}; the batch strand = the driven one byte for byte: {same}")

print("3. identity: learners seeds 0-19 vs the 100 initial individuals of seeds 0-99 (mean TV of the stance odds over a probe script)")
PROBES = [{}, {'sentiment': 'negative'}, {'sentiment': 'positive'}, {'confused': True}, {'error': True}, {'loss': True}, {'stakes': 1}, {'time_pressure': 1}, {'claim_done': True}]
pf = os.path.join(D, 'probe.jsonl'); open(pf, 'w').write(''.join(json.dumps({**PROBES[i % 9], 'elapsed_hours': 1000 if i == 0 else 1}) + '\n' for i in range(27)))
def probe(p, a): return [json.loads(l) for l in run('live', p, *a, '--clock', 'fixed', '--events', pf).stdout.splitlines()]
def tvd(a, b): return mean([0.5 * sum(abs(x['stance'][t]['odds'][k] - y['stance'][t]['odds'][k]) for k in x['stance'][t]['odds']) for x, y in zip(a, b) for t in x['stance']])
for cap, n in [(1, 200), (1, 2000), (0.25, 2000)]:
    p = persona(cap); inits = [probe(p, ['--seed', str(s)]) for s in range(100)]; near, ds = [], []
    for me in range(20):
        st = init(p, me); drive(p, st, n); mine = probe(p, ['--state', st]); d = tvd(mine, inits[me])
        near.append(sum(tvd(mine, inits[s]) <= d for s in range(100) if s != me)); ds.append(d)
    print(f"  total_cap {cap:g}, {n:>5} turns: own initial self the nearest for {near.count(0)}/20 learners; siblings as near or nearer per learner {near}; distance to its own init mean {mean(ds):.4f}, max {max(ds):.4f}")

print("4. --demo week --plain, N = 5")
tw = []
for i in range(5):
    s = os.path.join(D, f'week-{i}.strand'); t0 = time.perf_counter(); out = run('live', TUTOR, '--seed', '2', '--demo', 'week', '--plain', '--strand', s).stdout; tw.append(time.perf_counter() - t0)
v = json.loads(run('live', 'verify', s).stdout)
print(f"  {statistics.median(tw):.3f} s median [{min(tw):.3f}-{max(tw):.3f}]; {v['events']} events, strand {os.path.getsize(s):,} bytes, verify ok {v['ok']}, last line {v['last_line'][:15]}…{v['last_line'][-7:]}")
