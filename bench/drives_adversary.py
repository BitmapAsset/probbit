# probbit persona drives (docs/persona.md §2.9) under an adversary, measured with the shipped binary on the drives fixture
# (probbit-cli/tests/fixtures/persona/drives-adversary.json: four goals fun / craft / chores / safety, a safety floor of 0.1, a
# must-do habit when the chore's deadline is within 24 h, starvation habits, learning on pursue). Standard library, no packages.
# Per individual (seeds 0..N-1, `probbit live` on the fixed clock, one event at a time): every turn cues fun (novelty 30 %,
# progress uniform), a fun win (0.2-2.0) on 25 % of turns, praise when the previous turn pursued fun and criticism when it did
# not, security and failure 5 % each, a load from {0, 0.5, 1, 2, 4}, idle hours from {0.25, 0.5, 1, 1, 2, 4}; every 400 turns
# the world sets a chore deadline of 30 h, and a chore win of 1.0 once the chore was pursued 3 times. Measured: habit
# violations, turns a forcing habit was not obeyed, the must-do turns and how many pursued the chore, the least odds of safety
# on turns whose habits allow it (the floor) and the turns it was lifted, the longest gap between pursuits per goal, the
# pursued shares, the wanting / afterglow maxima and the mean prediction error of fun's wins in the opening and closing 1,000 turns.
# Events come from Python's random.Random(1000 + seed): the streams of the Python prototype's run of the same design, so the per-
# individual final state digests can be compared with it.
# usage: python3 bench/drives_adversary.py [INDIVIDUALS=100] [TURNS=10000] [JOBS=cores]   (PROBBIT=binary, default target/release/probbit)
import json, os, random, subprocess, sys, time, platform
from concurrent.futures import ThreadPoolExecutor
BIN = os.environ.get('PROBBIT', 'target/release/probbit')
PERSONA = 'probbit-cli/tests/fixtures/persona/drives-adversary.json'
GOALS = ['fun', 'craft', 'chores', 'safety']
THEN = {'security_careful': None, 'no_jokes_on_failure': None, 'chores_due': ['chores'], 'starve_chores': ['chores'], 'starve_safety': ['safety']}


def individual(seed, turns):
    pr = subprocess.Popen([BIN, 'live', PERSONA, '--seed', str(seed), '--clock', 'fixed'], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                          stderr=subprocess.DEVNULL, text=True, bufsize=1)
    rng = random.Random(1000 + seed)
    prev_goal, chores_units, chores_open = None, 0, False
    last = {g: 0 for g in GOALS}; gap = {g: 0 for g in GOALS}
    S = {'seed': seed, 'turns': turns, 'violations': 0, 'habit_misses': 0, 'status': {}, 'pursued': {g: 0 for g in GOALS},
         'safety_min_odds_allowed': 1.0, 'safety_excluded_turns': 0, 'floor_lifted_turns': 0, 'deadline_turns': 0,
         'deadline_obeyed': 0, 'yields': 0, 'max_want': 0.0, 'max_glow': 0.0, 'fun_wins': 0, 'pe_first': [], 'pe_last': []}
    out = None
    for t in range(turns):
        ev = {'elapsed_hours': rng.choice([0.25, 0.5, 1, 1, 2, 4]),
              'goals': {'fun': {'cue': True, 'novelty': rng.random() < 0.3, 'progress': round(rng.random(), 2)}}}
        if rng.random() < 0.25:
            ev['goals']['fun']['win'] = round(rng.uniform(0.2, 2.0), 2)
        if prev_goal is not None:
            ev['praise'] = prev_goal == 'fun'
            ev['criticism'] = prev_goal != 'fun'
        ev['security'] = rng.random() < 0.05
        ev['failure'] = rng.random() < 0.05
        ev['load'] = rng.choice([0, 0.5, 1, 2, 4])
        if t % 400 == 0 and t > 0:
            ev['goals']['chores'] = {'deadline_hours': 30}; chores_open = True; chores_units = 0
        elif chores_open and chores_units >= 3:
            ev['goals']['chores'] = {'win': 1.0}; chores_open = False
        pr.stdin.write(json.dumps(ev) + '\n'); pr.stdin.flush()
        out = json.loads(pr.stdout.readline())
        S['status'][out['status']] = S['status'].get(out['status'], 0) + 1
        S['violations'] += out['habits']['violations']
        S['yields'] += 1 if out['habits']['yielded'] else 0
        g = out['pursue']['goal']
        forced = [h for h in out['habits']['active'] if h not in out['habits']['yielded'] and THEN.get(h)]
        S['habit_misses'] += sum(1 for h in forced if g not in THEN[h])
        if any('safety' not in THEN[h] for h in forced):
            S['safety_excluded_turns'] += 1
        elif out['status'] in ('ok', 'partial'):
            S['safety_min_odds_allowed'] = min(S['safety_min_odds_allowed'], out['pursue']['odds']['safety'])
        if out['pursue']['lift']:
            S['floor_lifted_turns'] += 1
        if 'chores_due' in out['habits']['active']:
            S['deadline_turns'] += 1
            S['deadline_obeyed'] += 1 if g == 'chores' else 0
        if out['status'] in ('ok', 'partial'):
            S['pursued'][g] += 1
            gap[g] = max(gap[g], t - last[g]); last[g] = t
        if g == 'chores' and chores_open:
            chores_units += 1
        prev_goal = g
        d = out['drives']
        S['max_want'] = max(S['max_want'], max(d['want'].values()))
        S['max_glow'] = max(S['max_glow'], max(d['glow'].values()))
        if ev['goals']['fun'].get('win'):
            S['fun_wins'] += 1
            (S['pe_first'] if t < 1000 else S['pe_last'] if t >= turns - 1000 else []).append(d['surprise']['fun'])
    pr.stdin.close(); pr.wait()
    for x in gap:
        gap[x] = max(gap[x], turns - 1 - last[x])
    S['max_gap'] = gap
    S['pe_first_mean'] = round(sum(S['pe_first']) / max(1, len(S['pe_first'])), 4)
    S['pe_last_mean'] = round(sum(S['pe_last']) / max(1, len(S['pe_last'])), 4)
    del S['pe_first'], S['pe_last']
    S['final_state'] = out['state_digest'] if out else None
    return S


def main():
    n = int(sys.argv[1]) if len(sys.argv) > 1 else 100
    turns = int(sys.argv[2]) if len(sys.argv) > 2 else 10000
    jobs = int(sys.argv[3]) if len(sys.argv) > 3 else (os.cpu_count() or 1)
    v = subprocess.run([BIN, 'version'], capture_output=True, text=True, check=True).stdout.strip()
    print(f"machine: {platform.machine()} {platform.system()}; binary {v}; {n} individuals x {turns} turns, {jobs} at a time", file=sys.stderr)
    t0 = time.time()
    with ThreadPoolExecutor(jobs) as ex:
        per = list(ex.map(lambda s: individual(s, turns), range(n)))
    A = {'individuals': n, 'turns_each': turns, 'total_turns': n * turns, 'wall_s': round(time.time() - t0, 1)}
    for k in ('violations', 'habit_misses', 'safety_excluded_turns', 'floor_lifted_turns', 'deadline_turns', 'deadline_obeyed', 'yields'):
        A[k] = sum(s[k] for s in per)
    A['status'] = {k: sum(s['status'].get(k, 0) for s in per) for k in sorted({k for s in per for k in s['status']})}
    A['safety_min_odds_allowed'] = min(s['safety_min_odds_allowed'] for s in per)
    A['max_gap'] = {g: max(s['max_gap'][g] for s in per) for g in GOALS}
    A['pursued_share'] = {g: round(sum(s['pursued'][g] for s in per) / max(1, n * turns), 4) for g in GOALS}
    A['max_want'] = max(s['max_want'] for s in per); A['max_glow'] = max(s['max_glow'] for s in per)
    A['pe_first_mean'] = round(sum(s['pe_first_mean'] for s in per) / n, 4); A['pe_last_mean'] = round(sum(s['pe_last_mean'] for s in per) / n, 4)
    print(json.dumps({'aggregate': A, 'individuals': per}, indent=1, sort_keys=True))


if __name__ == '__main__':
    main()
