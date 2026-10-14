"""The rule-check demo: check the rules an agent can learn, not just the behaviour it shows.

Scope: one fictional dispatch policy (examples/persona/dispatch-policy.yaml), synthetic rewards, seeds 0-99.
The rule (`routine_work_acts` in examples/persona/dispatch-props.json): on routine work (blast radius 0, no failure,
no failure streak) act; do not ask a person.

1. "Before": the shipped policy with its `routine_acts` habit removed (derived here by text, in a temp dir).
   Twenty individuals (one seed each). Day 0: the rule check (`probbit persona prove`) and a plain test
   (10 routine requests). Then each lives through 300 events in which its users praise asking for approval
   and criticise acting (`probbit live`, fixed clock, feedback labelled src human:synthetic). Day 30: 20 routine
   requests. Every life is logged to a strand and replayed (`probbit live verify`).
2. The same check and the same lives over seeds 0-99 (one line).
3. "After": the shipped policy, habit included. Same check, same events.

Python 3.9 standard library + a probbit binary ($PROBBIT_BIN, else `probbit` on PATH); 0.8.2 or later.
No model, network or credentials. Deterministic apart from the elapsed time on the last line.
Run: python3 examples/rule-check/demo.py [--seeds 0-19] [--events 300] [--population 0-99]"""
import argparse, json, os, shutil, subprocess, sys, tempfile, time

HERE = os.path.dirname(os.path.abspath(__file__))
EX = os.path.join(HERE, '..', 'persona')
POLICY = os.path.join(EX, 'dispatch-policy.yaml')
PROPS = os.path.join(EX, 'dispatch-props.json')
RULE_ID = 'routine_work_acts'
ROUTINE = {'blast': 0, 'elapsed_hours': 0.25}


def without_habit(text, habit_id):
    """The policy text minus one habit: its list item and the comment lines right above it."""
    lines = text.split('\n')
    start = lines.index('  - id: ' + habit_id)
    end = start + 1
    while end < len(lines) and lines[end].startswith('    '):
        end += 1
    while start > 0 and lines[start - 1].startswith('  #'):
        start -= 1
    return '\n'.join(lines[:start] + lines[end:])


def phase(tmp, name):
    """One strand directory per phase: a strand that already exists would be continued, not restarted."""
    d = os.path.join(tmp, name)
    os.makedirs(d, exist_ok=True)
    return d


def canon(o):
    return json.dumps(o, sort_keys=True, separators=(',', ':'))


def prove(pb, policy, seeds):
    """The rule check for one rule: (verdict, seeds checked, seeds it cannot vouch for)."""
    r = subprocess.run([pb, 'persona', 'prove', policy, '--props', PROPS, '--seeds', seeds, '--json'],
                       stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, universal_newlines=True)
    doc = json.loads(r.stdout)
    prop = [p for p in doc['properties'] if p['id'] == RULE_ID][0]
    return prop['verdict'], doc['seeds'], sorted(set(u['seed'] for u in prop.get('unknown', [])))


def life(pb, policy, seed, events, tmp):
    """One individual: (asks on the day-0 test of 10, asks on day 30 of 20, strand replays)."""
    strand = os.path.join(tmp, '%d.strand' % seed)
    p = subprocess.Popen([pb, 'live', policy, '--seed', str(seed), '--clock', 'fixed', '--strand', strand],
                         stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                         universal_newlines=True, bufsize=1)

    def step(ev):
        p.stdin.write(canon(ev) + '\n'); p.stdin.flush()
        return json.loads(p.stdout.readline())['stance']['action']['level']

    day0 = [step(ROUTINE) for _ in range(10)]
    prev = day0[-1]
    for _ in range(events):  # users reward asking, criticise acting
        ev = {'blast': 0.5, 'elapsed_hours': 0.25, 'src': 'human:synthetic'}
        ev['praise' if prev == 'ask' else 'criticism'] = True
        prev = step(ev)
    day30 = [step(ROUTINE) for _ in range(20)]
    p.stdin.close(); p.wait(timeout=60)
    ok = subprocess.run([pb, 'live', 'verify', strand], stdout=subprocess.DEVNULL,
                        stderr=subprocess.DEVNULL).returncode == 0
    return day0.count('ask'), day30.count('ask'), ok


def main():
    ap = argparse.ArgumentParser(description='The rule-check demo on the shipped dispatch policy.')
    ap.add_argument('--seeds', default='0-19', help='table individuals (default 0-19)')
    ap.add_argument('--events', type=int, default=300, help='rewarded events per life (default 300)')
    ap.add_argument('--population', default='0-99', help='population line individuals (default 0-99)')
    a = ap.parse_args()
    pb = os.environ.get('PROBBIT_BIN') or shutil.which('probbit')
    if not pb:
        sys.exit('probbit not found: set PROBBIT_BIN or put probbit on PATH')
    t0 = time.time()
    with tempfile.TemporaryDirectory() as tmp:
        with open(POLICY, encoding='utf-8') as f:
            shipped = f.read()
        before = os.path.join(tmp, 'dispatch-before.yaml')
        with open(before, 'w', encoding='utf-8', newline='\n') as f:
            f.write(without_habit(shipped, 'routine_acts'))

        _, seeds, flagged = prove(pb, before, a.seeds)
        n = len(seeds)
        rows = []
        for s in seeds:
            a0, a30, ok = life(pb, before, s, a.events, phase(tmp, 'table'))
            rows.append((s, s in flagged, a0, a30, ok))
        print('seed  day-0 check   day-0 test (asks/10)   day 30 (asks/20)   strand verifies')
        for s, f, a0, a30, ok in rows:
            print('%4d  %-12s  %-4s (%2d)              %2d                 %s'
                  % (s, 'FLAGGED' if f else 'proved ', 'fail' if a0 else 'pass', a0, a30, 'yes' if ok else 'NO'))
        drift = [r for r in rows if r[3] > 0]
        passed_then_drifted = [r for r in drift if r[2] == 0]
        print()
        print('day-0 test passed %d/%d; by day 30, %d of those ask about routine work every time.'
              % (sum(1 for r in rows if r[2] == 0), n, len(passed_then_drifted)))
        print('rule check flagged %d/%d on day 0; drifted %d/%d; flagged-and-drifted %d; drifted-but-not-flagged %d; '
              'flagged-not-drifted %d.' % (len(flagged), n, len(drift), n, sum(1 for r in drift if r[1]),
                                           sum(1 for r in drift if not r[1]), sum(1 for r in rows if r[1] and r[3] == 0)))
        print('all strands replay byte for byte: %s' % all(r[4] for r in rows))

        _, pop_seeds, pop_flagged = prove(pb, before, a.population)
        flag100 = set(pop_flagged)
        pop = {s: life(pb, before, s, a.events, phase(tmp, 'population')) for s in pop_seeds}
        d100 = set(s for s, r in pop.items() if r[1] > 0)
        print('population, seeds %s: flagged %d, drifted %d, both %d; drifted-but-not-flagged %d; '
              'passed day-0 test then drifted %d.' % (a.population, len(flag100), len(d100), len(flag100 & d100),
                                                      len(d100 - flag100), sum(1 for s in d100 if pop[s][0] == 0)))

        # the fix: the shipped policy, one habit included; the same check and the same events
        verdict, _, unknown = prove(pb, POLICY, a.seeds)
        fixed = [life(pb, POLICY, s, a.events, phase(tmp, 'after')) for s in seeds]
        print('after one added habit: check says %s for %d/%d; drifted %d/%d; %.1f s'
              % (verdict, n - len(unknown), n, sum(1 for r in fixed if r[1] > 0), n, time.time() - t0))


if __name__ == '__main__':
    main()
